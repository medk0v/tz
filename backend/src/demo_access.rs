//! IP isolation for the public demo's conversations and realtime events.

use std::net::IpAddr;

use sqlx::PgPool;
use uuid::Uuid;

use crate::{auth::ActorContext, demo::DEMO_CHANNEL_ID, error::AppError};

pub(crate) async fn require_conversation(
    db: &PgPool,
    actor: &ActorContext,
    conversation_id: Uuid,
) -> Result<(), AppError> {
    if !actor.is_demo() {
        return Ok(());
    }
    let login_ip = actor.demo_login_ip().ok_or(AppError::Forbidden)?;
    let allowed = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM conversations AS conversation
            JOIN widget_sessions AS session
              ON session.tenant_id = conversation.tenant_id
             AND session.project_id = conversation.project_id
             AND session.channel_connection_id = conversation.channel_connection_id
             AND session.contact_id = conversation.contact_id
            WHERE conversation.tenant_id = $1 AND conversation.id = $2
              AND conversation.channel_connection_id = $3
              AND session.client_ip = $4::text::inet
              AND session.revoked_at IS NULL
              AND session.visitor_data_expires_at > now()
        )
        "#,
    )
    .bind(actor.tenant_id)
    .bind(conversation_id)
    .bind(DEMO_CHANNEL_ID)
    .bind(login_ip)
    .fetch_one(db)
    .await?;
    if allowed {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

pub(crate) async fn contact_is_visible(
    db: &PgPool,
    tenant_id: Uuid,
    login_ip: &str,
    contact_id: Option<Uuid>,
) -> Result<bool, AppError> {
    let Some(contact_id) = contact_id else {
        return Ok(false);
    };
    Ok(sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM widget_sessions
            WHERE tenant_id = $1 AND contact_id = $2
              AND channel_connection_id = $3 AND client_ip = $4::text::inet
              AND revoked_at IS NULL AND visitor_data_expires_at > now()
        )
        "#,
    )
    .bind(tenant_id)
    .bind(contact_id)
    .bind(DEMO_CHANNEL_ID)
    .bind(login_ip)
    .fetch_one(db)
    .await?)
}

/// Keep a browser visitor ID stable within one IP, while preventing reuse from
/// another address from adopting the original demo contact and its history.
pub(crate) fn visitor_hash(visitor_id: Uuid, address: IpAddr) -> Vec<u8> {
    crate::auth::hash_token(&format!("demo:{address}:{visitor_id}"))
}

#[cfg(test)]
mod tests {
    use super::visitor_hash;
    use uuid::Uuid;

    #[test]
    fn demo_visitor_identity_is_partitioned_by_server_derived_ip() {
        let visitor = Uuid::from_u128(1);
        let first_ip = "192.0.2.1".parse().unwrap();
        let second_ip = "192.0.2.2".parse().unwrap();
        assert_eq!(
            visitor_hash(visitor, first_ip),
            visitor_hash(visitor, first_ip)
        );
        assert_ne!(
            visitor_hash(visitor, first_ip),
            visitor_hash(visitor, second_ip)
        );
        assert_ne!(
            visitor_hash(visitor, first_ip),
            visitor_hash(Uuid::from_u128(2), first_ip)
        );
    }
}
