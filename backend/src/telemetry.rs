//! Process-wide tracing initialization.

use std::borrow::Cow;

use anyhow::Result;
use sentry::integrations::tracing::EventFilter;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{LogFormat, LoggingConfig};

/// Keeps the Sentry transport alive and flushes pending events during shutdown.
#[must_use]
pub struct TelemetryGuard {
    _sentry: sentry::ClientInitGuard,
}

/// Initializes tracing according to the configured output format.
///
/// When `SENTRY_DSN` is present, error-level tracing events and panics are also
/// sent to Sentry. Informational logs remain local and default PII collection is
/// disabled.
///
/// # Errors
///
/// Returns an error when the global tracing subscriber was already installed.
pub fn init(config: &LoggingConfig, service: &'static str) -> Result<TelemetryGuard> {
    let sentry = sentry::init(sentry::ClientOptions {
        attach_stacktrace: true,
        environment: runtime_or_build_value(
            "SENTRY_ENVIRONMENT",
            option_env!("TZ_BUILD_ENVIRONMENT"),
        ),
        release: runtime_or_build_value("SENTRY_RELEASE", option_env!("TZ_BUILD_RELEASE")),
        send_default_pii: false,
        ..Default::default()
    });
    sentry::configure_scope(|scope| scope.set_tag("service", service));
    let filter = EnvFilter::try_new(&config.filter)?;
    let sentry_layer = sentry::integrations::tracing::layer().event_filter(|metadata| {
        if *metadata.level() == tracing::Level::ERROR {
            EventFilter::Event
        } else {
            EventFilter::Ignore
        }
    });
    let subscriber = tracing_subscriber::registry().with(sentry_layer);

    match config.format {
        LogFormat::Json => subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_target(false)
                    .with_filter(filter),
            )
            .try_init()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
        LogFormat::Pretty => subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .pretty()
                    .with_target(false)
                    .with_filter(filter),
            )
            .try_init()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?,
    }

    Ok(TelemetryGuard { _sentry: sentry })
}

fn runtime_or_build_value(
    variable: &str,
    build_value: Option<&'static str>,
) -> Option<Cow<'static, str>> {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(Cow::Owned)
        .or_else(|| build_value.map(Cow::Borrowed))
}
