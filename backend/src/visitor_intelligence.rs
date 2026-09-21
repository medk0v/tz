//! Validated browser context and retention for widget visitor intelligence.

use anyhow::{Context, Result};
use axum::http::{HeaderMap, HeaderName, header};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::PgPool;
use url::Url;

use crate::error::AppError;

const MAX_URL_LENGTH: usize = 4_096;
const MAX_TITLE_LENGTH: usize = 512;
const MAX_SHORT_TEXT_LENGTH: usize = 128;
const MAX_ATTRIBUTION_LENGTH: usize = 512;
const MAX_LANGUAGES: usize = 20;
const MAX_BRANDS: usize = 16;
const MAX_HEADER_LENGTH: usize = 2_048;

/// A widget session is shown as online while its latest heartbeat falls inside
/// this window. The browser sends heartbeats more frequently than this value.
pub const ONLINE_PRESENCE_WINDOW_SECONDS: i64 = 90;

const SEC_CH_UA: HeaderName = HeaderName::from_static("sec-ch-ua");
const SEC_CH_UA_MOBILE: HeaderName = HeaderName::from_static("sec-ch-ua-mobile");
const SEC_CH_UA_PLATFORM: HeaderName = HeaderName::from_static("sec-ch-ua-platform");
const DNT: HeaderName = HeaderName::from_static("dnt");
const SEC_GPC: HeaderName = HeaderName::from_static("sec-gpc");

/// Browser-provided context for one widget session.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientContext {
    pub schema_version: u8,
    pub captured_at: DateTime<Utc>,
    pub page: PageContext,
    pub locale: LocaleContext,
    pub display: DisplayContext,
    pub browser: BrowserContext,
    pub device: Option<DeviceContext>,
    pub preferences: Option<PreferenceContext>,
    pub connection: Option<ConnectionContext>,
    pub attribution: Option<AttributionContext>,
}

impl ClientContext {
    /// Validates size and shape constraints and removes referrer path details.
    ///
    /// # Errors
    ///
    /// Returns `BadRequest` for unsupported schema versions, malformed or unsafe
    /// URLs, a page origin mismatch, or a field outside its supported bounds.
    pub fn validate_and_normalize(mut self, expected_origin: &str) -> Result<Self, AppError> {
        if self.schema_version != 1 {
            return Err(bad_request("client_context.schema_version must be 1"));
        }
        validate_page(&mut self.page, expected_origin)?;
        validate_locale(&self.locale)?;
        validate_display(&self.display)?;
        validate_browser(&self.browser)?;
        if let Some(device) = &self.device {
            validate_device(device)?;
        }
        if let Some(preferences) = &self.preferences {
            validate_preferences(preferences)?;
        }
        if let Some(connection) = &self.connection {
            validate_connection(connection)?;
        }
        if let Some(attribution) = &self.attribution {
            validate_attribution(attribution)?;
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageContext {
    pub url: String,
    pub title: String,
    pub referrer: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleContext {
    pub language: Option<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    pub timezone: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayContext {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub device_pixel_ratio: Option<f64>,
    pub color_depth: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserContext {
    pub platform: Option<String>,
    #[serde(default)]
    pub brands: Vec<BrowserBrand>,
    pub mobile: Option<bool>,
    pub cookie_enabled: Option<bool>,
    pub do_not_track: Option<String>,
    pub global_privacy_control: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserBrand {
    pub brand: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceContext {
    pub memory_gb: Option<f64>,
    pub logical_processors: Option<u32>,
    pub max_touch_points: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceContext {
    pub color_scheme: Option<String>,
    pub reduced_motion: Option<bool>,
    pub contrast: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionContext {
    pub effective_type: Option<String>,
    pub downlink_mbps: Option<f64>,
    pub rtt_ms: Option<u32>,
    pub save_data: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttributionContext {
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
    pub utm_term: Option<String>,
    pub utm_content: Option<String>,
    pub gclid: Option<String>,
    pub fbclid: Option<String>,
    pub msclkid: Option<String>,
}

/// HTTP metadata captured by the server rather than accepted from JSON.
#[derive(Debug)]
pub struct CapturedHeaders {
    pub user_agent: Option<String>,
    pub accept_language: Option<String>,
    pub client_hints: Option<Value>,
}

/// Captures bounded browser headers from the HTTP request.
///
/// # Errors
///
/// Returns `BadRequest` when a present header is not valid text or exceeds its
/// storage bound.
pub fn capture_headers(headers: &HeaderMap) -> Result<CapturedHeaders, AppError> {
    let user_agent = optional_header(headers, &header::USER_AGENT, MAX_HEADER_LENGTH)?;
    let accept_language = optional_header(headers, &header::ACCEPT_LANGUAGE, 1_024)?;
    let mut hints = Map::new();
    for (name, key) in [
        (&SEC_CH_UA, "sec_ch_ua"),
        (&SEC_CH_UA_MOBILE, "sec_ch_ua_mobile"),
        (&SEC_CH_UA_PLATFORM, "sec_ch_ua_platform"),
        (&DNT, "dnt"),
        (&SEC_GPC, "sec_gpc"),
    ] {
        if let Some(value) = optional_header(headers, name, 1_024)? {
            hints.insert(key.to_owned(), Value::String(value));
        }
    }

    Ok(CapturedHeaders {
        user_agent,
        accept_language,
        client_hints: (!hints.is_empty()).then_some(Value::Object(hints)),
    })
}

/// Clears expired raw visitor observations while retaining session linkage.
///
/// # Errors
///
/// Returns an error when `PostgreSQL` cannot complete the purge update.
pub async fn purge_expired(db: &PgPool) -> Result<u64> {
    let result = sqlx::query(
        r#"
        UPDATE widget_sessions
        SET client_ip = NULL,
            client_ip_source = NULL,
            user_agent = NULL,
            accept_language = NULL,
            client_hints = NULL,
            client_context = NULL,
            visitor_data_expires_at = NULL
        WHERE visitor_data_expires_at <= now()
        "#,
    )
    .execute(db)
    .await
    .context("failed to purge expired widget visitor data")?;
    Ok(result.rows_affected())
}

fn validate_page(page: &mut PageContext, expected_origin: &str) -> Result<(), AppError> {
    validate_string(&page.url, "client_context.page.url", MAX_URL_LENGTH, false)?;
    validate_string(
        &page.title,
        "client_context.page.title",
        MAX_TITLE_LENGTH,
        true,
    )?;
    let parsed = parse_http_url(&page.url, "client_context.page.url")?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(bad_request(
            "client_context.page.url must not contain a query or fragment",
        ));
    }
    let page_origin = parsed.origin().ascii_serialization();
    if page_origin != expected_origin {
        return Err(bad_request(
            "client_context.page.url origin does not match Origin",
        ));
    }
    page.url = parsed.to_string();

    if let Some(referrer) = &page.referrer {
        validate_string(
            referrer,
            "client_context.page.referrer",
            MAX_URL_LENGTH,
            false,
        )?;
        let parsed = parse_http_url(referrer, "client_context.page.referrer")?;
        page.referrer = Some(parsed.origin().ascii_serialization());
    }
    Ok(())
}

fn parse_http_url(value: &str, field: &str) -> Result<Url, AppError> {
    let parsed =
        Url::parse(value).map_err(|_| bad_request(format!("{field} is not a valid URL")))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.host().is_none()
    {
        return Err(bad_request(format!(
            "{field} must be an http(s) URL without credentials"
        )));
    }
    Ok(parsed)
}

fn validate_locale(locale: &LocaleContext) -> Result<(), AppError> {
    validate_optional_string(
        locale.language.as_deref(),
        "client_context.locale.language",
        MAX_SHORT_TEXT_LENGTH,
    )?;
    validate_optional_string(
        locale.timezone.as_deref(),
        "client_context.locale.timezone",
        MAX_SHORT_TEXT_LENGTH,
    )?;
    if locale.languages.len() > MAX_LANGUAGES {
        return Err(bad_request(format!(
            "client_context.locale.languages cannot contain more than {MAX_LANGUAGES} values"
        )));
    }
    for language in &locale.languages {
        validate_string(
            language,
            "client_context.locale.languages",
            MAX_SHORT_TEXT_LENGTH,
            false,
        )?;
    }
    Ok(())
}

fn validate_display(display: &DisplayContext) -> Result<(), AppError> {
    for (field, value) in [
        ("viewport_width", display.viewport_width),
        ("viewport_height", display.viewport_height),
        ("screen_width", display.screen_width),
        ("screen_height", display.screen_height),
    ] {
        if !(1..=100_000).contains(&value) {
            return Err(bad_request(format!(
                "client_context.display.{field} is outside the supported range"
            )));
        }
    }
    if display
        .device_pixel_ratio
        .is_some_and(|value| !value.is_finite() || !(0.1..=16.0).contains(&value))
    {
        return Err(bad_request(
            "client_context.display.device_pixel_ratio is outside the supported range",
        ));
    }
    if display
        .color_depth
        .is_some_and(|value| !(1..=128).contains(&value))
    {
        return Err(bad_request(
            "client_context.display.color_depth is outside the supported range",
        ));
    }
    Ok(())
}

fn validate_browser(browser: &BrowserContext) -> Result<(), AppError> {
    validate_optional_string(
        browser.platform.as_deref(),
        "client_context.browser.platform",
        MAX_SHORT_TEXT_LENGTH,
    )?;
    validate_optional_string(
        browser.do_not_track.as_deref(),
        "client_context.browser.do_not_track",
        32,
    )?;
    if browser.brands.len() > MAX_BRANDS {
        return Err(bad_request(format!(
            "client_context.browser.brands cannot contain more than {MAX_BRANDS} values"
        )));
    }
    for brand in &browser.brands {
        validate_string(
            &brand.brand,
            "client_context.browser.brands.brand",
            MAX_SHORT_TEXT_LENGTH,
            false,
        )?;
        validate_string(
            &brand.version,
            "client_context.browser.brands.version",
            MAX_SHORT_TEXT_LENGTH,
            false,
        )?;
    }
    Ok(())
}

fn validate_device(device: &DeviceContext) -> Result<(), AppError> {
    if device
        .memory_gb
        .is_some_and(|value| !value.is_finite() || !(0.0..=4_096.0).contains(&value))
    {
        return Err(bad_request(
            "client_context.device.memory_gb is outside the supported range",
        ));
    }
    if device
        .logical_processors
        .is_some_and(|value| !(1..=4_096).contains(&value))
    {
        return Err(bad_request(
            "client_context.device.logical_processors is outside the supported range",
        ));
    }
    if device.max_touch_points.is_some_and(|value| value > 1_024) {
        return Err(bad_request(
            "client_context.device.max_touch_points is outside the supported range",
        ));
    }
    Ok(())
}

fn validate_preferences(preferences: &PreferenceContext) -> Result<(), AppError> {
    validate_optional_string(
        preferences.color_scheme.as_deref(),
        "client_context.preferences.color_scheme",
        32,
    )?;
    validate_optional_string(
        preferences.contrast.as_deref(),
        "client_context.preferences.contrast",
        32,
    )?;
    if preferences
        .color_scheme
        .as_deref()
        .is_some_and(|value| !matches!(value, "light" | "dark" | "no-preference"))
    {
        return Err(bad_request(
            "client_context.preferences.color_scheme is unsupported",
        ));
    }
    if preferences
        .contrast
        .as_deref()
        .is_some_and(|value| !matches!(value, "more" | "less" | "custom" | "no-preference"))
    {
        return Err(bad_request(
            "client_context.preferences.contrast is unsupported",
        ));
    }
    Ok(())
}

fn validate_connection(connection: &ConnectionContext) -> Result<(), AppError> {
    validate_optional_string(
        connection.effective_type.as_deref(),
        "client_context.connection.effective_type",
        32,
    )?;
    if connection
        .effective_type
        .as_deref()
        .is_some_and(|value| !matches!(value, "slow-2g" | "2g" | "3g" | "4g"))
    {
        return Err(bad_request(
            "client_context.connection.effective_type is unsupported",
        ));
    }
    if connection
        .downlink_mbps
        .is_some_and(|value| !value.is_finite() || !(0.0..=100_000.0).contains(&value))
    {
        return Err(bad_request(
            "client_context.connection.downlink_mbps is outside the supported range",
        ));
    }
    if connection.rtt_ms.is_some_and(|value| value > 3_600_000) {
        return Err(bad_request(
            "client_context.connection.rtt_ms is outside the supported range",
        ));
    }
    Ok(())
}

fn validate_attribution(attribution: &AttributionContext) -> Result<(), AppError> {
    for (field, value) in [
        ("utm_source", attribution.utm_source.as_deref()),
        ("utm_medium", attribution.utm_medium.as_deref()),
        ("utm_campaign", attribution.utm_campaign.as_deref()),
        ("utm_term", attribution.utm_term.as_deref()),
        ("utm_content", attribution.utm_content.as_deref()),
        ("gclid", attribution.gclid.as_deref()),
        ("fbclid", attribution.fbclid.as_deref()),
        ("msclkid", attribution.msclkid.as_deref()),
    ] {
        validate_optional_string(
            value,
            &format!("client_context.attribution.{field}"),
            MAX_ATTRIBUTION_LENGTH,
        )?;
    }
    Ok(())
}

fn optional_header(
    headers: &HeaderMap,
    name: &HeaderName,
    max_chars: usize,
) -> Result<Option<String>, AppError> {
    headers
        .get(name)
        .map(|value| {
            let value = value
                .to_str()
                .map_err(|_| bad_request(format!("{name} header is invalid")))?;
            if value.chars().count() > max_chars {
                return Err(bad_request(format!("{name} header is too long")));
            }
            Ok(value.to_owned())
        })
        .transpose()
}

fn validate_optional_string(
    value: Option<&str>,
    field: &str,
    max_chars: usize,
) -> Result<(), AppError> {
    if let Some(value) = value {
        validate_string(value, field, max_chars, false)?;
    }
    Ok(())
}

fn validate_string(
    value: &str,
    field: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<(), AppError> {
    if (!allow_empty && value.is_empty()) || value.chars().count() > max_chars {
        return Err(bad_request(format!(
            "{field} is outside its supported length"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(bad_request(format!("{field} contains control characters")));
    }
    Ok(())
}

fn bad_request(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    use super::{ClientContext, capture_headers};

    fn context(page_url: &str, referrer: Option<&str>) -> ClientContext {
        serde_json::from_value(json!({
            "schema_version": 1,
            "captured_at": "2026-08-01T10:00:00Z",
            "page": {
                "url": page_url,
                "title": "Checkout",
                "referrer": referrer
            },
            "locale": {
                "language": "en-US",
                "languages": ["en-US", "en"],
                "timezone": "Europe/Chisinau"
            },
            "display": {
                "viewport_width": 1280,
                "viewport_height": 720,
                "screen_width": 2560,
                "screen_height": 1440,
                "device_pixel_ratio": 2.0,
                "color_depth": 24
            },
            "browser": {
                "platform": "macOS",
                "brands": [{"brand": "Chromium", "version": "140"}],
                "mobile": false,
                "cookie_enabled": true,
                "do_not_track": "1",
                "global_privacy_control": true
            },
            "device": null,
            "preferences": null,
            "connection": null,
            "attribution": null
        }))
        .unwrap()
    }

    #[test]
    fn validates_page_origin_and_reduces_referrer_to_origin() {
        let context = context(
            "https://shop.example/checkout",
            Some("https://search.example/results?q=private"),
        )
        .validate_and_normalize("https://shop.example")
        .unwrap();
        assert_eq!(
            context.page.referrer.as_deref(),
            Some("https://search.example")
        );
    }

    #[test]
    fn rejects_page_query_fragment_credentials_and_origin_mismatch() {
        assert!(
            context("https://shop.example/checkout?token=secret", None)
                .validate_and_normalize("https://shop.example")
                .is_err()
        );
        assert!(
            context("https://shop.example/checkout#payment", None)
                .validate_and_normalize("https://shop.example")
                .is_err()
        );
        assert!(
            context("https://user:secret@shop.example/checkout", None)
                .validate_and_normalize("https://shop.example")
                .is_err()
        );
        assert!(
            context("https://other.example/checkout", None)
                .validate_and_normalize("https://shop.example")
                .is_err()
        );
    }

    #[test]
    fn rejects_unknown_client_context_fields() {
        let value = json!({
            "schema_version": 1,
            "captured_at": "2026-08-01T10:00:00Z",
            "page": {"url": "https://shop.example/", "title": "Shop", "extra": true},
            "locale": {"language": null, "languages": [], "timezone": null},
            "display": {
                "viewport_width": 1,
                "viewport_height": 1,
                "screen_width": 1,
                "screen_height": 1,
                "device_pixel_ratio": null,
                "color_depth": null
            },
            "browser": {
                "platform": null,
                "brands": [],
                "mobile": null,
                "cookie_enabled": null,
                "do_not_track": null,
                "global_privacy_control": null
            },
            "device": null,
            "preferences": null,
            "connection": null,
            "attribution": null
        });
        assert!(serde_json::from_value::<ClientContext>(value).is_err());
    }

    #[test]
    fn captures_only_bounded_server_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("Browser/1"));
        headers.insert(
            "accept-language",
            HeaderValue::from_static("en-US,en;q=0.9"),
        );
        headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));

        let captured = capture_headers(&headers).unwrap();
        assert_eq!(captured.user_agent.as_deref(), Some("Browser/1"));
        assert_eq!(captured.accept_language.as_deref(), Some("en-US,en;q=0.9"));
        assert_eq!(
            captured
                .client_hints
                .as_ref()
                .and_then(|value| value.get("sec_ch_ua_mobile"))
                .and_then(serde_json::Value::as_str),
            Some("?0")
        );
    }
}
