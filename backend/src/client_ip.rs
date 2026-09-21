//! Server-derived client address resolution with explicit proxy trust.

use std::net::IpAddr;

use axum::http::HeaderMap;
#[cfg(test)]
use axum::http::HeaderName;
use ipnet::IpNet;

use crate::error::AppError;

#[cfg(test)]
const CLIENT_IP_HEADER: HeaderName = HeaderName::from_static(crate::config::CLIENT_IP_HEADER);

/// Provenance of the client address stored for a widget session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientIpSource {
    Peer,
    TrustedProxy,
}

impl ClientIpSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Peer => "peer",
            Self::TrustedProxy => "trusted_proxy",
        }
    }
}

/// Client address resolved from the TCP peer and the canonical trusted header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientIp {
    pub address: IpAddr,
    pub source: ClientIpSource,
}

/// Resolves the client address without trusting browser-controlled forwarding headers.
///
/// The `X-Tz-Client-IP` header is consulted only when the direct TCP peer belongs
/// to an explicitly configured trusted proxy network. A trusted proxy must provide
/// exactly one bare IPv4 or IPv6 address.
///
/// # Errors
///
/// Returns `BadRequest` when a trusted peer omits the canonical header, sends it more
/// than once, or provides anything other than one bare IP address.
pub fn resolve(
    peer_ip: IpAddr,
    headers: &HeaderMap,
    trusted_proxies: &[IpNet],
) -> Result<ClientIp, AppError> {
    if !trusted_proxies
        .iter()
        .any(|network| network.contains(&peer_ip))
    {
        return Ok(ClientIp {
            address: peer_ip,
            source: ClientIpSource::Peer,
        });
    }

    let header = crate::config::CLIENT_IP_HEADER;
    let mut values = headers.get_all(header).iter();
    let value = values
        .next()
        .ok_or_else(|| AppError::BadRequest(format!("trusted proxy did not provide {header}")))?;
    if values.next().is_some() {
        return Err(AppError::BadRequest(format!(
            "trusted proxy provided {header} more than once"
        )));
    }
    let value = value
        .to_str()
        .map_err(|_| AppError::BadRequest(format!("{header} is invalid")))?;
    let address = value
        .parse::<IpAddr>()
        .map_err(|_| AppError::BadRequest(format!("{header} must contain one bare IP address")))?;

    Ok(ClientIp {
        address,
        source: ClientIpSource::TrustedProxy,
    })
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use axum::http::{HeaderMap, HeaderValue};
    use ipnet::{IpNet, Ipv4Net, Ipv6Net};

    use super::{CLIENT_IP_HEADER, ClientIp, ClientIpSource, resolve};

    fn trusted_v4() -> IpNet {
        IpNet::V4(Ipv4Net::new(Ipv4Addr::new(10, 0, 0, 0), 8).unwrap())
    }

    fn trusted_v6() -> IpNet {
        IpNet::V6(Ipv6Net::new("fd00::".parse().unwrap(), 8).unwrap())
    }

    #[test]
    fn untrusted_peer_ignores_spoofed_header() {
        let mut headers = HeaderMap::new();
        headers.insert(&CLIENT_IP_HEADER, HeaderValue::from_static("203.0.113.9"));
        let peer = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));

        assert_eq!(
            resolve(peer, &headers, &[trusted_v4()]).unwrap(),
            ClientIp {
                address: peer,
                source: ClientIpSource::Peer,
            }
        );
    }

    #[test]
    fn trusted_peer_requires_exactly_one_header() {
        let peer = IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3));
        assert!(resolve(peer, &HeaderMap::new(), &[trusted_v4()]).is_err());

        let mut duplicate = HeaderMap::new();
        duplicate.append(&CLIENT_IP_HEADER, HeaderValue::from_static("203.0.113.9"));
        duplicate.append(&CLIENT_IP_HEADER, HeaderValue::from_static("203.0.113.10"));
        assert!(resolve(peer, &duplicate, &[trusted_v4()]).is_err());
    }

    #[test]
    fn trusted_peer_rejects_lists_and_non_bare_values() {
        let peer = IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3));
        for value in ["203.0.113.9, 10.1.2.3", "203.0.113.9:443", " 203.0.113.9"] {
            let mut headers = HeaderMap::new();
            headers.insert(&CLIENT_IP_HEADER, HeaderValue::from_str(value).unwrap());
            assert!(resolve(peer, &headers, &[trusted_v4()]).is_err());
        }
    }

    #[test]
    fn trusted_peer_accepts_bare_ipv4() {
        let mut headers = HeaderMap::new();
        headers.insert(&CLIENT_IP_HEADER, HeaderValue::from_static("203.0.113.9"));

        assert_eq!(
            resolve(
                IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)),
                &headers,
                &[trusted_v4()]
            )
            .unwrap(),
            ClientIp {
                address: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)),
                source: ClientIpSource::TrustedProxy,
            }
        );
    }

    #[test]
    fn trusted_peer_accepts_bare_ipv6() {
        let mut headers = HeaderMap::new();
        headers.insert(&CLIENT_IP_HEADER, HeaderValue::from_static("2001:db8::42"));

        assert_eq!(
            resolve(
                IpAddr::V6("fd00::1".parse().unwrap()),
                &headers,
                &[trusted_v6()]
            )
            .unwrap(),
            ClientIp {
                address: IpAddr::V6("2001:db8::42".parse::<Ipv6Addr>().unwrap()),
                source: ClientIpSource::TrustedProxy,
            }
        );
    }

    #[test]
    fn only_the_canonical_proxy_header_is_trusted() {
        let peer = "10.1.2.3".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-tz-client-ip", HeaderValue::from_static("203.0.113.9"));
        let resolved = super::resolve(peer, &headers, &[trusted_v4()]).unwrap();
        assert_eq!(resolved.address, "203.0.113.9".parse::<IpAddr>().unwrap());
        let mut foreign = HeaderMap::new();
        foreign.insert(
            "x-tzomet-client-ip",
            HeaderValue::from_static("203.0.113.9"),
        );
        assert!(super::resolve(peer, &foreign, &[trusted_v4()]).is_err());
        headers.append("x-tz-client-ip", HeaderValue::from_static("203.0.113.10"));
        assert!(super::resolve(peer, &headers, &[trusted_v4()]).is_err());
    }
}
