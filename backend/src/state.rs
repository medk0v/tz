//! Shared process state and realtime publishing.

use std::{str::FromStr, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use redis::AsyncCommands;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::sync::broadcast;

use crate::{
    attachments::AttachmentStorage, config::Config, realtime::RealtimeEvent,
    visitor_environment::VisitorEnvironment,
};

/// Dependencies shared by HTTP handlers and background tasks.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub redis: Option<redis::Client>,
    pub provider_http: reqwest::Client,
    pub openclaw_http: reqwest::Client,
    pub realtime: broadcast::Sender<RealtimeEvent>,
    pub visitor_environment: Arc<VisitorEnvironment>,
    pub attachment_storage: Arc<AttachmentStorage>,
    pub(crate) mobile_push: Option<Arc<crate::mobile_push::FcmClient>>,
}

impl AppState {
    /// Builds lazy infrastructure clients and optionally runs migrations.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed connection settings or failed migrations.
    pub async fn build(config: Config) -> Result<Self> {
        let connect_options = PgConnectOptions::from_str(&config.pg.url)
            .context("invalid PostgreSQL URL in pg.url")?;
        let db = PgPoolOptions::new()
            .max_connections(config.pg.max_connections)
            .connect_lazy_with(connect_options);

        if config.pg.migrate_on_start {
            sqlx::migrate!("./migrations")
                .run(&db)
                .await
                .context("failed to run database migrations")?;
        }

        let redis = config
            .redis
            .url
            .as_deref()
            .map(redis::Client::open)
            .transpose()
            .context("invalid Redis URL in redis.url")?;
        let provider_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(5 * 60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build provider HTTP client")?;
        let openclaw_http = openclaw_http_client_builder()
            .build()
            .context("failed to build OpenClaw HTTP client")?;
        let (realtime, _) = broadcast::channel(1_024);
        let visitor_environment = Arc::new(VisitorEnvironment::new(&config.geoip));
        let attachment_storage = Arc::new(AttachmentStorage::prepare(&config.attachments).await?);
        let mobile_push = crate::mobile_push::FcmClient::from_config(&config.mobile_push)
            .await?
            .map(Arc::new);

        Ok(Self {
            config: Arc::new(config),
            db,
            redis,
            provider_http,
            openclaw_http,
            realtime,
            visitor_environment,
            attachment_storage,
            mobile_push,
        })
    }

    /// Publishes a committed realtime event through Redis or the local bus.
    ///
    /// # Errors
    ///
    /// Returns an error when Redis is configured but cannot publish the event.
    pub async fn publish(&self, event: &RealtimeEvent) -> Result<()> {
        if let Some(client) = &self.redis {
            let mut connection = client
                .get_multiplexed_async_connection()
                .await
                .context("failed to connect to Redis")?;
            let payload = serde_json::to_string(event)?;
            let _: i64 = connection
                .publish(crate::config::REALTIME_CHANNEL, payload)
                .await
                .context("failed to publish realtime event")?;
        } else {
            let _ = self.realtime.send(event.clone());
        }

        Ok(())
    }
}

fn openclaw_http_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        // The configured Gateway is selected by exact URL. Environment proxy
        // variables must not reroute its token-bearing local requests.
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(5 * 60))
        .redirect(reqwest::redirect::Policy::none())
}

#[cfg(test)]
mod tests {
    #[test]
    fn openclaw_client_ignores_environment_proxies() {
        let source = include_str!("state.rs");
        let builder_start = source
            .find("fn openclaw_http_client_builder()")
            .expect("OpenClaw client builder should exist");
        let builder_end = source[builder_start..]
            .find("#[cfg(test)]")
            .expect("OpenClaw client builder should precede its tests");
        let builder_source = &source[builder_start..builder_start + builder_end];
        assert!(builder_source.contains(".no_proxy()"));
        assert!(builder_source.contains("Policy::none()"));
    }
}
