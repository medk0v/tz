//! TOML-backed process configuration.

use std::{
    fs,
    net::{IpAddr, SocketAddr},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use ipnet::IpNet;
use serde::Deserialize;
use uuid::Uuid;

/// Runtime configuration shared by the API and worker processes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub product: ProductConfig,
    pub server: ServerConfig,
    pub worker: WorkerConfig,
    pub pg: PostgresConfig,
    pub redis: RedisConfig,
    #[serde(default)]
    pub geoip: GeoIpConfig,
    pub outbox: OutboxConfig,
    pub widget: WidgetConfig,
    pub realtime: RealtimeConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub attachments: AttachmentConfig,
    #[serde(default)]
    pub secrets: SecretsConfig,
    #[serde(default)]
    pub openclaw: OpenClawConfig,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub telephony: TelephonyConfig,
    #[serde(default)]
    pub mobile_push: MobilePushConfig,
    pub logging: LoggingConfig,
}

impl Config {
    /// Loads and validates configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, TOML is malformed, or a
    /// value violates a runtime invariant.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let mut config: Self = toml::from_str(&source)
            .with_context(|| format!("failed to parse config file {}", path.display()))?;
        let config_directory = path.parent().unwrap_or_else(|| Path::new("."));
        config.geoip.resolve_paths(config_directory);
        config.attachments.resolve_paths(config_directory);
        config.openclaw.resolve_paths(config_directory);
        if let Some(path) = &mut config.mobile_push.service_account_path
            && path.is_relative()
        {
            *path = config_directory.join(&*path);
        }
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        match self.product.project_id {
            None => bail!("product.project-id is required"),
            Some(id) if id.is_nil() => bail!("product.project-id must not be nil"),
            Some(_) => {}
        }
        if let Some(edition) = self.product.edition.as_deref()
            && edition != "lite"
        {
            bail!("product.edition must be \"lite\"; this build serves one workspace");
        }
        if !(1..=MAX_AI_TASK_CONCURRENCY).contains(&self.worker.ai_task_concurrency) {
            bail!("worker.ai-task-concurrency must be between 1 and {MAX_AI_TASK_CONCURRENCY}");
        }
        if !(1..=16).contains(&self.worker.ai_run_concurrency) {
            bail!("worker.ai-run-concurrency must be between 1 and 16");
        }
        if self.pg.max_connections == 0 {
            bail!("pg.max-connections must be greater than zero");
        }
        if self.outbox.poll_interval_ms == 0 {
            bail!("outbox.poll-interval-ms must be greater than zero");
        }
        if self.outbox.max_attempts <= 0 {
            bail!("outbox.max-attempts must be greater than zero");
        }
        if self.widget.session_ttl_seconds == 0 {
            bail!("widget.session-ttl-seconds must be greater than zero");
        }
        if self.widget.visitor_data_retention_days == 0 {
            bail!("widget.visitor-data-retention-days must be greater than zero");
        }
        if self
            .server
            .trusted_proxies
            .iter()
            .any(|network| network.prefix_len() == 0)
        {
            bail!("server.trusted-proxies must not contain an all-addresses CIDR");
        }
        if self.geoip.city_database_path.as_os_str().is_empty()
            || self.geoip.country_database_path.as_os_str().is_empty()
        {
            bail!("geoip database paths must not be empty");
        }
        if self.realtime.ticket_ttl_seconds == 0 {
            bail!("realtime.ticket-ttl-seconds must be greater than zero");
        }
        if self.auth.session_idle_ttl_seconds == 0 {
            bail!("auth.session-idle-ttl-seconds must be greater than zero");
        }
        if self.auth.session_absolute_ttl_seconds < self.auth.session_idle_ttl_seconds {
            bail!(
                "auth.session-absolute-ttl-seconds must be greater than or equal to the idle TTL"
            );
        }
        if self.auth.remember_session_idle_ttl_seconds == 0 {
            bail!("auth.remember-session-idle-ttl-seconds must be greater than zero");
        }
        if self.auth.remember_session_absolute_ttl_seconds
            < self.auth.remember_session_idle_ttl_seconds
        {
            bail!(
                "auth.remember-session-absolute-ttl-seconds must be greater than or equal to the remembered idle TTL"
            );
        }
        if self.auth.login_window_seconds == 0
            || self.auth.login_max_attempts == 0
            || self.auth.login_lockout_seconds == 0
        {
            bail!("auth login throttling values must be greater than zero");
        }
        if self.attachments.max_concurrent_uploads == 0 {
            bail!("attachments.max-concurrent-uploads must be greater than zero");
        }
        if self.attachments.scan_timeout_seconds == 0 {
            bail!("attachments.scan-timeout-seconds must be greater than zero");
        }
        if self.attachments.retention_days == 0 {
            bail!("attachments.retention-days must be greater than zero");
        }
        if self.attachments.max_conversation_bytes == 0
            || self.attachments.max_tenant_bytes < self.attachments.max_conversation_bytes
        {
            bail!(
                "attachments.max-tenant-bytes must be greater than or equal to the non-zero conversation limit"
            );
        }
        if self.attachments.enabled
            && self.attachments.scan_mode == AttachmentScanMode::Required
            && self.attachments.clamav_address.is_none()
        {
            bail!("attachments.clamav-address is required when attachments are enabled");
        }
        if self.attachments.enabled
            && self.attachments.scan_mode == AttachmentScanMode::Disabled
            && self.attachments.clamav_address.is_some()
        {
            bail!("attachments.clamav-address must be omitted when scan-mode is disabled");
        }
        if self.attachments.enabled
            && self
                .attachments
                .clamav_address
                .is_some_and(|address| !is_private_scanner_address(address))
        {
            bail!("attachments.clamav-address must use a loopback or private network address");
        }
        if self.attachments.storage_path.as_os_str().is_empty()
            || self.attachments.storage_path.file_name().is_none()
        {
            bail!("attachments.storage-path must name a dedicated directory");
        }
        if self.secrets.encryption_key_env.trim().is_empty()
            || self.secrets.key_version.trim().is_empty()
        {
            bail!("secrets encryption-key-env and key-version must not be empty");
        }
        self.openclaw.validate()?;
        self.telegram.validate()?;
        self.telephony.validate()?;
        if self.mobile_push.enabled
            && self
                .mobile_push
                .service_account_path
                .as_ref()
                .is_none_or(|path| path.as_os_str().is_empty())
        {
            bail!("mobile_push.service-account-path is required when enabled");
        }
        if self.logging.filter.trim().is_empty() {
            bail!("logging.filter must not be empty");
        }
        Ok(())
    }
}

/// Namespace for host paths, accounts, services, pub/sub, cookies and storage.
pub const PRODUCT_NAMESPACE: &str = "tz";
/// Request header carrying the active project id.
pub const PROJECT_HEADER: &str = "x-tz-project-id";
/// Request header a trusted reverse proxy uses to pass the real client IP.
pub const CLIENT_IP_HEADER: &str = "x-tz-client-ip";
/// Redis pub/sub channel for realtime events.
pub const REALTIME_CHANNEL: &str = "tz.events";

/// The deployment uses one pre-provisioned internal project across every
/// credential type.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ProductConfig {
    /// Installations provisioned before the edition split was removed still carry
    /// `edition = "lite"` in their configuration file, and provisioning preserves
    /// that file across deployments. This build is always single-project, so the
    /// key is accepted for those installations and rejected for any other value.
    pub edition: Option<String>,
    pub project_id: Option<Uuid>,
}

impl ProductConfig {
    /// The internal project configured for this installation.
    pub fn fixed_project_id(&self) -> Option<Uuid> {
        self.project_id
    }

    /// Applies deployment scope before any session, token, or realtime lookup.
    pub(crate) fn resolve_project(
        &self,
        requested: Option<Uuid>,
    ) -> Result<Option<Uuid>, crate::error::AppError> {
        let fixed = self.project_id.ok_or(crate::error::AppError::Forbidden)?;
        if requested.is_some_and(|project| project != fixed) {
            return Err(crate::error::AppError::Forbidden);
        }
        Ok(Some(fixed))
    }
}

/// Optional Firebase Cloud Messaging delivery for native operator applications.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct MobilePushConfig {
    pub enabled: bool,
    pub service_account_path: Option<PathBuf>,
}

/// Public HTTPS endpoint used when registering Telegram Bot API webhooks.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TelegramConfig {
    pub webhook_base_url: Option<String>,
}

impl TelegramConfig {
    fn validate(&self) -> Result<()> {
        let Some(value) = self.webhook_base_url.as_deref() else {
            return Ok(());
        };
        let url = url::Url::parse(value.trim())
            .context("telegram.webhook-base-url is not a valid URL")?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!(
                "telegram.webhook-base-url must be a public HTTPS origin without credentials, path, query, or fragment"
            );
        }
        Ok(())
    }
}

/// Phone gateways reach the worker over a private network. Asterisk sends call
/// audio to the `AudioSocket` listener and calls the API to register calls.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TelephonyConfig {
    /// Private TCP address where the worker accepts Asterisk `AudioSocket` calls.
    pub audiosocket_address: Option<SocketAddr>,
    /// API origin that Asterisk uses for call registration, shown in dialplan examples.
    pub gateway_api_base_url: Option<String>,
}

impl TelephonyConfig {
    fn validate(&self) -> Result<()> {
        if self
            .audiosocket_address
            .is_some_and(|address| !is_private_scanner_address(address))
        {
            bail!("telephony.audiosocket-address must be a loopback or private address");
        }
        if let Some(value) = self.gateway_api_base_url.as_deref() {
            let url = url::Url::parse(value.trim())
                .context("telephony.gateway-api-base-url is not a valid URL")?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!("telephony.gateway-api-base-url must be an HTTP(S) origin without a path");
            }
        }
        Ok(())
    }
}

fn is_private_scanner_address(address: SocketAddr) -> bool {
    match address.ip() {
        IpAddr::V4(address) => address.is_loopback() || address.is_private(),
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unique_local()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|address| address.is_loopback() || address.is_private())
        }
    }
}

/// Public API listener and browser access settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ServerConfig {
    pub host: std::net::IpAddr,
    pub port: u16,
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub trusted_proxies: Vec<IpNet>,
}

impl ServerConfig {
    pub fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

/// Worker health listener settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct WorkerConfig {
    pub host: std::net::IpAddr,
    pub port: u16,
    #[serde(default = "default_ai_task_concurrency")]
    pub ai_task_concurrency: usize,
    /// Deployment-wide capacity shared by all durable agent execution queues.
    #[serde(default = "default_ai_run_concurrency")]
    pub ai_run_concurrency: usize,
}

pub const MAX_AI_TASK_CONCURRENCY: usize = 4;

const fn default_ai_task_concurrency() -> usize {
    MAX_AI_TASK_CONCURRENCY
}

const fn default_ai_run_concurrency() -> usize {
    2
}

impl WorkerConfig {
    pub fn address(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }

    pub fn effective_ai_task_concurrency(&self) -> usize {
        self.ai_task_concurrency.min(self.ai_run_concurrency)
    }
}

/// `PostgreSQL` connection and migration settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PostgresConfig {
    pub url: String,
    pub max_connections: u32,
    pub migrate_on_start: bool,
}

/// Optional Redis connection and readiness policy.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisConfig {
    pub url: Option<String>,
    pub required: bool,
}

/// Local `MaxMind` database paths used for coarse visitor geolocation.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct GeoIpConfig {
    pub city_database_path: PathBuf,
    pub country_database_path: PathBuf,
}

impl GeoIpConfig {
    fn resolve_paths(&mut self, config_directory: &Path) {
        if self.city_database_path.is_relative() {
            self.city_database_path = config_directory.join(&self.city_database_path);
        }
        if self.country_database_path.is_relative() {
            self.country_database_path = config_directory.join(&self.country_database_path);
        }
    }
}

impl Default for GeoIpConfig {
    fn default() -> Self {
        Self {
            city_database_path: PathBuf::from("data/GeoLite2-City.mmdb"),
            country_database_path: PathBuf::from("data/GeoLite2-Country.mmdb"),
        }
    }
}

/// Transactional outbox worker settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct OutboxConfig {
    pub poll_interval_ms: u64,
    pub max_attempts: i32,
}

impl OutboxConfig {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }
}

/// Anonymous widget session settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct WidgetConfig {
    pub session_ttl_seconds: u64,
    #[serde(default = "default_visitor_data_retention_days")]
    pub visitor_data_retention_days: u32,
}

impl WidgetConfig {
    pub fn session_ttl(&self) -> Duration {
        Duration::from_secs(self.session_ttl_seconds)
    }
}

const fn default_visitor_data_retention_days() -> u32 {
    30
}

/// Short-lived realtime authorization settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RealtimeConfig {
    pub ticket_ttl_seconds: u64,
}

/// Password login, server-side session, and cookie settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct AuthConfig {
    pub cookie_secure: bool,
    pub session_idle_ttl_seconds: u64,
    pub session_absolute_ttl_seconds: u64,
    #[serde(default = "default_remember_session_idle_ttl_seconds")]
    pub remember_session_idle_ttl_seconds: u64,
    #[serde(default = "default_remember_session_absolute_ttl_seconds")]
    pub remember_session_absolute_ttl_seconds: u64,
    pub login_window_seconds: u64,
    pub login_max_attempts: u32,
    pub login_lockout_seconds: u64,
}

fn default_remember_session_idle_ttl_seconds() -> u64 {
    30 * 24 * 60 * 60
}

fn default_remember_session_absolute_ttl_seconds() -> u64 {
    90 * 24 * 60 * 60
}

/// Quarantined chat-attachment storage and malware-scanner settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AttachmentConfig {
    pub enabled: bool,
    pub storage_path: PathBuf,
    pub scan_mode: AttachmentScanMode,
    pub clamav_address: Option<SocketAddr>,
    pub scan_timeout_seconds: u64,
    pub max_concurrent_uploads: usize,
    pub max_conversation_bytes: u64,
    pub max_tenant_bytes: u64,
    pub retention_days: u32,
}

impl AttachmentConfig {
    fn resolve_paths(&mut self, config_directory: &Path) {
        if self.storage_path.is_relative() {
            self.storage_path = config_directory.join(&self.storage_path);
        }
    }

    pub fn scan_timeout(&self) -> Duration {
        Duration::from_secs(self.scan_timeout_seconds)
    }
}

impl Default for AttachmentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            storage_path: PathBuf::from("data/attachments"),
            scan_mode: AttachmentScanMode::Required,
            clamav_address: None,
            scan_timeout_seconds: 60,
            max_concurrent_uploads: 4,
            max_conversation_bytes: 256 * 1_024 * 1_024,
            max_tenant_bytes: 5 * 1_024 * 1_024 * 1_024,
            retention_days: 30,
        }
    }
}

/// Malware-scanning policy for uploaded attachments.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AttachmentScanMode {
    /// Reject uploads unless the complete file passes the configured scanner.
    #[default]
    Required,
    /// Skip malware scanning. Intended only for an isolated local environment.
    Disabled,
}

/// References the process environment key used for encrypted integration secrets.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SecretsConfig {
    pub encryption_key_env: String,
    pub key_version: String,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            encryption_key_env: "TZOMET_SECRETS_KEY".to_owned(),
            key_version: "v1".to_owned(),
        }
    }
}

/// Optional handoff from Tzomet profiles to an `OpenClaw` Gateway agent run.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct OpenClawConfig {
    pub base_url: Option<String>,
    pub grant_directory: PathBuf,
    pub action_directory: PathBuf,
    pub grant_ttl_seconds: u64,
}

impl OpenClawConfig {
    fn resolve_paths(&mut self, config_directory: &Path) {
        if self.grant_directory.is_relative() {
            self.grant_directory = config_directory.join(&self.grant_directory);
        }
        if self.action_directory.is_relative() {
            self.action_directory = config_directory.join(&self.action_directory);
        }
    }

    fn validate(&self) -> Result<()> {
        if let Some(base_url) = self.base_url.as_deref() {
            let base_url = base_url.trim();
            let parsed =
                url::Url::parse(base_url).context("openclaw.base-url is not a valid URL")?;
            if !matches!(parsed.scheme(), "http" | "https")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                bail!(
                    "openclaw.base-url must be an HTTP(S) URL without credentials, query, or fragment"
                );
            }
        }
        if self.grant_directory.as_os_str().is_empty() || self.grant_directory.file_name().is_none()
        {
            bail!("openclaw.grant-directory must name a dedicated directory");
        }
        if self.action_directory.as_os_str().is_empty()
            || self.action_directory.file_name().is_none()
        {
            bail!("openclaw.action-directory must name a dedicated directory");
        }
        let grant_directory = normalize_path(&self.grant_directory);
        let action_directory = normalize_path(&self.action_directory);
        if grant_directory == action_directory
            || grant_directory.starts_with(&action_directory)
            || action_directory.starts_with(&grant_directory)
        {
            bail!("openclaw grant and action directories must be separate, non-nested paths");
        }
        if !(360..=600).contains(&self.grant_ttl_seconds) {
            bail!("openclaw.grant-ttl-seconds must be between 360 and 600");
        }
        Ok(())
    }

    pub fn enabled(&self) -> bool {
        self.base_url.is_some()
    }

    pub fn canonical_base_url(&self) -> Option<String> {
        self.base_url
            .as_deref()
            .and_then(|base_url| canonical_provider_url(base_url).ok())
    }

    pub fn handles_provider(&self, provider_base_url: &str) -> bool {
        self.canonical_base_url().is_some_and(|configured| {
            canonical_provider_url(provider_base_url).is_ok_and(|provider| configured == provider)
        })
    }

    pub fn grant_ttl(&self) -> Duration {
        Duration::from_secs(self.grant_ttl_seconds)
    }
}

fn normalize_url_path(url: &mut url::Url) {
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(if path.is_empty() { "/" } else { &path });
}

fn canonical_provider_url(value: &str) -> Result<String> {
    let mut url = url::Url::parse(value.trim()).context("provider URL is invalid")?;
    normalize_url_path(&mut url);
    Ok(url.to_string())
}

impl Default for OpenClawConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            grant_directory: PathBuf::from("../infra/openclaw/runtime/agent-secrets"),
            action_directory: PathBuf::from("../infra/openclaw/runtime/agent-actions"),
            grant_ttl_seconds: 360,
        }
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => normalized.push(component.as_os_str()),
            },
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

impl AuthConfig {
    pub(crate) fn idle_ttl_seconds(&self, remember_me: bool) -> u64 {
        if remember_me {
            self.remember_session_idle_ttl_seconds
        } else {
            self.session_idle_ttl_seconds
        }
    }

    pub(crate) fn absolute_ttl_seconds(&self, remember_me: bool) -> u64 {
        if remember_me {
            self.remember_session_absolute_ttl_seconds
        } else {
            self.session_absolute_ttl_seconds
        }
    }

    pub fn session_idle_ttl(&self) -> Duration {
        Duration::from_secs(self.session_idle_ttl_seconds)
    }

    pub fn session_absolute_ttl(&self) -> Duration {
        Duration::from_secs(self.session_absolute_ttl_seconds)
    }
}

impl RealtimeConfig {
    pub fn ticket_ttl(&self) -> Duration {
        Duration::from_secs(self.ticket_ttl_seconds)
    }
}

/// Process-wide tracing settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    pub filter: String,
    pub format: LogFormat,
}

/// Output format for structured logs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Json,
    Pretty,
}

#[cfg(test)]
mod tests {
    use super::{AttachmentScanMode, Config};
    use std::path::Path;

    const EXAMPLE: &str = include_str!("../Config.example.toml");

    #[test]
    fn a_provisioned_lite_edition_key_still_loads() {
        // Installations provisioned before the edition split was removed keep
        // `edition = "lite"` in a configuration file that provisioning preserves,
        // so rejecting the key would stop the API from starting on them.
        let source = EXAMPLE.replace("[product]", "[product]\nedition = \"lite\"");
        let config: Config = toml::from_str(&source).unwrap();
        assert_eq!(config.product.edition.as_deref(), Some("lite"));
        assert!(config.validate().is_ok());

        let other = EXAMPLE.replace("[product]", "[product]\nedition = \"standard\"");
        let config: Config = toml::from_str(&other).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn a_fixed_non_nil_project_is_required() {
        let mut config: Config = toml::from_str(EXAMPLE).unwrap();
        let fixed = uuid::Uuid::now_v7();
        config.product.project_id = None;
        assert!(config.validate().is_err());
        assert!(config.product.resolve_project(None).is_err());
        config.product.project_id = Some(uuid::Uuid::nil());
        assert!(config.validate().is_err());
        config.product.project_id = Some(fixed);
        assert!(config.validate().is_ok());
        assert_eq!(config.product.resolve_project(None).unwrap(), Some(fixed));
        assert_eq!(
            config.product.resolve_project(Some(fixed)).unwrap(),
            Some(fixed)
        );
        assert!(
            config
                .product
                .resolve_project(Some(uuid::Uuid::now_v7()))
                .is_err()
        );
    }

    #[test]
    fn example_config_is_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Config.example.toml");
        let config = Config::from_file(path).unwrap();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.worker.ai_task_concurrency, 4);
        assert_eq!(config.worker.effective_ai_task_concurrency(), 2);
        assert_eq!(config.worker.ai_run_concurrency, 2);
        assert_eq!(config.outbox.poll_interval().as_millis(), 500);
        assert!(config.geoip.city_database_path.is_absolute());
        assert!(config.geoip.country_database_path.is_absolute());
        assert!(config.attachments.storage_path.is_absolute());
        assert_eq!(config.attachments.scan_mode, AttachmentScanMode::Required);
        assert!(config.attachments.clamav_address.is_some());
        assert!(
            config
                .openclaw
                .handles_provider("http://127.0.0.1:18789/v1/")
        );
        assert!(
            config
                .openclaw
                .handles_provider("HTTP://127.0.0.1:18789/v1")
        );
        assert!(
            !config
                .openclaw
                .handles_provider("http://localhost:18789/v1")
        );
        assert!(config.openclaw.grant_directory.is_absolute());
        assert!(config.openclaw.action_directory.is_absolute());
        assert!(config.openclaw.enabled());
        assert_eq!(
            config.openclaw.canonical_base_url().as_deref(),
            Some("http://127.0.0.1:18789/v1")
        );

        let disabled: Config =
            toml::from_str(&EXAMPLE.replace("base-url = \"http://127.0.0.1:18789/v1\"\n", ""))
                .unwrap();
        assert!(disabled.validate().is_ok());
        assert!(!disabled.openclaw.enabled());
        assert_eq!(disabled.worker.effective_ai_task_concurrency(), 2);
    }

    #[test]
    fn rejects_unknown_fields() {
        let invalid = EXAMPLE.replace("port = 8080", "port = 8080\nunsupported-setting = true");
        assert!(toml::from_str::<Config>(&invalid).is_err());
    }

    #[test]
    fn remembered_session_defaults_preserve_standard_session_policy() {
        let legacy = EXAMPLE
            .replace("remember-session-idle-ttl-seconds = 2592000\n", "")
            .replace("remember-session-absolute-ttl-seconds = 7776000\n", "");
        let config: Config = toml::from_str(&legacy).unwrap();
        assert!(config.validate().is_ok());
        assert_eq!(config.auth.idle_ttl_seconds(false), 3600);
        assert_eq!(config.auth.absolute_ttl_seconds(false), 43200);
        assert_eq!(config.auth.idle_ttl_seconds(true), 30 * 24 * 60 * 60);
        assert_eq!(config.auth.absolute_ttl_seconds(true), 90 * 24 * 60 * 60);
    }

    #[test]
    fn remembered_session_lifetimes_are_configurable_and_validated() {
        let mut config: Config = toml::from_str(EXAMPLE).unwrap();
        config.auth.remember_session_idle_ttl_seconds = 120;
        config.auth.remember_session_absolute_ttl_seconds = 180;
        assert!(config.validate().is_ok());
        assert_eq!(config.auth.idle_ttl_seconds(true), 120);
        assert_eq!(config.auth.absolute_ttl_seconds(true), 180);
        assert_eq!(config.auth.idle_ttl_seconds(false), 3600);
        config.auth.remember_session_absolute_ttl_seconds = 119;
        assert!(config.validate().is_err());
        config.auth.remember_session_idle_ttl_seconds = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_invalid_runtime_values() {
        let invalid = EXAMPLE.replace("max-connections = 10", "max-connections = 0");
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());

        for concurrency in [0, 5] {
            let invalid = EXAMPLE.replace(
                "ai-task-concurrency = 4",
                &format!("ai-task-concurrency = {concurrency}"),
            );
            let config: Config = toml::from_str(&invalid).unwrap();
            assert!(config.validate().is_err());
        }

        for concurrency in [0, 17] {
            let invalid = EXAMPLE.replace(
                "ai-run-concurrency = 2",
                &format!("ai-run-concurrency = {concurrency}"),
            );
            let config: Config = toml::from_str(&invalid).unwrap();
            assert!(config.validate().is_err());
        }

        for ttl in [359, 601] {
            let invalid = EXAMPLE.replace(
                "grant-ttl-seconds = 360",
                &format!("grant-ttl-seconds = {ttl}"),
            );
            let config: Config = toml::from_str(&invalid).unwrap();
            assert!(config.validate().is_err());
        }

        let disabled_invalid = EXAMPLE
            .replace("base-url = \"http://127.0.0.1:18789/v1\"\n", "")
            .replace("grant-ttl-seconds = 360", "grant-ttl-seconds = 0");
        let config: Config = toml::from_str(&disabled_invalid).unwrap();
        assert!(config.validate().is_err());

        let invalid = EXAMPLE.replace("clamav-address = \"127.0.0.1:3310\"", "");
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());

        let local = EXAMPLE
            .replace("scan-mode = \"required\"", "scan-mode = \"disabled\"")
            .replace("clamav-address = \"127.0.0.1:3310\"\n", "");
        let config: Config = toml::from_str(&local).unwrap();
        assert!(config.validate().is_ok());

        let invalid = EXAMPLE.replace("127.0.0.1:3310", "8.8.8.8:3310");
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());

        let invalid = EXAMPLE.replace(
            "storage-path = \"data/attachments\"",
            "storage-path = \"/\"",
        );
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());

        let invalid = EXAMPLE.replace(
            "action-directory = \"../infra/openclaw/runtime/agent-actions\"",
            "action-directory = \"../infra/openclaw/runtime/agent-secrets\"",
        );
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());

        let invalid = EXAMPLE.replace(
            "action-directory = \"../infra/openclaw/runtime/agent-actions\"",
            "action-directory = \"../infra/openclaw/runtime/agent-secrets/actions\"",
        );
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());

        let disabled_invalid = invalid.replace("base-url = \"http://127.0.0.1:18789/v1\"\n", "");
        let config: Config = toml::from_str(&disabled_invalid).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_all_addresses_trusted_proxy_networks() {
        let invalid = EXAMPLE.replace("trusted-proxies = []", "trusted-proxies = [\"0.0.0.0/0\"]");
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());

        let invalid = EXAMPLE.replace("trusted-proxies = []", "trusted-proxies = [\"::/0\"]");
        let config: Config = toml::from_str(&invalid).unwrap();
        assert!(config.validate().is_err());
    }
}
