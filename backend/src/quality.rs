//! Project-scoped support quality reporting.

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{AppState, auth::ActorContext, error::AppError};

const DEFAULT_PERIOD_DAYS: i64 = 30;
const MAX_PERIOD_DAYS: i64 = 3_650;
const DEFAULT_RESULT_LIMIT: i64 = 100;
const MAX_RESULT_LIMIT: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/support-quality", get(get_support_quality))
}

#[derive(Debug, Deserialize)]
struct SupportQualityQuery {
    inbox_id: Option<Uuid>,
    operator_id: Option<Uuid>,
    team_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    period_days: Option<i64>,
    /// One through five filters by score; zero shows resolutions without a rating.
    rating: Option<i16>,
    limit: Option<i64>,
}

#[derive(Debug, FromRow)]
struct QualityAggregateRow {
    total_resolutions: i64,
    rated_resolutions: i64,
    average_rating: Option<f64>,
    satisfied_resolutions: i64,
    reopened_resolutions: i64,
    average_first_response_seconds: Option<f64>,
    average_resolution_seconds: Option<f64>,
    rating_1_count: i64,
    rating_2_count: i64,
    rating_3_count: i64,
    rating_4_count: i64,
    rating_5_count: i64,
}

#[derive(Debug, Serialize)]
struct SupportQualitySummary {
    total_resolutions: i64,
    rated_resolutions: i64,
    average_rating: Option<f64>,
    csat_percent: Option<f64>,
    response_rate_percent: f64,
    reopened_rate_percent: f64,
    average_first_response_seconds: Option<f64>,
    average_resolution_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct RatingDistributionItem {
    rating: i16,
    count: i64,
}

#[derive(Debug, FromRow, Serialize)]
struct QualityFilterOption {
    id: Uuid,
    name: String,
}

#[derive(Debug, Serialize)]
struct SupportQualityFilters {
    operators: Vec<QualityFilterOption>,
    teams: Vec<QualityFilterOption>,
    channels: Vec<QualityFilterOption>,
}

#[derive(Debug, FromRow)]
struct QualityTrendRow {
    period_started_at: DateTime<Utc>,
    total_resolutions: i64,
    rated_resolutions: i64,
    average_rating: Option<f64>,
    satisfied_resolutions: i64,
    average_first_response_seconds: Option<f64>,
    average_resolution_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct QualityTrendPoint {
    period_started_at: DateTime<Utc>,
    total_resolutions: i64,
    rated_resolutions: i64,
    average_rating: Option<f64>,
    csat_percent: Option<f64>,
    average_first_response_seconds: Option<f64>,
    average_resolution_seconds: Option<f64>,
}

#[derive(Debug, FromRow, Serialize)]
struct SupportQualityItem {
    resolution_id: Uuid,
    conversation_id: Uuid,
    cycle_number: i32,
    conversation_subject: Option<String>,
    contact_id: Uuid,
    contact_name: Option<String>,
    inbox_id: Uuid,
    inbox_name: String,
    channel_id: Uuid,
    channel_name: String,
    channel_kind: String,
    responsible_actor_id: Option<Uuid>,
    responsible_operator_name: Option<String>,
    responsible_ai_profile_id: Option<Uuid>,
    responsible_ai_profile_name: Option<String>,
    responsible_team_id: Option<Uuid>,
    responsible_team_name: Option<String>,
    resolved_at: DateTime<Utc>,
    reopened_at: Option<DateTime<Utc>>,
    first_response_seconds: Option<f64>,
    resolution_seconds: f64,
    rating_id: Option<Uuid>,
    rating: Option<i16>,
    comment: Option<String>,
    reasons: Vec<String>,
    rated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct SupportQualityResponse {
    period_started_at: DateTime<Utc>,
    summary: SupportQualitySummary,
    distribution: Vec<RatingDistributionItem>,
    trend: Vec<QualityTrendPoint>,
    available_filters: SupportQualityFilters,
    items: Vec<SupportQualityItem>,
}

async fn get_support_quality(
    State(state): State<AppState>,
    actor: ActorContext,
    Query(query): Query<SupportQualityQuery>,
) -> Result<Json<SupportQualityResponse>, AppError> {
    actor.require("quality:read")?;
    let project_id = actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))?;
    actor.require_project(project_id)?;

    let period_days = query
        .period_days
        .unwrap_or(DEFAULT_PERIOD_DAYS)
        .clamp(1, MAX_PERIOD_DAYS);
    let period_started_at = Utc::now() - Duration::days(period_days);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_RESULT_LIMIT)
        .clamp(1, MAX_RESULT_LIMIT);
    if query
        .rating
        .is_some_and(|rating| !(0..=5).contains(&rating))
    {
        return Err(AppError::BadRequest(
            "rating filter must be between 0 and 5".to_owned(),
        ));
    }

    if let Some(inbox_id) = query.inbox_id {
        let inbox_project_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT project_id FROM inboxes WHERE tenant_id = $1 AND id = $2",
        )
        .bind(actor.tenant_id)
        .bind(inbox_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
        if inbox_project_id != project_id {
            return Err(AppError::NotFound);
        }
        actor.require_inbox(project_id, inbox_id)?;
    }

    let inbox_scope = actor.inbox_scope().map(<[Uuid]>::to_vec);
    let can_read_all_quality = actor
        .permissions()
        .iter()
        .any(|permission| permission == "quality:read_all");
    let aggregate = sqlx::query_as::<_, QualityAggregateRow>(
        r#"
        SELECT
            COUNT(*)::bigint AS total_resolutions,
            COUNT(rating.id)::bigint AS rated_resolutions,
            AVG(rating.rating)::float8 AS average_rating,
            COUNT(*) FILTER (WHERE rating.rating >= 4)::bigint AS satisfied_resolutions,
            COUNT(*) FILTER (WHERE resolution.reopened_at IS NOT NULL)::bigint
                AS reopened_resolutions,
            AVG(EXTRACT(EPOCH FROM first_response.created_at - conversation.created_at))::float8
                AS average_first_response_seconds,
            AVG(EXTRACT(EPOCH FROM resolution.resolved_at - conversation.created_at))::float8
                AS average_resolution_seconds,
            COUNT(*) FILTER (WHERE rating.rating = 1)::bigint AS rating_1_count,
            COUNT(*) FILTER (WHERE rating.rating = 2)::bigint AS rating_2_count,
            COUNT(*) FILTER (WHERE rating.rating = 3)::bigint AS rating_3_count,
            COUNT(*) FILTER (WHERE rating.rating = 4)::bigint AS rating_4_count,
            COUNT(*) FILTER (WHERE rating.rating = 5)::bigint AS rating_5_count
        FROM conversation_resolutions AS resolution
        JOIN conversations AS conversation
          ON conversation.tenant_id = resolution.tenant_id
         AND conversation.id = resolution.conversation_id
        LEFT JOIN support_ratings AS rating
          ON rating.tenant_id = resolution.tenant_id
         AND rating.resolution_id = resolution.id
        LEFT JOIN LATERAL (
            SELECT message.created_at
            FROM messages AS message
            WHERE message.tenant_id = resolution.tenant_id
              AND message.conversation_id = resolution.conversation_id
              AND message.direction = 'outbound'
            ORDER BY message.created_at, message.id
            LIMIT 1
        ) AS first_response ON TRUE
        WHERE resolution.tenant_id = $1
          AND resolution.project_id = $2
          AND resolution.resolved_at >= $3
          AND (NOT $11::boolean OR EXISTS (
              SELECT 1 FROM conversations AS demo_conversation
              JOIN widget_sessions AS demo_session
                ON demo_session.tenant_id = demo_conversation.tenant_id
               AND demo_session.project_id = demo_conversation.project_id
               AND demo_session.channel_connection_id = demo_conversation.channel_connection_id
               AND demo_session.contact_id = demo_conversation.contact_id
              WHERE demo_conversation.tenant_id = resolution.tenant_id
                AND demo_conversation.id = resolution.conversation_id
                AND demo_conversation.channel_connection_id = $13
                AND demo_session.client_ip = $12::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND ($4::uuid IS NULL OR resolution.inbox_id = $4)
          AND ($5::uuid[] IS NULL OR resolution.inbox_id = ANY($5))
          AND (
            $6::boolean
            OR resolution.responsible_actor_id = $7
          )
          AND ($8::uuid IS NULL OR resolution.responsible_actor_id = $8)
          AND ($9::uuid IS NULL OR resolution.responsible_team_id = $9)
          AND ($10::uuid IS NULL OR conversation.channel_connection_id = $10)
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(period_started_at)
    .bind(query.inbox_id)
    .bind(inbox_scope.as_deref())
    .bind(can_read_all_quality)
    .bind(actor.actor_id)
    .bind(query.operator_id)
    .bind(query.team_id)
    .bind(query.channel_id)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_one(&state.db)
    .await?;

    let trend_bucket = if period_days <= 31 { "day" } else { "week" };
    let trend_rows = sqlx::query_as::<_, QualityTrendRow>(
        r#"
        SELECT
            date_trunc($11, resolution.resolved_at) AS period_started_at,
            COUNT(*)::bigint AS total_resolutions,
            COUNT(rating.id)::bigint AS rated_resolutions,
            AVG(rating.rating)::float8 AS average_rating,
            COUNT(*) FILTER (WHERE rating.rating >= 4)::bigint AS satisfied_resolutions,
            AVG(EXTRACT(EPOCH FROM first_response.created_at - conversation.created_at))::float8
                AS average_first_response_seconds,
            AVG(EXTRACT(EPOCH FROM resolution.resolved_at - conversation.created_at))::float8
                AS average_resolution_seconds
        FROM conversation_resolutions AS resolution
        JOIN conversations AS conversation
          ON conversation.tenant_id = resolution.tenant_id
         AND conversation.id = resolution.conversation_id
        LEFT JOIN support_ratings AS rating
          ON rating.tenant_id = resolution.tenant_id
         AND rating.resolution_id = resolution.id
        LEFT JOIN LATERAL (
            SELECT message.created_at
            FROM messages AS message
            WHERE message.tenant_id = resolution.tenant_id
              AND message.conversation_id = resolution.conversation_id
              AND message.direction = 'outbound'
            ORDER BY message.created_at, message.id
            LIMIT 1
        ) AS first_response ON TRUE
        WHERE resolution.tenant_id = $1
          AND resolution.project_id = $2
          AND resolution.resolved_at >= $3
          AND (NOT $12::boolean OR EXISTS (
              SELECT 1 FROM conversations AS demo_conversation
              JOIN widget_sessions AS demo_session
                ON demo_session.tenant_id = demo_conversation.tenant_id
               AND demo_session.project_id = demo_conversation.project_id
               AND demo_session.channel_connection_id = demo_conversation.channel_connection_id
               AND demo_session.contact_id = demo_conversation.contact_id
              WHERE demo_conversation.tenant_id = resolution.tenant_id
                AND demo_conversation.id = resolution.conversation_id
                AND demo_conversation.channel_connection_id = $14
                AND demo_session.client_ip = $13::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND ($4::uuid IS NULL OR resolution.inbox_id = $4)
          AND ($5::uuid[] IS NULL OR resolution.inbox_id = ANY($5))
          AND (
            $6::boolean
            OR resolution.responsible_actor_id = $7
          )
          AND ($8::uuid IS NULL OR resolution.responsible_actor_id = $8)
          AND ($9::uuid IS NULL OR resolution.responsible_team_id = $9)
          AND ($10::uuid IS NULL OR conversation.channel_connection_id = $10)
        GROUP BY period_started_at
        ORDER BY period_started_at
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(period_started_at)
    .bind(query.inbox_id)
    .bind(inbox_scope.as_deref())
    .bind(can_read_all_quality)
    .bind(actor.actor_id)
    .bind(query.operator_id)
    .bind(query.team_id)
    .bind(query.channel_id)
    .bind(trend_bucket)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_all(&state.db)
    .await?;
    let trend = trend_rows
        .into_iter()
        .map(|row| QualityTrendPoint {
            period_started_at: row.period_started_at,
            total_resolutions: row.total_resolutions,
            rated_resolutions: row.rated_resolutions,
            average_rating: row.average_rating,
            csat_percent: percentage(row.satisfied_resolutions, row.rated_resolutions),
            average_first_response_seconds: row.average_first_response_seconds,
            average_resolution_seconds: row.average_resolution_seconds,
        })
        .collect();

    let items = sqlx::query_as::<_, SupportQualityItem>(
        r#"
        SELECT
            resolution.id AS resolution_id,
            resolution.conversation_id,
            resolution.cycle_number,
            conversation.subject AS conversation_subject,
            conversation.contact_id,
            contact.display_name AS contact_name,
            resolution.inbox_id,
            inbox.name AS inbox_name,
            conversation.channel_connection_id AS channel_id,
            channel.name AS channel_name,
            channel.kind AS channel_kind,
            resolution.responsible_actor_id,
            responsible.display_name AS responsible_operator_name,
            resolution.responsible_ai_profile_id,
            responsible_ai_profile.name AS responsible_ai_profile_name,
            resolution.responsible_team_id,
            responsible_team.name AS responsible_team_name,
            resolution.resolved_at,
            resolution.reopened_at,
            EXTRACT(EPOCH FROM first_response.created_at - conversation.created_at)::float8
                AS first_response_seconds,
            EXTRACT(EPOCH FROM resolution.resolved_at - conversation.created_at)::float8
                AS resolution_seconds,
            rating.id AS rating_id,
            rating.rating,
            rating.comment,
            COALESCE(rating_reasons.reasons, ARRAY[]::text[]) AS reasons,
            rating.created_at AS rated_at
        FROM conversation_resolutions AS resolution
        JOIN conversations AS conversation
          ON conversation.tenant_id = resolution.tenant_id
         AND conversation.id = resolution.conversation_id
        JOIN contacts AS contact
          ON contact.tenant_id = conversation.tenant_id
         AND contact.id = conversation.contact_id
        JOIN inboxes AS inbox
          ON inbox.tenant_id = resolution.tenant_id
         AND inbox.id = resolution.inbox_id
        JOIN channel_connections AS channel
          ON channel.tenant_id = conversation.tenant_id
         AND channel.id = conversation.channel_connection_id
        LEFT JOIN users AS responsible
          ON responsible.id = resolution.responsible_actor_id
        LEFT JOIN ai_profiles AS responsible_ai_profile
          ON responsible_ai_profile.tenant_id = resolution.tenant_id
         AND responsible_ai_profile.id = resolution.responsible_ai_profile_id
        LEFT JOIN teams AS responsible_team
          ON responsible_team.tenant_id = resolution.tenant_id
         AND responsible_team.id = resolution.responsible_team_id
        LEFT JOIN support_ratings AS rating
          ON rating.tenant_id = resolution.tenant_id
         AND rating.resolution_id = resolution.id
        LEFT JOIN LATERAL (
            SELECT message.created_at
            FROM messages AS message
            WHERE message.tenant_id = resolution.tenant_id
              AND message.conversation_id = resolution.conversation_id
              AND message.direction = 'outbound'
            ORDER BY message.created_at, message.id
            LIMIT 1
        ) AS first_response ON TRUE
        LEFT JOIN LATERAL (
            SELECT ARRAY_AGG(reason.reason ORDER BY reason.created_at, reason.id) AS reasons
            FROM support_rating_reasons AS reason
            WHERE reason.tenant_id = rating.tenant_id
              AND reason.support_rating_id = rating.id
        ) AS rating_reasons ON TRUE
        WHERE resolution.tenant_id = $1
          AND resolution.project_id = $2
          AND resolution.resolved_at >= $3
          AND (NOT $13::boolean OR EXISTS (
              SELECT 1 FROM conversations AS demo_conversation
              JOIN widget_sessions AS demo_session
                ON demo_session.tenant_id = demo_conversation.tenant_id
               AND demo_session.project_id = demo_conversation.project_id
               AND demo_session.channel_connection_id = demo_conversation.channel_connection_id
               AND demo_session.contact_id = demo_conversation.contact_id
              WHERE demo_conversation.tenant_id = resolution.tenant_id
                AND demo_conversation.id = resolution.conversation_id
                AND demo_conversation.channel_connection_id = $15
                AND demo_session.client_ip = $14::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND ($4::uuid IS NULL OR resolution.inbox_id = $4)
          AND ($5::uuid[] IS NULL OR resolution.inbox_id = ANY($5))
          AND (
            $6::boolean
            OR resolution.responsible_actor_id = $7
          )
          AND ($8::uuid IS NULL OR resolution.responsible_actor_id = $8)
          AND ($9::uuid IS NULL OR resolution.responsible_team_id = $9)
          AND ($10::uuid IS NULL OR conversation.channel_connection_id = $10)
          AND (
            $11::smallint IS NULL
            OR ($11 = 0 AND rating.id IS NULL)
            OR rating.rating = $11
          )
        ORDER BY COALESCE(rating.created_at, resolution.resolved_at) DESC, resolution.id DESC
        LIMIT $12
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(period_started_at)
    .bind(query.inbox_id)
    .bind(inbox_scope.as_deref())
    .bind(can_read_all_quality)
    .bind(actor.actor_id)
    .bind(query.operator_id)
    .bind(query.team_id)
    .bind(query.channel_id)
    .bind(query.rating)
    .bind(limit)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_all(&state.db)
    .await?;

    let operators = sqlx::query_as::<_, QualityFilterOption>(
        r#"
        SELECT DISTINCT responsible.id, responsible.display_name AS name
        FROM conversation_resolutions AS resolution
        JOIN users AS responsible ON responsible.id = resolution.responsible_actor_id
        WHERE resolution.tenant_id = $1
          AND resolution.project_id = $2
          AND resolution.resolved_at >= $3
          AND (NOT $8::boolean OR EXISTS (
              SELECT 1 FROM conversations AS demo_conversation
              JOIN widget_sessions AS demo_session
                ON demo_session.tenant_id = demo_conversation.tenant_id
               AND demo_session.project_id = demo_conversation.project_id
               AND demo_session.channel_connection_id = demo_conversation.channel_connection_id
               AND demo_session.contact_id = demo_conversation.contact_id
              WHERE demo_conversation.tenant_id = resolution.tenant_id
                AND demo_conversation.id = resolution.conversation_id
                AND demo_conversation.channel_connection_id = $10
                AND demo_session.client_ip = $9::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND ($4::uuid IS NULL OR resolution.inbox_id = $4)
          AND ($5::uuid[] IS NULL OR resolution.inbox_id = ANY($5))
          AND (
            $6::boolean
            OR resolution.responsible_actor_id = $7
          )
        ORDER BY name, responsible.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(period_started_at)
    .bind(query.inbox_id)
    .bind(inbox_scope.as_deref())
    .bind(can_read_all_quality)
    .bind(actor.actor_id)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_all(&state.db)
    .await?;
    let teams = sqlx::query_as::<_, QualityFilterOption>(
        r#"
        SELECT DISTINCT responsible_team.id, responsible_team.name
        FROM conversation_resolutions AS resolution
        JOIN teams AS responsible_team
          ON responsible_team.tenant_id = resolution.tenant_id
         AND responsible_team.id = resolution.responsible_team_id
        WHERE resolution.tenant_id = $1
          AND resolution.project_id = $2
          AND resolution.resolved_at >= $3
          AND (NOT $8::boolean OR EXISTS (
              SELECT 1 FROM conversations AS demo_conversation
              JOIN widget_sessions AS demo_session
                ON demo_session.tenant_id = demo_conversation.tenant_id
               AND demo_session.project_id = demo_conversation.project_id
               AND demo_session.channel_connection_id = demo_conversation.channel_connection_id
               AND demo_session.contact_id = demo_conversation.contact_id
              WHERE demo_conversation.tenant_id = resolution.tenant_id
                AND demo_conversation.id = resolution.conversation_id
                AND demo_conversation.channel_connection_id = $10
                AND demo_session.client_ip = $9::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND ($4::uuid IS NULL OR resolution.inbox_id = $4)
          AND ($5::uuid[] IS NULL OR resolution.inbox_id = ANY($5))
          AND (
            $6::boolean
            OR resolution.responsible_actor_id = $7
          )
        ORDER BY responsible_team.name, responsible_team.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(period_started_at)
    .bind(query.inbox_id)
    .bind(inbox_scope.as_deref())
    .bind(can_read_all_quality)
    .bind(actor.actor_id)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_all(&state.db)
    .await?;
    let channels = sqlx::query_as::<_, QualityFilterOption>(
        r#"
        SELECT DISTINCT channel.id, channel.name
        FROM conversation_resolutions AS resolution
        JOIN conversations AS conversation
          ON conversation.tenant_id = resolution.tenant_id
         AND conversation.id = resolution.conversation_id
        JOIN channel_connections AS channel
          ON channel.tenant_id = conversation.tenant_id
         AND channel.id = conversation.channel_connection_id
        WHERE resolution.tenant_id = $1
          AND resolution.project_id = $2
          AND resolution.resolved_at >= $3
          AND (NOT $8::boolean OR EXISTS (
              SELECT 1 FROM conversations AS demo_conversation
              JOIN widget_sessions AS demo_session
                ON demo_session.tenant_id = demo_conversation.tenant_id
               AND demo_session.project_id = demo_conversation.project_id
               AND demo_session.channel_connection_id = demo_conversation.channel_connection_id
               AND demo_session.contact_id = demo_conversation.contact_id
              WHERE demo_conversation.tenant_id = resolution.tenant_id
                AND demo_conversation.id = resolution.conversation_id
                AND demo_conversation.channel_connection_id = $10
                AND demo_session.client_ip = $9::text::inet
                AND demo_session.revoked_at IS NULL
                AND demo_session.visitor_data_expires_at > now()
          ))
          AND ($4::uuid IS NULL OR resolution.inbox_id = $4)
          AND ($5::uuid[] IS NULL OR resolution.inbox_id = ANY($5))
          AND (
            $6::boolean
            OR resolution.responsible_actor_id = $7
          )
        ORDER BY channel.name, channel.id
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(period_started_at)
    .bind(query.inbox_id)
    .bind(inbox_scope.as_deref())
    .bind(can_read_all_quality)
    .bind(actor.actor_id)
    .bind(actor.is_demo())
    .bind(actor.demo_login_ip())
    .bind(crate::demo::DEMO_CHANNEL_ID)
    .fetch_all(&state.db)
    .await?;

    let summary = quality_summary(&aggregate);
    let distribution = vec![
        RatingDistributionItem {
            rating: 5,
            count: aggregate.rating_5_count,
        },
        RatingDistributionItem {
            rating: 4,
            count: aggregate.rating_4_count,
        },
        RatingDistributionItem {
            rating: 3,
            count: aggregate.rating_3_count,
        },
        RatingDistributionItem {
            rating: 2,
            count: aggregate.rating_2_count,
        },
        RatingDistributionItem {
            rating: 1,
            count: aggregate.rating_1_count,
        },
    ];

    Ok(Json(SupportQualityResponse {
        period_started_at,
        summary,
        distribution,
        trend,
        available_filters: SupportQualityFilters {
            operators,
            teams,
            channels,
        },
        items,
    }))
}

fn quality_summary(aggregate: &QualityAggregateRow) -> SupportQualitySummary {
    SupportQualitySummary {
        total_resolutions: aggregate.total_resolutions,
        rated_resolutions: aggregate.rated_resolutions,
        average_rating: aggregate.average_rating,
        csat_percent: percentage(aggregate.satisfied_resolutions, aggregate.rated_resolutions),
        response_rate_percent: percentage(aggregate.rated_resolutions, aggregate.total_resolutions)
            .unwrap_or(0.0),
        reopened_rate_percent: percentage(
            aggregate.reopened_resolutions,
            aggregate.total_resolutions,
        )
        .unwrap_or(0.0),
        average_first_response_seconds: aggregate.average_first_response_seconds,
        average_resolution_seconds: aggregate.average_resolution_seconds,
    }
}

#[allow(clippy::cast_precision_loss)]
fn percentage(numerator: i64, denominator: i64) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 * 100.0 / denominator as f64)
}

#[cfg(test)]
mod tests {
    use super::percentage;

    #[test]
    fn percentage_is_absent_without_a_denominator() {
        assert_eq!(percentage(0, 0), None);
        assert_eq!(percentage(4, 5), Some(80.0));
    }
}
