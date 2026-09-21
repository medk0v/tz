//! Shared durable capacity for API calls, scheduled work and conversation replies.

use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

const CLAIM_LOCK: i64 = 6_075_998_209_141_571_923;

pub(crate) struct Capacity {
    pub blocked_profiles: Vec<Uuid>,
    pub busy_conversations: Vec<Uuid>,
}

/// Keep this transaction open through queue claiming and lease insertion.
pub(crate) async fn available(
    tx: &mut Transaction<'_, Postgres>,
    limit: usize,
) -> Result<Option<Capacity>> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CLAIM_LOCK)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM ai_execution_leases WHERE expires_at<=now()")
        .execute(&mut **tx)
        .await?;
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_execution_leases")
        .fetch_one(&mut **tx)
        .await?;
    if active >= i64::try_from(limit)? {
        return Ok(None);
    }
    let blocked_profiles = sqlx::query_scalar(
        "SELECT p.id FROM ai_profiles p JOIN ai_execution_leases l ON l.ai_profile_id=p.id
         GROUP BY p.id,p.max_concurrent_runs HAVING count(*)>=p.max_concurrent_runs",
    )
    .fetch_all(&mut **tx)
    .await?;
    let busy_conversations = sqlx::query_scalar(
        "SELECT conversation_id FROM ai_execution_leases WHERE conversation_id IS NOT NULL",
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(Some(Capacity {
        blocked_profiles,
        busy_conversations,
    }))
}

pub(crate) async fn reserve(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    profile: Option<Uuid>,
    conversation: Option<Uuid>,
    seconds: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO ai_execution_leases (id,ai_profile_id,conversation_id,expires_at)
                 VALUES ($1,$2,$3,now()+($4::bigint * interval '1 second'))",
    )
    .bind(id)
    .bind(profile)
    .bind(conversation)
    .bind(seconds)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Explicit release on normal completion, with cancellation/panic cleanup as a fallback.
pub(crate) struct Lease {
    db: PgPool,
    id: Option<Uuid>,
}

impl Lease {
    pub(crate) fn new(db: &PgPool, id: Uuid) -> Self {
        Self {
            db: db.clone(),
            id: Some(id),
        }
    }

    pub(crate) async fn release(mut self) -> Result<()> {
        if let Some(id) = self.id {
            sqlx::query("DELETE FROM ai_execution_leases WHERE id=$1")
                .bind(id)
                .execute(&self.db)
                .await?;
            self.id = None;
        }
        Ok(())
    }

    /// Auto-resolution chooses its profile after claiming the conversation.
    pub(crate) async fn assign_profile(&self, profile: Uuid) -> Result<()> {
        let id = self
            .id
            .ok_or_else(|| anyhow::anyhow!("execution lease was released"))?;
        let wait = async {
            loop {
                let mut tx = self.db.begin().await?;
                sqlx::query("SELECT pg_advisory_xact_lock($1)")
                    .bind(CLAIM_LOCK)
                    .execute(&mut *tx)
                    .await?;
                let assigned = sqlx::query("UPDATE ai_execution_leases SET ai_profile_id=$2
                    WHERE id=$1 AND expires_at>now() AND (
                      SELECT count(*) FROM ai_execution_leases WHERE ai_profile_id=$2 AND id<>$1 AND expires_at>now()
                    ) < (SELECT max_concurrent_runs FROM ai_profiles WHERE id=$2)")
                    .bind(id).bind(profile).execute(&mut *tx).await?.rows_affected() == 1;
                tx.commit().await?;
                if assigned {
                    return Ok::<_, anyhow::Error>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(60), wait).await??;
        Ok(())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else { return };
        let db = self.db.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = sqlx::query("DELETE FROM ai_execution_leases WHERE id=$1")
                    .bind(id)
                    .execute(&db)
                    .await
                {
                    tracing::warn!(%id, ?error, "could not release AI execution capacity");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::Barrier;

    async fn profiles(db: &PgPool) -> [Uuid; 2] {
        sqlx::raw_sql("INSERT INTO tenants (id,name) VALUES ('00000000-0000-0000-0000-000000000001','Capacity');
            INSERT INTO projects (id,tenant_id,name,slug) VALUES ('00000000-0000-0000-0000-000000000002','00000000-0000-0000-0000-000000000001','Capacity','capacity');
            INSERT INTO users (id,email,display_name) VALUES ('00000000-0000-0000-0000-000000000005','capacity@example.test','Capacity');
            INSERT INTO ai_profiles (id,tenant_id,project_id,name,status,created_by) VALUES
            ('00000000-0000-0000-0000-000000000003','00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-000000000002','First','draft','00000000-0000-0000-0000-000000000005'),
            ('00000000-0000-0000-0000-000000000004','00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-000000000002','Second','draft','00000000-0000-0000-0000-000000000005');")
            .execute(db).await.unwrap();
        [Uuid::from_u128(3), Uuid::from_u128(4)]
    }

    #[sqlx::test]
    #[ignore = "requires disposable PostgreSQL databases"]
    async fn racing_workers_obey_global_and_profile_limits(db: PgPool) {
        let profiles = profiles(&db).await;
        let barrier = Arc::new(Barrier::new(12));
        let mut workers = Vec::new();
        for index in 0..12 {
            let db = db.clone();
            let barrier = barrier.clone();
            workers.push(tokio::spawn(async move {
                barrier.wait().await;
                let mut tx = db.begin().await.unwrap();
                if let Some(capacity) = available(&mut tx, 3).await.unwrap()
                    && !capacity.blocked_profiles.contains(&profiles[index % 2])
                {
                    reserve(&mut tx, Uuid::now_v7(), Some(profiles[index % 2]), None, 60)
                        .await
                        .unwrap();
                }
                tx.commit().await.unwrap();
            }));
        }
        for worker in workers {
            worker.await.unwrap();
        }
        let counts: Vec<i64> =
            sqlx::query_scalar("SELECT count(*) FROM ai_execution_leases GROUP BY ai_profile_id")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(counts.iter().sum::<i64>(), 3);
        assert!(counts.iter().all(|count| *count <= 2));
    }

    #[sqlx::test]
    #[ignore = "requires disposable PostgreSQL databases"]
    async fn cancellation_releases_only_its_lease_and_conversation(db: PgPool) {
        let profiles = profiles(&db).await;
        let conversation = Uuid::now_v7();
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let mut tx = db.begin().await.unwrap();
        available(&mut tx, 3).await.unwrap().unwrap();
        reserve(&mut tx, first, Some(profiles[0]), Some(conversation), 60)
            .await
            .unwrap();
        reserve(&mut tx, second, Some(profiles[0]), None, 60)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut tx = db.begin().await.unwrap();
        let capacity = available(&mut tx, 3).await.unwrap().unwrap();
        assert!(capacity.blocked_profiles.contains(&profiles[0]));
        assert_eq!(capacity.busy_conversations, vec![conversation]);
        tx.commit().await.unwrap();
        let guard = Lease::new(&db, first);
        let worker = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        worker.abort();
        let _ = worker.await;
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let count: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM ai_execution_leases WHERE id=$1")
                        .bind(first)
                        .fetch_one(&db)
                        .await
                        .unwrap();
                if count == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut tx = db.begin().await.unwrap();
        let capacity = available(&mut tx, 3).await.unwrap().unwrap();
        assert!(capacity.blocked_profiles.is_empty());
        assert!(capacity.busy_conversations.is_empty());
        let remaining: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM ai_execution_leases")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        assert_eq!(remaining, vec![second]);
    }

    #[sqlx::test]
    #[ignore = "requires disposable PostgreSQL databases"]
    async fn crashed_worker_expiry_restores_capacity(db: PgPool) {
        let profile = profiles(&db).await[0];
        let id = Uuid::now_v7();
        let mut tx = db.begin().await.unwrap();
        available(&mut tx, 1).await.unwrap().unwrap();
        reserve(&mut tx, id, Some(profile), None, 60).await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = db.begin().await.unwrap();
        assert!(available(&mut tx, 1).await.unwrap().is_none());
        tx.commit().await.unwrap();
        sqlx::query("UPDATE ai_execution_leases SET expires_at=now()-interval '1 second'")
            .execute(&db)
            .await
            .unwrap();
        let mut tx = db.begin().await.unwrap();
        assert!(available(&mut tx, 1).await.unwrap().is_some());
    }
}
