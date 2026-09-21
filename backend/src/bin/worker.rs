use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use tokio::{net::TcpListener, task::JoinSet};
use tracing::{error, info};
use tz_backend::{
    AppState, Config, ai_api, ai_orchestration, ai_tasks, attachments,
    cli::{self, Command},
    conversation_maintenance, conversations, health, inbox_routing, outbox, provider_reply,
    telegram_bot, telemetry, visitor_intelligence,
};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let Command::Run { config_path } = cli::parse()? else {
        cli::print_usage("worker");
        return Ok(());
    };
    let config = Config::from_file(config_path)?;
    let _telemetry = telemetry::init(&config.logging, "worker")?;

    let address = config.worker.address();
    let poll_interval = config.outbox.poll_interval();
    let state = AppState::build(config).await?;
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind worker health server to {address}"))?;
    info!(%address, "worker health server listening");

    let worker_state = state.clone();
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
        let worker_id = Uuid::now_v7();
        loop {
            match outbox::process_once(&worker_state, worker_id).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(poll_interval).await,
                Err(error) => {
                    error!(?error, "outbox worker iteration failed");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    });
    // FCM calls have their own bounded lanes: slow phones/provider responses
    // cannot occupy the core realtime/email/Telegram delivery worker.
    for slot in 0..4 {
        let mobile_push_state = state.clone();
        tasks.spawn(async move {
            let worker_id = Uuid::now_v7();
            loop {
                match outbox::process_mobile_push_once(&mobile_push_state, worker_id).await {
                    Ok(true) => {}
                    Ok(false) => tokio::time::sleep(poll_interval).await,
                    Err(error) => {
                        error!(slot, ?error, "mobile push worker iteration failed");
                        tokio::time::sleep(poll_interval).await;
                    }
                }
            }
        });
    }
    let telegram_receiver_state = state.clone();
    tasks.spawn(async move {
        let worker_id = Uuid::now_v7();
        loop {
            match telegram_bot::maintain_receiver_once(&telegram_receiver_state, worker_id).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(poll_interval).await,
                Err(error) => {
                    error!(?error, "Telegram receiver maintenance failed");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    });
    let inbox_routing_state = state.clone();
    tasks.spawn(async move {
        let worker_id = Uuid::now_v7();
        loop {
            match inbox_routing::process_sla_once(&inbox_routing_state, worker_id).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(poll_interval).await,
                Err(error) => {
                    error!(?error, "Inbox SLA worker iteration failed");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    });
    let blacklist_state = state.clone();
    tasks.spawn(async move {
        let worker_id = Uuid::now_v7();
        loop {
            match conversations::process_blacklist_reply_once(&blacklist_state, worker_id).await {
                Ok(true) => {}
                Ok(false) => tokio::time::sleep(poll_interval).await,
                Err(error) => {
                    error!(?error, "blacklist reply worker iteration failed");
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    });
    for _ in 0..state.config.worker.ai_run_concurrency {
        let provider_reply_state = state.clone();
        tasks.spawn(async move {
            let worker_id = Uuid::now_v7();
            loop {
                match provider_reply::process_once(&provider_reply_state, worker_id).await {
                    Ok(true) => {}
                    Ok(false) => tokio::time::sleep(poll_interval).await,
                    Err(error) => {
                        error!(?error, "provider reply worker iteration failed");
                        tokio::time::sleep(poll_interval).await;
                    }
                }
            }
        });
    }
    if state.config.openclaw.enabled() {
        let openclaw_maintenance_state = state.clone();
        tasks.spawn(async move {
            let interval = openclaw_maintenance_state.config.openclaw.grant_ttl();
            loop {
                if let Err(error) =
                    provider_reply::maintain_openclaw_runtime(&openclaw_maintenance_state).await
                {
                    error!(?error, "OpenClaw runtime maintenance failed");
                }
                tokio::time::sleep(interval).await;
            }
        });
    }
    let ai_task_concurrency = state.config.worker.effective_ai_task_concurrency();
    for _ in 0..state.config.worker.ai_run_concurrency {
        let ai_api_state = state.clone();
        tasks.spawn(async move {
            loop {
                match ai_api::process_once(&ai_api_state).await {
                    Ok(true) => {}
                    Ok(false) => tokio::time::sleep(poll_interval).await,
                    Err(error) => {
                        error!(?error, "AI API worker iteration failed");
                        tokio::time::sleep(poll_interval).await;
                    }
                }
            }
        });
    }
    for slot in 0..ai_task_concurrency {
        let ai_task_state = state.clone();
        tasks.spawn(async move {
            loop {
                if let Err(error) = ai_orchestration::process_once(&ai_task_state).await {
                    error!(slot, ?error, "AI team worker iteration failed");
                }
                match ai_tasks::process_once(&ai_task_state).await {
                    Ok(true) => {}
                    Ok(false) => tokio::time::sleep(poll_interval).await,
                    Err(error) => {
                        error!(slot, ?error, "AI task worker iteration failed");
                        tokio::time::sleep(poll_interval).await;
                    }
                }
            }
        });
    }
    let purge_state = state.clone();
    tasks.spawn(async move {
        loop {
            match visitor_intelligence::purge_expired(&purge_state.db).await {
                Ok(count) if count > 0 => {
                    info!(sessions = count, "expired widget visitor data purged");
                }
                Ok(_) => {}
                Err(error) => {
                    error!(?error, "widget visitor data purge failed");
                }
            }
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
    let conversation_state = state.clone();
    tasks.spawn(async move {
        loop {
            match conversations::return_unanswered_operator_conversations_to_ai(
                &conversation_state.db,
            )
            .await
            {
                Ok(count) if count > 0 => {
                    info!(
                        conversations = count,
                        "unanswered operator conversations returned to AI"
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    error!(?error, "automatic operator-to-AI handoff failed");
                }
            }
            match conversation_maintenance::schedule_operator_auto_resolutions(
                &conversation_state.db,
            )
            .await
            {
                Ok(count) if count > 0 => {
                    info!(
                        conversations = count,
                        "operator conversation auto-resolutions scheduled"
                    );
                    if count == conversation_maintenance::INACTIVE_CONVERSATION_BATCH_SIZE {
                        continue;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    error!(?error, "operator auto-resolution scheduling failed");
                }
            }
            match conversation_maintenance::auto_resolve_inactive(&conversation_state.db).await {
                Ok(count) if count > 0 => {
                    info!(
                        conversations = count,
                        "inactive conversations automatically resolved"
                    );
                    if count == conversation_maintenance::INACTIVE_CONVERSATION_BATCH_SIZE {
                        continue;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    error!(?error, "inactive-conversation automatic resolution failed");
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
    let attachment_state = state.clone();
    tasks.spawn(async move {
        loop {
            match attachments::purge_expired(&attachment_state).await {
                Ok(count) if count > 0 => {
                    info!(attachments = count, "expired chat attachments purged");
                    if count == 100 {
                        continue;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    error!(?error, "chat attachment purge failed");
                }
            }
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
    let attachment_reconcile_state = state.clone();
    tasks.spawn(async move {
        loop {
            match attachments::purge_stale_storage(&attachment_reconcile_state).await {
                Ok(count) if count > 0 => {
                    info!(files = count, "stale attachment storage files removed");
                    if count >= 100 {
                        continue;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    error!(?error, "attachment storage reconciliation failed");
                }
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
    tasks.spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .merge(health::worker_router())
                .with_state(state),
        )
        .await
        .context("worker health server failed")
    });

    tokio::select! {
        result = tasks.join_next() => {
            match result {
                Some(Ok(Ok(()))) | None => Ok(()),
                Some(Ok(Err(error))) => Err(error),
                Some(Err(error)) => Err(error).context("worker task panicked"),
            }
        }
        () = shutdown_signal() => {
            info!("worker shutdown requested");
            tasks.abort_all();
            Ok(())
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install termination signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
