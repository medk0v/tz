use super::{
    CreateRecord, Database, Field, OrderInput, Record, Scope, UpdateRecord, VersionQuery, audit,
    bump_database, check_version, fields, get_database, lock_project, relations, require_scope,
    validate_order, validate_text, views,
};
use crate::{AppState, auth::ActorContext, error::AppError};
use axum::{
    Json, Router,
    extract::{Path, Query as QueryParams, State},
    http::StatusCode,
    routing::{get as route_get, put},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub view_id: Option<Uuid>,
    pub q: String,
    pub page: usize,
    pub per_page: usize,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            view_id: None,
            q: String::new(),
            page: 1,
            per_page: 200,
        }
    }
}

#[derive(Serialize)]
pub struct List {
    pub items: Vec<Record>,
    pub total: usize,
    pub page: usize,
    pub per_page: usize,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/notes/databases/{database_id}/records",
            route_get(list_route).post(create_route),
        )
        .route(
            "/api/v1/notes/databases/{database_id}/records/{record_id}",
            route_get(get_route)
                .patch(update_route)
                .delete(delete_route),
        )
        .route(
            "/api/v1/notes/databases/{database_id}/records/order",
            put(order_route),
        )
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
#[allow(clippy::implicit_hasher)] // Application scopes use the standard randomized UUID set.
pub async fn list(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    query: &Query,
    allowed_fields: Option<&HashSet<Uuid>>,
) -> Result<List, AppError> {
    get_database(tx, scope, database_id).await?;
    validate_text(query.q.trim(), 0, 200, "query")?;
    if query.page < 1 || query.page > 5000 || !(1..=500).contains(&query.per_page) {
        return Err(AppError::BadRequest(
            "page must be 1 to 5000 and per_page 1 to 500".to_owned(),
        ));
    }
    let mut fields = fields::list(tx, scope, database_id).await?;
    if let Some(allowed) = allowed_fields {
        fields.retain(|field| allowed.contains(&field.id));
    }
    let mut rows=sqlx::query_as::<_,Record>("SELECT id,database_id,position,version,field_values AS values,NULL::text AS content_markdown,created_at,updated_at FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 ORDER BY position,id LIMIT 5000")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).fetch_all(&mut **tx).await?;
    if let Some(allowed) = allowed_fields {
        for row in &mut rows {
            if let Some(values) = row.values.as_object_mut() {
                values.retain(|key, _| Uuid::parse_str(key).is_ok_and(|id| allowed.contains(&id)));
            }
        }
    }
    relations::project_records(tx, scope, database_id, &fields, &mut rows).await?;
    if let Some(view_id) = query.view_id {
        let mut view = views::fetch_view(tx, scope, database_id, view_id).await?;
        if let Some(allowed) = allowed_fields {
            project_view(&mut view.config, allowed);
        }
        views::apply_view(&mut rows, &view.config)?;
    }
    let query_text = query.q.trim().to_lowercase();
    if !query_text.is_empty() {
        rows.retain(|row| {
            row.values.as_object().is_some_and(|values| {
                values
                    .values()
                    .any(|value| value.to_string().to_lowercase().contains(&query_text))
            })
        });
    }
    let total = rows.len();
    let items = rows
        .into_iter()
        .skip((query.page - 1) * query.per_page)
        .take(query.per_page)
        .collect();
    Ok(List {
        items,
        total,
        page: query.page,
        per_page: query.per_page,
    })
}

fn project_view(config: &mut Value, allowed: &HashSet<Uuid>) {
    for key in ["filters", "sorts"] {
        if let Some(items) = config.get_mut(key).and_then(Value::as_array_mut) {
            items.retain(|item| {
                item.get("field_id")
                    .and_then(Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .is_some_and(|id| allowed.contains(&id))
            });
        }
    }
    for key in ["hidden_fields", "column_order", "pinned_fields"] {
        if let Some(items) = config.get_mut(key).and_then(Value::as_array_mut) {
            items.retain(|item| {
                item.as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .is_some_and(|id| allowed.contains(&id))
            });
        }
    }
    if let Some(widths) = config
        .get_mut("column_widths")
        .and_then(Value::as_object_mut)
    {
        widths.retain(|id, _| Uuid::parse_str(id).is_ok_and(|id| allowed.contains(&id)));
    }
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn get(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
) -> Result<Record, AppError> {
    get_projected(tx, scope, database_id, id, None).await
}

/// # Errors
/// Returns scope, projection, or storage errors. Hidden rollups are never evaluated.
#[allow(clippy::implicit_hasher)] // Application scopes use the standard randomized UUID set.
pub async fn get_projected(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
    allowed_fields: Option<&HashSet<Uuid>>,
) -> Result<Record, AppError> {
    let mut row=sqlx::query_as::<_,Record>("SELECT id,database_id,position,version,field_values AS values,content_markdown,created_at,updated_at FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)?;
    let mut fields = fields::list(tx, scope, database_id).await?;
    if let Some(allowed) = allowed_fields {
        fields.retain(|field| allowed.contains(&field.id));
        if let Some(values) = row.values.as_object_mut() {
            values.retain(|key, _| Uuid::parse_str(key).is_ok_and(|id| allowed.contains(&id)));
        }
    }
    let mut rows = vec![row];
    relations::project_records(tx, scope, database_id, &fields, &mut rows).await?;
    rows.pop().ok_or(AppError::NotFound)
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub fn validate_values(fields: &[Field], values: &Value) -> Result<(), AppError> {
    let object = values.as_object().ok_or_else(|| {
        AppError::BadRequest("values must be an object keyed by field UUID".to_owned())
    })?;
    if object.len() > 100 || values.to_string().len() > 65_536 || fields::contains_nul(values) {
        return Err(AppError::BadRequest(
            "record values exceed 100 fields or 65536 bytes, or contain null characters".to_owned(),
        ));
    }
    let fields = fields
        .iter()
        .map(|field| (field.id, field))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    for (key, value) in object {
        let id = Uuid::parse_str(key).map_err(|_| {
            AppError::BadRequest("record value keys must be field UUIDs".to_owned())
        })?;
        if !seen.insert(id) || id.to_string() != *key {
            return Err(AppError::BadRequest(
                "record value keys must be unique canonical field UUIDs".to_owned(),
            ));
        }
        let field = fields.get(&id).ok_or_else(|| {
            AppError::BadRequest("record values include an unknown field".to_owned())
        })?;
        fields::validate_value(field, value)?;
    }
    Ok(())
}

fn validate_content(content: &str) -> Result<(), AppError> {
    validate_text(content, 0, 200_000, "content_markdown")?;
    if content.len() > 800_000 {
        return Err(AppError::BadRequest(
            "content_markdown exceeds 800000 UTF-8 bytes".to_owned(),
        ));
    }
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn create(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    input: CreateRecord,
    actor_id: Option<Uuid>,
) -> Result<Record, AppError> {
    create_projected(tx, scope, database_id, input, actor_id, None).await
}

/// # Errors
/// Returns validation, scope, or storage errors. The caller must hold the project lock.
#[allow(clippy::implicit_hasher)] // Application scopes use the standard randomized UUID set.
pub async fn create_projected(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    mut input: CreateRecord,
    actor_id: Option<Uuid>,
    allowed_fields: Option<&HashSet<Uuid>>,
) -> Result<Record, AppError> {
    get_database(tx, scope, database_id).await?;
    validate_content(&input.content_markdown)?;
    let fields = fields::list(tx, scope, database_id).await?;
    validate_values(&fields, &input.values)?;
    let (count,position):(i64,i32)=sqlx::query_as("SELECT count(*),COALESCE(max(position)+1,0) FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).fetch_one(&mut **tx).await?;
    if count >= 5000 {
        return Err(AppError::Conflict(
            "a database can contain at most 5000 records".to_owned(),
        ));
    }
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO note_database_records(id,tenant_id,project_id,database_id,position,content_markdown) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id).bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(position).bind(input.content_markdown).execute(&mut **tx).await?;
    relations::replace_record_relations(tx, scope, &fields, id, &mut input.values).await?;
    sqlx::query("UPDATE note_database_records SET field_values=$5 WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).bind(input.values).execute(&mut **tx).await?;
    bump_database(tx, scope, database_id).await?;
    audit(
        tx,
        scope,
        actor_id,
        "note_database_record",
        id,
        "note_database_record.created",
    )
    .await?;
    get_projected(tx, scope, database_id, id, allowed_fields).await
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn update(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
    input: UpdateRecord,
    actor_id: Option<Uuid>,
) -> Result<Record, AppError> {
    update_projected(tx, scope, database_id, id, input, actor_id, None).await
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors under the caller's project lock.
#[allow(clippy::implicit_hasher)] // Application scopes use the standard randomized UUID set.
pub async fn update_projected(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
    mut input: UpdateRecord,
    actor_id: Option<Uuid>,
    allowed_fields: Option<&HashSet<Uuid>>,
) -> Result<Record, AppError> {
    check_record_version(tx, scope, database_id, id, input.expected_version).await?;
    validate_content(&input.content_markdown)?;
    let fields = fields::list(tx, scope, database_id).await?;
    validate_values(&fields, &input.values)?;
    relations::replace_record_relations(tx, scope, &fields, id, &mut input.values).await?;
    sqlx::query("UPDATE note_database_records SET field_values=$5,content_markdown=$6,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).bind(input.values).bind(input.content_markdown).execute(&mut **tx).await?;
    bump_database(tx, scope, database_id).await?;
    audit(
        tx,
        scope,
        actor_id,
        "note_database_record",
        id,
        "note_database_record.updated",
    )
    .await?;
    get_projected(tx, scope, database_id, id, allowed_fields).await
}

async fn check_record_version(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
    expected: i32,
) -> Result<(), AppError> {
    let current = sqlx::query_scalar::<_, i32>("SELECT version FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)?;
    check_version(current, expected)
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn delete(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
    expected_version: i32,
    actor_id: Option<Uuid>,
) -> Result<(), AppError> {
    check_record_version(tx, scope, database_id, id, expected_version).await?;
    // Inverse values change when a row is removed, so invalidate related row snapshots first.
    let related:Vec<(Uuid,Uuid)>=sqlx::query_as("SELECT DISTINCT CASE WHEN source_record_id=$3 THEN target_database_id ELSE source_database_id END,CASE WHEN source_record_id=$3 THEN target_record_id ELSE source_record_id END FROM note_record_relations WHERE tenant_id=$1 AND project_id=$2 AND (source_record_id=$3 OR target_record_id=$3)")
        .bind(scope.tenant_id).bind(scope.project_id).bind(id).fetch_all(&mut **tx).await?;
    let mut databases = HashSet::new();
    for (db, record) in related {
        sqlx::query("UPDATE note_database_records SET version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(db).bind(record).execute(&mut **tx).await?;
        databases.insert(db);
    }
    databases.insert(database_id);
    sqlx::query("DELETE FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4").bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).execute(&mut **tx).await?;
    for db in databases {
        bump_database(tx, scope, db).await?;
    }
    audit(
        tx,
        scope,
        actor_id,
        "note_database_record",
        id,
        "note_database_record.deleted",
    )
    .await
}

async fn list_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(db): Path<Uuid>,
    QueryParams(query): QueryParams<Query>,
) -> Result<Json<List>, AppError> {
    let scope = require_scope(&actor, "notes:read")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let rows = list(&mut tx, &scope, db, &query, None).await?;
    tx.commit().await?;
    Ok(Json(rows))
}
async fn get_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((db, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Record>, AppError> {
    let scope = require_scope(&actor, "notes:read")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let row = get(&mut tx, &scope, db, id).await?;
    tx.commit().await?;
    Ok(Json(row))
}
async fn create_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(db): Path<Uuid>,
    Json(input): Json<CreateRecord>,
) -> Result<(StatusCode, Json<Record>), AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let row = create(&mut tx, &scope, db, input, Some(actor.actor_id)).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}
async fn update_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((db, id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateRecord>,
) -> Result<Json<Record>, AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    let row = update(&mut tx, &scope, db, id, input, Some(actor.actor_id)).await?;
    tx.commit().await?;
    Ok(Json(row))
}
async fn delete_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((db, id)): Path<(Uuid, Uuid)>,
    QueryParams(input): QueryParams<VersionQuery>,
) -> Result<StatusCode, AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    delete(
        &mut tx,
        &scope,
        db,
        id,
        input.expected_version,
        Some(actor.actor_id),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn order_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(db): Path<Uuid>,
    Json(input): Json<OrderInput>,
) -> Result<Json<Database>, AppError> {
    let scope = require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    lock_project(&mut tx, &scope).await?;
    check_version(
        get_database(&mut tx, &scope, db).await?.version,
        input.expected_version,
    )?;
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3").bind(scope.tenant_id).bind(scope.project_id).bind(db).fetch_all(&mut *tx).await?;
    validate_order(&ids, &input.ids)?;
    sqlx::query("UPDATE note_database_records AS record SET position=ordering.position::integer-1,version=record.version+1,updated_at=clock_timestamp() FROM unnest($4::uuid[]) WITH ORDINALITY AS ordering(id,position) WHERE record.tenant_id=$1 AND record.project_id=$2 AND record.database_id=$3 AND record.id=ordering.id AND record.position<>ordering.position-1")
        .bind(scope.tenant_id).bind(scope.project_id).bind(db).bind(input.ids).execute(&mut *tx).await?;
    let row = bump_database(&mut tx, &scope, db).await?;
    audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database",
        db,
        "note_database.records_reordered",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(row))
}
