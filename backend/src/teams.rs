//! Project-scoped team and operator administration.

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::{get, patch, put},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use url::Url;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{ActorContext, Role},
    avatars::{MAX_AVATAR_BYTES, ValidatedAvatar, public_avatar_url},
    error::AppError,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/teams", get(list_teams).post(create_team))
        .route(
            "/api/v1/teams/{team_id}",
            patch(update_team).delete(delete_team),
        )
        .route(
            "/api/v1/operators/{operator_id}/chat-profile",
            patch(update_operator_chat_profile),
        )
        .route(
            "/api/v1/operators/{operator_id}/chat-profile/avatar",
            put(upload_operator_avatar)
                .delete(delete_operator_avatar)
                .layer(DefaultBodyLimit::max(MAX_AVATAR_BYTES)),
        )
}

#[derive(Debug, FromRow)]
struct TeamRow {
    id: Uuid,
    name: String,
    member_ids: Vec<Uuid>,
    inbox_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
struct TeamResponse {
    id: Uuid,
    name: String,
    member_ids: Vec<Uuid>,
    inbox_ids: Vec<Uuid>,
}

impl From<TeamRow> for TeamResponse {
    fn from(row: TeamRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            member_ids: row.member_ids,
            inbox_ids: row.inbox_ids,
        }
    }
}

#[derive(Debug, Serialize)]
struct OperatorResponse {
    user_id: Uuid,
    email: String,
    display_name: String,
    chat_display_name: String,
    avatar_url: Option<String>,
    role: Role,
    role_id: String,
    role_name: String,
}

#[derive(Debug, FromRow)]
struct OperatorRow {
    user_id: Uuid,
    email: String,
    display_name: String,
    chat_display_name: String,
    avatar_url: Option<String>,
    role: String,
    role_id: String,
    role_name: String,
}

#[derive(Debug, Serialize)]
struct TeamListResponse {
    items: Vec<TeamResponse>,
    operators: Vec<OperatorResponse>,
}

async fn list_teams(
    State(state): State<AppState>,
    actor: ActorContext,
) -> Result<Json<TeamListResponse>, AppError> {
    let project_id = require_management(&actor)?;
    let items = load_teams(&state, &actor, project_id).await?;
    let rows = sqlx::query_as::<_, OperatorRow>(
        r#"
        SELECT DISTINCT ON (app_user.id)
               app_user.id AS user_id, app_user.email, app_user.display_name,
               COALESCE(profile.display_name, app_user.display_name) AS chat_display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS avatar_url,
               project_role.base_role AS role,
               membership.role AS role_id,
               project_role.name AS role_name
        FROM memberships AS membership
        JOIN users AS app_user ON app_user.id = membership.user_id
        JOIN project_roles AS project_role
          ON project_role.tenant_id = membership.tenant_id
         AND project_role.project_id = COALESCE((
                 SELECT role_token.role_project_id FROM api_keys AS role_token
                 WHERE role_token.tenant_id = membership.tenant_id
                   AND role_token.actor_user_id = membership.user_id
                   AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
                 LIMIT 1
             ), $2)
         AND project_role.id = membership.role
         AND NOT EXISTS (
             SELECT 1 FROM api_keys AS role_token
             JOIN projects AS role_source ON role_source.tenant_id = role_token.tenant_id
               AND role_source.id = role_token.role_project_id
             WHERE role_token.tenant_id = membership.tenant_id
               AND role_token.actor_user_id = membership.user_id
               AND role_token.role = membership.role AND role_token.role_project_id IS NOT NULL
               AND (role_token.revoked_at IS NOT NULL OR role_token.expires_at <= now()
                    OR role_source.status <> 'active')
         )
        LEFT JOIN operator_chat_profiles AS profile
          ON profile.tenant_id = membership.tenant_id
         AND profile.project_id = $2
         AND profile.user_id = app_user.id
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = membership.tenant_id
         AND stored_avatar.project_id = $2
         AND stored_avatar.user_id = app_user.id
        WHERE membership.tenant_id = $1
          AND (membership.project_id = $2 OR membership.project_id IS NULL)
          AND membership.revoked_at IS NULL
          AND app_user.status = 'active'
        ORDER BY app_user.id, (membership.project_id IS NULL), app_user.display_name, app_user.email
        LIMIT 500
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let mut operators = rows
        .into_iter()
        .map(|row| {
            Ok(OperatorResponse {
                user_id: row.user_id,
                email: row.email,
                display_name: row.display_name,
                chat_display_name: row.chat_display_name,
                avatar_url: row.avatar_url,
                role: row.role.parse()?,
                role_id: row.role_id,
                role_name: row.role_name,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    operators.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.email.cmp(&right.email))
    });
    Ok(Json(TeamListResponse { items, operators }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperatorChatProfileRequest {
    display_name: String,
    avatar_url: Option<String>,
}

#[derive(Debug, FromRow, Serialize)]
struct OperatorChatProfileResponse {
    user_id: Uuid,
    display_name: String,
    avatar_url: Option<String>,
}

async fn update_operator_chat_profile(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(operator_id): Path<Uuid>,
    Json(request): Json<OperatorChatProfileRequest>,
) -> Result<Json<OperatorChatProfileResponse>, AppError> {
    let project_id = require_profile_management(&actor, operator_id)?;
    let display_name = normalize_operator_display_name(request.display_name)?;
    let (avatar_url, replace_managed_avatar) = normalize_avatar_url(request.avatar_url)?;
    operator_default_display_name(&state, &actor, project_id, operator_id).await?;

    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO operator_chat_profiles (
            tenant_id, project_id, user_id, display_name, avatar_url
        ) VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (tenant_id, project_id, user_id) DO UPDATE
        SET display_name = EXCLUDED.display_name,
            avatar_url = EXCLUDED.avatar_url,
            updated_at = now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .bind(display_name)
    .bind(avatar_url)
    .execute(&mut *transaction)
    .await?;

    if replace_managed_avatar {
        sqlx::query(
            r#"
            DELETE FROM operator_profile_avatars
            WHERE tenant_id = $1 AND project_id = $2 AND user_id = $3
            "#,
        )
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(operator_id)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;

    Ok(Json(
        load_operator_chat_profile(&state, actor.tenant_id, project_id, operator_id).await?,
    ))
}

async fn upload_operator_avatar(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(operator_id): Path<Uuid>,
    bytes: Bytes,
) -> Result<Json<OperatorChatProfileResponse>, AppError> {
    let project_id = require_profile_management(&actor, operator_id)?;
    let default_display_name =
        operator_default_display_name(&state, &actor, project_id, operator_id).await?;
    let avatar = ValidatedAvatar::from_bytes(bytes)?;
    let public_id = avatar.public_id;
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO operator_chat_profiles (
            tenant_id, project_id, user_id, display_name, avatar_url
        ) VALUES ($1, $2, $3, $4, NULL)
        ON CONFLICT (tenant_id, project_id, user_id) DO NOTHING
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .bind(default_display_name)
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO operator_profile_avatars (
            tenant_id, project_id, user_id, public_id, media_type, content,
            sha256, width, height, created_by
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (tenant_id, project_id, user_id) DO UPDATE
        SET public_id = EXCLUDED.public_id,
            media_type = EXCLUDED.media_type,
            content = EXCLUDED.content,
            sha256 = EXCLUDED.sha256,
            width = EXCLUDED.width,
            height = EXCLUDED.height,
            created_by = EXCLUDED.created_by,
            updated_at = now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .bind(public_id)
    .bind(avatar.media_type)
    .bind(avatar.content)
    .bind(avatar.sha256)
    .bind(avatar.width)
    .bind(avatar.height)
    .bind(actor.actor_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE operator_chat_profiles
        SET avatar_url = NULL, updated_at = now()
        WHERE tenant_id = $1 AND project_id = $2 AND user_id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let mut profile =
        load_operator_chat_profile(&state, actor.tenant_id, project_id, operator_id).await?;
    profile.avatar_url = Some(public_avatar_url(public_id));
    Ok(Json(profile))
}

async fn delete_operator_avatar(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(operator_id): Path<Uuid>,
) -> Result<Json<OperatorChatProfileResponse>, AppError> {
    let project_id = require_profile_management(&actor, operator_id)?;
    let default_display_name =
        operator_default_display_name(&state, &actor, project_id, operator_id).await?;
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO operator_chat_profiles (
            tenant_id, project_id, user_id, display_name, avatar_url
        ) VALUES ($1, $2, $3, $4, NULL)
        ON CONFLICT (tenant_id, project_id, user_id) DO UPDATE
        SET avatar_url = NULL, updated_at = now()
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .bind(default_display_name)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        DELETE FROM operator_profile_avatars
        WHERE tenant_id = $1 AND project_id = $2 AND user_id = $3
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_operator_chat_profile(&state, actor.tenant_id, project_id, operator_id).await?,
    ))
}

async fn operator_default_display_name(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    operator_id: Uuid,
) -> Result<String, AppError> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT app_user.display_name
        FROM memberships AS membership
        JOIN users AS app_user ON app_user.id = membership.user_id
        WHERE membership.tenant_id = $1
          AND (membership.project_id = $2 OR membership.project_id IS NULL)
          AND membership.user_id = $3
          AND membership.revoked_at IS NULL
          AND app_user.status = 'active'
        ORDER BY (membership.project_id IS NULL)
        LIMIT 1
        "#,
    )
    .bind(actor.tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

async fn load_operator_chat_profile(
    state: &AppState,
    tenant_id: Uuid,
    project_id: Uuid,
    operator_id: Uuid,
) -> Result<OperatorChatProfileResponse, AppError> {
    sqlx::query_as::<_, OperatorChatProfileResponse>(
        r#"
        SELECT profile.user_id, profile.display_name,
               COALESCE(
                   '/public/v1/avatars/' || stored_avatar.public_id::text,
                   profile.avatar_url
               ) AS avatar_url
        FROM operator_chat_profiles AS profile
        LEFT JOIN operator_profile_avatars AS stored_avatar
          ON stored_avatar.tenant_id = profile.tenant_id
         AND stored_avatar.project_id = profile.project_id
         AND stored_avatar.user_id = profile.user_id
        WHERE profile.tenant_id = $1 AND profile.project_id = $2 AND profile.user_id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(project_id)
    .bind(operator_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TeamRequest {
    name: String,
    #[serde(default)]
    member_ids: Vec<Uuid>,
    #[serde(default)]
    inbox_ids: Vec<Uuid>,
}

async fn create_team(
    State(state): State<AppState>,
    actor: ActorContext,
    Json(request): Json<TeamRequest>,
) -> Result<(StatusCode, Json<TeamResponse>), AppError> {
    let project_id = require_management(&actor)?;
    let name = normalize_name(request.name)?;
    let team_id = Uuid::now_v7();
    let mut transaction = state.db.begin().await?;
    validate_assignments(
        &mut transaction,
        &actor,
        project_id,
        &request.member_ids,
        &request.inbox_ids,
    )
    .await?;
    sqlx::query("INSERT INTO teams (id, tenant_id, project_id, name) VALUES ($1, $2, $3, $4)")
        .bind(team_id)
        .bind(actor.tenant_id)
        .bind(project_id)
        .bind(&name)
        .execute(&mut *transaction)
        .await
        .map_err(team_write_error)?;
    replace_assignments(
        &mut transaction,
        actor.tenant_id,
        team_id,
        &request.member_ids,
        &request.inbox_ids,
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_team(&state, &actor, project_id, team_id).await?),
    ))
}

async fn update_team(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(team_id): Path<Uuid>,
    Json(request): Json<TeamRequest>,
) -> Result<Json<TeamResponse>, AppError> {
    let project_id = require_management(&actor)?;
    let name = normalize_name(request.name)?;
    let mut transaction = state.db.begin().await?;
    validate_assignments(
        &mut transaction,
        &actor,
        project_id,
        &request.member_ids,
        &request.inbox_ids,
    )
    .await?;
    let updated = sqlx::query("UPDATE teams SET name = $4, updated_at = now() WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
        .bind(actor.tenant_id).bind(project_id).bind(team_id).bind(&name)
        .execute(&mut *transaction).await.map_err(team_write_error)?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    replace_assignments(
        &mut transaction,
        actor.tenant_id,
        team_id,
        &request.member_ids,
        &request.inbox_ids,
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(load_team(&state, &actor, project_id, team_id).await?))
}

async fn delete_team(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(team_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let project_id = require_management(&actor)?;
    let mut transaction = state.db.begin().await?;
    sqlx::query("DELETE FROM inbox_team_access WHERE tenant_id = $1 AND team_id = $2")
        .bind(actor.tenant_id)
        .bind(team_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM team_members WHERE tenant_id = $1 AND team_id = $2")
        .bind(actor.tenant_id)
        .bind(team_id)
        .execute(&mut *transaction)
        .await?;
    let deleted =
        sqlx::query("DELETE FROM teams WHERE tenant_id = $1 AND project_id = $2 AND id = $3")
            .bind(actor.tenant_id)
            .bind(project_id)
            .bind(team_id)
            .execute(&mut *transaction)
            .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn require_management(actor: &ActorContext) -> Result<Uuid, AppError> {
    actor.require("teams:manage")?;
    if actor.has_restricted_inbox_scope() {
        return Err(AppError::Forbidden);
    }
    actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))
}

fn require_profile_management(actor: &ActorContext, operator_id: Uuid) -> Result<Uuid, AppError> {
    if actor.actor_id == operator_id {
        actor.require_password_session()?;
    } else {
        actor.require("teams:manage")?;
    }
    if actor.has_restricted_inbox_scope() {
        return Err(AppError::Forbidden);
    }
    actor
        .project_id
        .ok_or_else(|| AppError::BadRequest("select a project first".to_owned()))
}

#[cfg(test)]
fn require_project_wide_scope(inbox_scope: Option<&[Uuid]>) -> Result<(), AppError> {
    if inbox_scope.is_some() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

fn normalize_name(value: String) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > 200 {
        return Err(AppError::BadRequest(
            "team name must contain 1 to 200 characters".to_owned(),
        ));
    }
    Ok(value)
}

fn normalize_operator_display_name(value: String) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > 200 || value.chars().any(char::is_control) {
        return Err(AppError::BadRequest(
            "operator display name must contain between 1 and 200 printable characters".to_owned(),
        ));
    }
    Ok(value)
}

fn normalize_avatar_url(value: Option<String>) -> Result<(Option<String>, bool), AppError> {
    let Some(value) = value else {
        return Ok((None, true));
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok((None, true));
    }
    if let Some(public_id) = value.strip_prefix("/public/v1/avatars/") {
        Uuid::parse_str(public_id)
            .map_err(|_| AppError::BadRequest("managed avatar URL is malformed".to_owned()))?;
        return Ok((None, false));
    }
    if value.chars().count() > 2_048 {
        return Err(AppError::BadRequest(
            "avatar URL cannot exceed 2048 characters".to_owned(),
        ));
    }
    let parsed = Url::parse(value)
        .map_err(|_| AppError::BadRequest("avatar URL must be an absolute URL".to_owned()))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(AppError::BadRequest(
            "avatar URL must use HTTP or HTTPS and cannot contain credentials".to_owned(),
        ));
    }
    Ok((Some(parsed.to_string()), true))
}

async fn validate_assignments(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project_id: Uuid,
    member_ids: &[Uuid],
    inbox_ids: &[Uuid],
) -> Result<(), AppError> {
    let member_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT user_id) FROM memberships WHERE tenant_id = $1 AND (project_id = $2 OR project_id IS NULL) AND revoked_at IS NULL AND user_id = ANY($3)")
        .bind(actor.tenant_id).bind(project_id).bind(member_ids).fetch_one(&mut **transaction).await?;
    let inbox_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT id) FROM inboxes WHERE tenant_id = $1 AND project_id = $2 AND status = 'active' AND id = ANY($3)")
        .bind(actor.tenant_id).bind(project_id).bind(inbox_ids).fetch_one(&mut **transaction).await?;
    let expected_member_count = i64::try_from(
        member_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
    )
    .map_err(|_| AppError::BadRequest("too many team assignments".to_owned()))?;
    let expected_inbox_count = i64::try_from(
        inbox_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
    )
    .map_err(|_| AppError::BadRequest("too many team assignments".to_owned()))?;
    if member_count != expected_member_count || inbox_count != expected_inbox_count {
        return Err(AppError::BadRequest(
            "team assignments must belong to the active project".to_owned(),
        ));
    }
    Ok(())
}

async fn replace_assignments(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_id: Uuid,
    member_ids: &[Uuid],
    inbox_ids: &[Uuid],
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM team_members WHERE tenant_id = $1 AND team_id = $2")
        .bind(tenant_id)
        .bind(team_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO team_members (id, tenant_id, team_id, user_id) SELECT gen_random_uuid(), $1, $2, value FROM unnest($3::uuid[]) AS value")
        .bind(tenant_id).bind(team_id).bind(member_ids).execute(&mut **transaction).await?;
    sqlx::query("DELETE FROM inbox_team_access WHERE tenant_id = $1 AND team_id = $2")
        .bind(tenant_id)
        .bind(team_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO inbox_team_access (id, tenant_id, inbox_id, team_id, permissions) SELECT gen_random_uuid(), $1, value, $2, ARRAY['read','reply']::text[] FROM unnest($3::uuid[]) AS value")
        .bind(tenant_id).bind(team_id).bind(inbox_ids).execute(&mut **transaction).await?;
    Ok(())
}

async fn load_teams(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
) -> Result<Vec<TeamResponse>, AppError> {
    let rows = sqlx::query_as::<_, TeamRow>(r#"
        SELECT team.id, team.name,
               COALESCE(array_agg(DISTINCT member.user_id) FILTER (WHERE member.user_id IS NOT NULL), ARRAY[]::uuid[]) AS member_ids,
               COALESCE(array_agg(DISTINCT access.inbox_id) FILTER (WHERE access.inbox_id IS NOT NULL), ARRAY[]::uuid[]) AS inbox_ids
        FROM teams AS team
        LEFT JOIN team_members AS member ON member.tenant_id = team.tenant_id AND member.team_id = team.id
        LEFT JOIN inbox_team_access AS access ON access.tenant_id = team.tenant_id AND access.team_id = team.id
        WHERE team.tenant_id = $1 AND team.project_id = $2
        GROUP BY team.id, team.name ORDER BY team.name, team.id
    "#).bind(actor.tenant_id).bind(project_id).fetch_all(&state.db).await?;
    Ok(rows.into_iter().map(TeamResponse::from).collect())
}

async fn load_team(
    state: &AppState,
    actor: &ActorContext,
    project_id: Uuid,
    team_id: Uuid,
) -> Result<TeamResponse, AppError> {
    load_teams(state, actor, project_id)
        .await?
        .into_iter()
        .find(|team| team.id == team_id)
        .ok_or(AppError::NotFound)
}

fn team_write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("a team with this name already exists".to_owned())
    } else {
        AppError::Database(error)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn project_wide_team_and_operator_profiles_reject_inbox_scoped_access() {
        assert!(super::require_project_wide_scope(None).is_ok());
        for scope in [Vec::new(), vec![uuid::Uuid::now_v7()]] {
            assert!(matches!(
                super::require_project_wide_scope(Some(&scope)),
                Err(crate::error::AppError::Forbidden)
            ));
        }
    }

    use super::{normalize_avatar_url, normalize_operator_display_name};

    #[test]
    fn normalizes_public_operator_profile_fields() {
        assert_eq!(
            normalize_operator_display_name(" Anna ".to_owned()).unwrap(),
            "Anna"
        );
        assert_eq!(
            normalize_avatar_url(Some(" https://cdn.example/avatar.png ".to_owned())).unwrap(),
            (Some("https://cdn.example/avatar.png".to_owned()), true),
        );
        assert_eq!(
            normalize_avatar_url(Some("  ".to_owned())).unwrap(),
            (None, true)
        );
        assert_eq!(
            normalize_avatar_url(Some(
                "/public/v1/avatars/00000000-0000-4000-8000-000000000001".to_owned(),
            ))
            .unwrap(),
            (None, false),
        );
        assert!(normalize_operator_display_name("Anna\nAdmin".to_owned()).is_err());
    }

    #[test]
    fn rejects_unsafe_or_relative_avatar_urls() {
        assert!(normalize_avatar_url(Some("/avatar.png".to_owned())).is_err());
        assert!(normalize_avatar_url(Some("javascript:alert(1)".to_owned())).is_err());
        assert!(
            normalize_avatar_url(Some("https://user:secret@example.com/avatar".to_owned()))
                .is_err()
        );
    }
}
