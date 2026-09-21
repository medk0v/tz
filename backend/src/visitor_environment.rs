//! Server-side visitor geolocation and user-agent parsing.

use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use maxminddb::{
    Reader,
    geoip2::{City, Country},
};
use serde::Serialize;
use tracing::warn;
use woothee::parser::Parser;

use crate::config::GeoIpConfig;

const UNKNOWN_USER_AGENT_VALUE: &str = "UNKNOWN";

/// Coarse location derived from a server-captured IP address.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GeoIpDetails {
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub time_zone: Option<String>,
}

impl GeoIpDetails {
    fn is_empty(&self) -> bool {
        self.country_code.is_none()
            && self.country.is_none()
            && self.city.is_none()
            && self.time_zone.is_none()
    }
}

/// Normalized fields parsed from the retained raw `User-Agent` header.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UserAgentDetails {
    pub browser: Option<String>,
    pub browser_version: Option<String>,
    pub operating_system: Option<String>,
    pub operating_system_version: Option<String>,
    pub device_category: Option<String>,
}

impl UserAgentDetails {
    fn is_empty(&self) -> bool {
        self.browser.is_none()
            && self.browser_version.is_none()
            && self.operating_system.is_none()
            && self.operating_system_version.is_none()
            && self.device_category.is_none()
    }
}

/// Lazily loaded, process-wide visitor enrichment services.
#[derive(Debug)]
pub struct VisitorEnvironment {
    geo_ip: GeoIpLookup,
}

impl VisitorEnvironment {
    pub fn new(config: &GeoIpConfig) -> Self {
        Self {
            geo_ip: GeoIpLookup::new(
                config.city_database_path.clone(),
                config.country_database_path.clone(),
            ),
        }
    }

    /// Looks up an optional textual IP address without making a network request.
    pub fn geo_ip(&self, address: Option<&str>) -> Option<GeoIpDetails> {
        address
            .and_then(|value| value.parse::<IpAddr>().ok())
            .and_then(|value| self.geo_ip.lookup(value))
    }

    /// Parses an optional raw user-agent string into normalized display fields.
    pub fn user_agent(&self, user_agent: Option<&str>) -> Option<UserAgentDetails> {
        parse_user_agent(user_agent?)
    }
}

#[derive(Debug)]
struct GeoIpLookup {
    city_database_path: PathBuf,
    country_database_path: PathBuf,
    city_reader: OnceLock<Option<Arc<Reader<Vec<u8>>>>>,
    country_reader: OnceLock<Option<Arc<Reader<Vec<u8>>>>>,
}

impl GeoIpLookup {
    fn new(city_database_path: PathBuf, country_database_path: PathBuf) -> Self {
        Self {
            city_database_path,
            country_database_path,
            city_reader: OnceLock::new(),
            country_reader: OnceLock::new(),
        }
    }

    fn lookup(&self, address: IpAddr) -> Option<GeoIpDetails> {
        let mut details = GeoIpDetails::default();

        if let Some(reader) = self.city_reader()
            && let Ok(result) = reader.lookup(address)
            && let Ok(Some(record)) = result.decode::<City>()
        {
            details.city = record.city.names.english.map(str::to_owned);
            details.time_zone = record.location.time_zone.map(str::to_owned);
            details.country_code = record.country.iso_code.map(str::to_owned);
            details.country = record.country.names.english.map(str::to_owned);
        }

        if let Some(reader) = self.country_reader()
            && let Ok(result) = reader.lookup(address)
            && let Ok(Some(record)) = result.decode::<Country>()
        {
            details.country_code = record.country.iso_code.map(str::to_owned);
            details.country = record.country.names.english.map(str::to_owned);
        }

        (!details.is_empty()).then_some(details)
    }

    fn city_reader(&self) -> Option<&Reader<Vec<u8>>> {
        self.city_reader
            .get_or_init(|| load_reader(&self.city_database_path, "city"))
            .as_deref()
    }

    fn country_reader(&self) -> Option<&Reader<Vec<u8>>> {
        self.country_reader
            .get_or_init(|| load_reader(&self.country_database_path, "country"))
            .as_deref()
    }
}

fn load_reader(path: &Path, database_kind: &'static str) -> Option<Arc<Reader<Vec<u8>>>> {
    match Reader::open_readfile(path) {
        Ok(reader) => Some(Arc::new(reader)),
        Err(error) => {
            warn!(
                database_kind,
                path = %path.display(),
                %error,
                "GeoIP database is unavailable; matching visitor location fields will be empty"
            );
            None
        }
    }
}

fn parse_user_agent(user_agent: &str) -> Option<UserAgentDetails> {
    let parsed = Parser::new().parse(user_agent)?;
    let details = UserAgentDetails {
        browser: known_user_agent_value(parsed.name),
        browser_version: known_user_agent_value(parsed.version),
        operating_system: known_user_agent_value(parsed.os),
        operating_system_version: known_user_agent_value(parsed.os_version.as_ref()),
        device_category: known_user_agent_value(parsed.category),
    };
    (!details.is_empty()).then_some(details)
}

fn known_user_agent_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != UNKNOWN_USER_AGENT_VALUE).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::config::GeoIpConfig;

    use super::VisitorEnvironment;

    #[test]
    fn parses_browser_operating_system_and_device_category() {
        let environment = environment();
        let parsed = environment
            .user_agent(Some(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/122.0.0.0 Safari/537.36",
            ))
            .unwrap();

        assert_eq!(parsed.browser.as_deref(), Some("Chrome"));
        assert_eq!(parsed.browser_version.as_deref(), Some("122.0.0.0"));
        assert_eq!(parsed.operating_system.as_deref(), Some("Mac OSX"));
        assert_eq!(parsed.device_category.as_deref(), Some("pc"));
    }

    #[test]
    fn reads_transferred_geoip_databases() {
        let environment = environment();
        let details = environment.geo_ip(Some("8.8.8.8")).unwrap();

        assert_eq!(details.country_code.as_deref(), Some("US"));
        assert!(details.country.is_some());
    }

    #[test]
    fn ignores_missing_or_invalid_observations() {
        let environment = environment();
        assert!(environment.geo_ip(None).is_none());
        assert!(environment.geo_ip(Some("not-an-ip")).is_none());
        assert!(environment.user_agent(None).is_none());
        assert!(environment.user_agent(Some("")).is_none());
    }

    #[test]
    fn missing_databases_leave_geoip_fields_empty() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/missing.mmdb");
        let environment = VisitorEnvironment::new(&GeoIpConfig {
            city_database_path: missing.clone(),
            country_database_path: missing,
        });

        assert!(environment.geo_ip(Some("8.8.8.8")).is_none());
    }

    fn environment() -> VisitorEnvironment {
        let data_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        VisitorEnvironment::new(&GeoIpConfig {
            city_database_path: data_directory.join("GeoLite2-City.mmdb"),
            country_database_path: data_directory.join("GeoLite2-Country.mmdb"),
        })
    }
}
