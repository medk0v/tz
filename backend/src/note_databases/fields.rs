use super::{
    CreateField, Database, Field, OrderInput, Scope, UpdateField, VersionQuery, audit,
    bump_database, check_version, get_database, lock_project, model::List, relations,
    require_scope, validate_order, validate_text, views,
};
use crate::{AppState, auth::ActorContext, error::AppError};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get as route_get, patch, put},
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;
use uuid::Uuid;

pub const TYPES: [&str; 11] = [
    "text",
    "long_text",
    "number",
    "date",
    "boolean",
    "single_select",
    "multi_select",
    "url",
    "email",
    "relation",
    "rollup",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/notes/databases/{database_id}/fields",
            route_get(list_route).post(create_route),
        )
        .route(
            "/api/v1/notes/databases/{database_id}/fields/{field_id}",
            patch(update_route).delete(delete_route),
        )
        .route(
            "/api/v1/notes/databases/{database_id}/fields/order",
            put(order_route),
        )
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn list(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
) -> Result<Vec<Field>, AppError> {
    sqlx::query_as("SELECT id,database_id,name,field_type,position,version,config,relation_target_database_id,relation_cardinality,relation_owner_field_id,inverse_field_id,rollup_relation_field_id,rollup_target_field_id,rollup_operation,created_at,updated_at FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 ORDER BY position,id")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).fetch_all(&mut **tx).await.map_err(AppError::from)
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn get(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    field_id: Uuid,
) -> Result<Field, AppError> {
    sqlx::query_as("SELECT id,database_id,name,field_type,position,version,config,relation_target_database_id,relation_cardinality,relation_owner_field_id,inverse_field_id,rollup_relation_field_id,rollup_target_field_id,rollup_operation,created_at,updated_at FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(field_id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)
}

pub fn contains_nul(value: &Value) -> bool {
    match value {
        Value::String(value) => value.contains('\0'),
        Value::Array(values) => values.iter().any(contains_nul),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains('\0') || contains_nul(value)),
        _ => false,
    }
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub fn validate_config(name: &str, field_type: &str, config: &Value) -> Result<(), AppError> {
    validate_text(name.trim(), 1, 160, "field name")?;
    if !TYPES.contains(&field_type) {
        return Err(AppError::BadRequest("unsupported field_type".to_owned()));
    }
    if !config.is_object() || config.to_string().len() > 65_536 || contains_nul(config) {
        return Err(AppError::BadRequest(
            "field config must be an object up to 65536 bytes without null characters".to_owned(),
        ));
    }
    if matches!(field_type, "single_select" | "multi_select") {
        let options = config
            .get("options")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::BadRequest("select fields require config.options".to_owned())
            })?;
        if options.len() > 100 {
            return Err(AppError::BadRequest(
                "select fields allow at most 100 options".to_owned(),
            ));
        }
        let mut seen = HashSet::new();
        for option in options {
            let id = option
                .get("id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| {
                    AppError::BadRequest("each select option requires a UUID id".to_owned())
                })?;
            if !seen.insert(id) {
                return Err(AppError::BadRequest(
                    "select option ids must be unique".to_owned(),
                ));
            }
            let label = option.get("label").and_then(Value::as_str).ok_or_else(|| {
                AppError::BadRequest("each select option requires a label".to_owned())
            })?;
            validate_text(label.trim(), 1, 120, "select option label")?;
            if let Some(color) = option.get("color") {
                let color = color.as_str().ok_or_else(|| {
                    AppError::BadRequest("option color must be a string".to_owned())
                })?;
                validate_text(color, 0, 32, "option color")?;
            }
        }
    }
    Ok(())
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub fn validate_value(field: &Field, value: &Value) -> Result<(), AppError> {
    if field.field_type == "rollup" {
        return Err(AppError::BadRequest(
            "rollup values are computed and cannot be written".to_owned(),
        ));
    }
    if value.is_null() {
        return Ok(());
    }
    let valid = match field.field_type.as_str() {
        "text" | "long_text" => value.as_str().is_some_and(|text| {
            text.chars().count()
                <= if field.field_type == "text" {
                    4000
                } else {
                    20000
                }
                && !text.contains('\0')
        }),
        "number" => value
            .as_f64()
            .is_some_and(|number| number.is_finite() && number.abs() <= 9_007_199_254_740_991.0),
        "boolean" => value.is_boolean(),
        "date" => value.as_str().is_some_and(|text| {
            (text.len() == 10 && chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok())
                || chrono::DateTime::parse_from_rfc3339(text).is_ok()
        }),
        "url" => value.as_str().is_some_and(|text| {
            text.len() <= 2048
                && url::Url::parse(text).is_ok_and(|url| {
                    matches!(url.scheme(), "http" | "https")
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                })
        }),
        "email" => value.as_str().is_some_and(|text| {
            let parts = text.split('@').collect::<Vec<_>>();
            text.len() <= 254
                && !text.chars().any(char::is_whitespace)
                && parts.len() == 2
                && !parts[0].is_empty()
                && parts[1].contains('.')
                && !parts[1].starts_with('.')
                && !parts[1].ends_with('.')
        }),
        "single_select" => select_id(field, value),
        "multi_select" => value.as_array().is_some_and(|values| {
            values.len() <= 100
                && values.iter().all(|value| select_id(field, value))
                && values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<HashSet<_>>()
                    .len()
                    == values.len()
        }),
        "relation" => value.as_array().is_some_and(|values| {
            values.len() <= 100
                && values.iter().all(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| Uuid::parse_str(value).is_ok())
                })
                && values
                    .iter()
                    .filter_map(|value| {
                        value.as_str().and_then(|value| Uuid::parse_str(value).ok())
                    })
                    .collect::<HashSet<_>>()
                    .len()
                    == values.len()
        }),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "value does not match field {} ({})",
            field.id, field.field_type
        )))
    }
}

fn select_id(field: &Field, value: &Value) -> bool {
    let Some(id) = value.as_str().and_then(|text| Uuid::parse_str(text).ok()) else {
        return false;
    };
    field
        .config
        .get("options")
        .and_then(Value::as_array)
        .is_some_and(|options| {
            options.iter().any(|option| {
                option
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|text| Uuid::parse_str(text).ok())
                    == Some(id)
            })
        })
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn create(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    mut input: CreateField,
    actor_id: Option<Uuid>,
) -> Result<Field, AppError> {
    get_database(tx, scope, database_id).await?;
    input.name = input.name.trim().to_owned();
    validate_config(&input.name, &input.field_type, &input.config)?;
    if (input.field_type == "relation") != input.relation.is_some()
        || (input.field_type == "rollup") != input.rollup.is_some()
    {
        return Err(AppError::BadRequest(
            "relation and rollup configuration must match field_type".to_owned(),
        ));
    }
    let existing = list(tx, scope, database_id).await?;
    if existing.len() >= 100 {
        return Err(AppError::Conflict(
            "a database can contain at most 100 fields".to_owned(),
        ));
    }
    if let Some(relation) = &input.relation {
        get_database(tx, scope, relation.target_database_id).await?;
    }
    if let Some(rollup) = &input.rollup {
        let relation = get(tx, scope, database_id, rollup.relation_field_id).await?;
        if relation.field_type != "relation" {
            return Err(AppError::BadRequest(
                "rollup relation_field_id must refer to a relation field".to_owned(),
            ));
        }
        if let Some(target) = rollup.target_field_id {
            get(
                tx,
                scope,
                relation
                    .relation_target_database_id
                    .ok_or(AppError::NotFound)?,
                target,
            )
            .await?;
        }
    }
    let position = existing.last().map_or(0, |field| field.position + 1);
    let row=sqlx::query_as::<_,Field>("INSERT INTO note_database_fields(id,tenant_id,project_id,database_id,name,field_type,position,config,relation_target_database_id,relation_cardinality,rollup_relation_field_id,rollup_target_field_id,rollup_operation) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING id,database_id,name,field_type,position,version,config,relation_target_database_id,relation_cardinality,relation_owner_field_id,inverse_field_id,rollup_relation_field_id,rollup_target_field_id,rollup_operation,created_at,updated_at")
        .bind(Uuid::now_v7()).bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(&input.name).bind(&input.field_type).bind(position).bind(&input.config)
        .bind(input.relation.as_ref().map(|value|value.target_database_id)).bind(input.relation.as_ref().map(|value|value.cardinality.as_str()))
        .bind(input.rollup.as_ref().map(|value|value.relation_field_id)).bind(input.rollup.as_ref().and_then(|value|value.target_field_id)).bind(input.rollup.as_ref().map(|value|value.operation.as_str()))
        .fetch_one(&mut **tx).await.map_err(write_error)?;
    let row = relations::initialize_field(tx, scope, &row, &input).await?;
    bump_database(tx, scope, database_id).await?;
    audit(
        tx,
        scope,
        actor_id,
        "note_database_field",
        row.id,
        "note_database_field.created",
    )
    .await?;
    Ok(row)
}

/// # Errors
/// Returns validation, scope, version-conflict, or storage errors.
/// Mutating callers must hold the project lock for the whole transaction.
pub async fn update(
    tx: &mut Transaction<'_, Postgres>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
    mut input: UpdateField,
    actor_id: Option<Uuid>,
) -> Result<Field, AppError> {
    let current = get(tx, scope, database_id, id).await?;
    check_version(current.version, input.expected_version)?;
    input.name = input.name.trim().to_owned();
    validate_config(&input.name, &input.field_type, &input.config)?;
    if input.field_type != current.field_type
        && (matches!(input.field_type.as_str(), "relation" | "rollup")
            || matches!(current.field_type.as_str(), "relation" | "rollup"))
    {
        return Err(AppError::BadRequest(
            "relation and rollup field types cannot be converted".to_owned(),
        ));
    }
    relations::validate_field_update(tx, scope, &current, &input.field_type).await?;
    let mut proposed = current.clone();
    proposed.name = input.name;
    proposed.field_type = input.field_type;
    proposed.config = input.config;
    if proposed.field_type != "relation" && proposed.field_type != "rollup" {
        let values=sqlx::query_scalar::<_,Value>("SELECT field_values->$4 FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND field_values ? $4")
            .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id.to_string()).fetch_all(&mut **tx).await?;
        for value in values {
            if validate_value(&proposed, &value).is_err() {
                return Err(AppError::Conflict(
                    "existing record values do not fit the requested field type or options"
                        .to_owned(),
                ));
            }
        }
    }
    let mut fields = list(tx, scope, database_id).await?;
    for field in &mut fields {
        if field.id == id {
            *field = proposed.clone();
        }
    }
    for view in views::fetch_views(tx, scope, database_id).await? {
        views::validate_config(&fields, &view.config)?;
    }
    sqlx::query("UPDATE note_database_fields SET name=$5,field_type=$6,config=$7,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).bind(proposed.name).bind(proposed.field_type).bind(proposed.config).execute(&mut **tx).await?;
    bump_database(tx, scope, database_id).await?;
    audit(
        tx,
        scope,
        actor_id,
        "note_database_field",
        id,
        "note_database_field.updated",
    )
    .await?;
    get(tx, scope, database_id, id).await
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
    let current = get(tx, scope, database_id, id).await?;
    check_version(current.version, expected_version)?;
    relations::delete_field(tx, scope, &current).await?;
    sqlx::query("UPDATE note_database_records SET field_values=field_values-$4,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND field_values ? $4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id.to_string()).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4").bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).execute(&mut **tx).await.map_err(write_error)?;
    bump_database(tx, scope, database_id).await?;
    audit(
        tx,
        scope,
        actor_id,
        "note_database_field",
        id,
        "note_database_field.deleted",
    )
    .await
}

pub fn write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation)
    {
        AppError::Conflict("field is still referenced by another field or view".to_owned())
    } else if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_check_violation)
    {
        AppError::BadRequest("invalid field configuration".to_owned())
    } else {
        AppError::Database(error)
    }
}

async fn list_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(db): Path<Uuid>,
) -> Result<Json<List<Field>>, AppError> {
    let scope = require_scope(&actor, "notes:read")?;
    let mut tx = state.db.begin().await?;
    get_database(&mut tx, &scope, db).await?;
    let items = list(&mut tx, &scope, db).await?;
    tx.commit().await?;
    Ok(Json(List { items }))
}
async fn create_route(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(db): Path<Uuid>,
    Json(input): Json<CreateField>,
) -> Result<(StatusCode, Json<Field>), AppError> {
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
    Json(input): Json<UpdateField>,
) -> Result<Json<Field>, AppError> {
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
    Query(input): Query<VersionQuery>,
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
    validate_order(
        &list(&mut tx, &scope, db)
            .await?
            .iter()
            .map(|field| field.id)
            .collect::<Vec<_>>(),
        &input.ids,
    )?;
    sqlx::query("UPDATE note_database_fields AS field SET position=ordering.position::integer-1,version=field.version+1,updated_at=clock_timestamp() FROM unnest($4::uuid[]) WITH ORDINALITY AS ordering(id,position) WHERE field.tenant_id=$1 AND field.project_id=$2 AND field.database_id=$3 AND field.id=ordering.id AND field.position<>ordering.position-1")
        .bind(scope.tenant_id).bind(scope.project_id).bind(db).bind(input.ids).execute(&mut *tx).await?;
    let row = bump_database(&mut tx, &scope, db).await?;
    audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database",
        db,
        "note_database.fields_reordered",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(row))
}
