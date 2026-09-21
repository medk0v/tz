//! Saved table layouts and validated, deterministic record filtering and sorting.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, put},
};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::{
    Database, Field, List, OrderInput, Record, Scope, VersionQuery, View, empty_object,
};
use crate::{AppState, auth::ActorContext, error::AppError};

type Tx<'a> = Transaction<'a, Postgres>;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/notes/databases/{database_id}/views",
            get(list).post(create),
        )
        .route(
            "/api/v1/notes/databases/{database_id}/views/order",
            put(reorder),
        )
        .route(
            "/api/v1/notes/databases/{database_id}/views/{view_id}",
            get(read).patch(update).delete(delete),
        )
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewConfig {
    pub filters: Vec<ViewFilter>,
    pub sorts: Vec<ViewSort>,
    pub hidden_fields: Vec<Uuid>,
    pub column_order: Vec<Uuid>,
    pub column_widths: BTreeMap<Uuid, u16>,
    pub pinned_fields: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewFilter {
    pub field_id: Uuid,
    pub operator: String,
    #[serde(default)]
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSort {
    pub field_id: Uuid,
    pub direction: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateView {
    name: String,
    #[serde(default = "empty_object")]
    config: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateView {
    name: String,
    config: Value,
    expected_version: i32,
}

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.to_owned())
}

fn name(value: &str) -> Result<&str, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 160 || value.contains('\0') {
        return Err(invalid(
            "view name must contain 1 to 160 characters without NUL",
        ));
    }
    Ok(value)
}

/// List saved views in their manual order.
///
/// # Errors
/// Returns database failures.
pub async fn fetch_views(
    tx: &mut Tx<'_>,
    scope: &Scope,
    database_id: Uuid,
) -> Result<Vec<View>, AppError> {
    Ok(sqlx::query_as("SELECT * FROM note_database_views WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 ORDER BY position,id")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).fetch_all(&mut **tx).await?)
}

/// Fetch a view within the specified project and database.
///
/// # Errors
/// Returns not found outside the requested scope, or a database failure.
pub async fn fetch_view(
    tx: &mut Tx<'_>,
    scope: &Scope,
    database_id: Uuid,
    id: Uuid,
) -> Result<View, AppError> {
    sqlx::query_as("SELECT * FROM note_database_views WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)
}

async fn fields(tx: &mut Tx<'_>, scope: &Scope, database_id: Uuid) -> Result<Vec<Field>, AppError> {
    Ok(sqlx::query_as("SELECT * FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).fetch_all(&mut **tx).await?)
}

async fn list(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(database_id): Path<Uuid>,
) -> Result<Json<List<View>>, AppError> {
    let scope = super::require_scope(&actor, "notes:read")?;
    let mut tx = state.db.begin().await?;
    super::get_database(&mut tx, &scope, database_id).await?;
    let items = fetch_views(&mut tx, &scope, database_id).await?;
    tx.commit().await?;
    Ok(Json(List { items }))
}

async fn read(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((database_id, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<View>, AppError> {
    let scope = super::require_scope(&actor, "notes:read")?;
    let mut tx = state.db.begin().await?;
    let view = fetch_view(&mut tx, &scope, database_id, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn create(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(database_id): Path<Uuid>,
    Json(input): Json<CreateView>,
) -> Result<(StatusCode, Json<View>), AppError> {
    let scope = super::require_scope(&actor, "notes:write")?;
    let name = name(&input.name)?;
    let mut tx = state.db.begin().await?;
    super::lock_project(&mut tx, &scope).await?;
    super::get_database(&mut tx, &scope, database_id).await?;
    let config = validate_config(&fields(&mut tx, &scope, database_id).await?, &input.config)?;
    let existing = fetch_views(&mut tx, &scope, database_id).await?;
    if existing.len() >= 100 {
        return Err(invalid("a database can have at most 100 saved views"));
    }
    let position = existing.last().map_or(0, |view| view.position + 1);
    let view: View = sqlx::query_as("INSERT INTO note_database_views (id,tenant_id,project_id,database_id,name,position,config) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING *")
        .bind(Uuid::now_v7()).bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(name).bind(position)
        .bind(serde_json::to_value(config).map_err(AppError::internal)?).fetch_one(&mut *tx).await?;
    super::bump_database(&mut tx, &scope, database_id).await?;
    super::audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database_view",
        view.id,
        "note_database_view.created",
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn update(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((database_id, id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateView>,
) -> Result<Json<View>, AppError> {
    let scope = super::require_scope(&actor, "notes:write")?;
    let name = name(&input.name)?;
    let mut tx = state.db.begin().await?;
    super::lock_project(&mut tx, &scope).await?;
    let current = fetch_view(&mut tx, &scope, database_id, id).await?;
    super::check_version(current.version, input.expected_version)?;
    let config = validate_config(&fields(&mut tx, &scope, database_id).await?, &input.config)?;
    let view = sqlx::query_as("UPDATE note_database_views SET name=$5,config=$6,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4 RETURNING *")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).bind(name)
        .bind(serde_json::to_value(config).map_err(AppError::internal)?).fetch_one(&mut *tx).await?;
    super::bump_database(&mut tx, &scope, database_id).await?;
    super::audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database_view",
        id,
        "note_database_view.updated",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn delete(
    State(state): State<AppState>,
    actor: ActorContext,
    Path((database_id, id)): Path<(Uuid, Uuid)>,
    Query(input): Query<VersionQuery>,
) -> Result<StatusCode, AppError> {
    let scope = super::require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    super::lock_project(&mut tx, &scope).await?;
    let current = fetch_view(&mut tx, &scope, database_id, id).await?;
    super::check_version(current.version, input.expected_version)?;
    let embedded: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM note_database_embeds WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND view_id=$4)")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).fetch_one(&mut *tx).await?;
    if embedded {
        return Err(AppError::Conflict(
            "remove or change embeds that use this view first".to_owned(),
        ));
    }
    sqlx::query("DELETE FROM note_database_views WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4")
        .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).execute(&mut *tx).await?;
    super::bump_database(&mut tx, &scope, database_id).await?;
    super::audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database_view",
        id,
        "note_database_view.deleted",
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reorder(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(database_id): Path<Uuid>,
    Json(input): Json<OrderInput>,
) -> Result<Json<Database>, AppError> {
    let scope = super::require_scope(&actor, "notes:write")?;
    let mut tx = state.db.begin().await?;
    super::lock_project(&mut tx, &scope).await?;
    let database = super::get_database(&mut tx, &scope, database_id).await?;
    super::check_version(database.version, input.expected_version)?;
    let existing = fetch_views(&mut tx, &scope, database_id).await?;
    let requested: HashSet<_> = input.ids.iter().copied().collect();
    if requested.len() != input.ids.len()
        || requested != existing.iter().map(|view| view.id).collect()
    {
        return Err(invalid(
            "view order must contain each database view exactly once",
        ));
    }
    for (position, id) in input.ids.iter().enumerate() {
        let position = i32::try_from(position).map_err(AppError::internal)?;
        sqlx::query("UPDATE note_database_views SET position=$5,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=$4 AND position<>$5")
            .bind(scope.tenant_id).bind(scope.project_id).bind(database_id).bind(id).bind(position).execute(&mut *tx).await?;
    }
    let database = super::bump_database(&mut tx, &scope, database_id).await?;
    super::audit(
        &mut tx,
        &scope,
        Some(actor.actor_id),
        "note_database",
        database_id,
        "note_database.views_reordered",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(database))
}

/// Field deletion must not leave saved layouts pointing at missing fields.
///
/// # Errors
/// Returns invalid stored configuration or database failures.
pub async fn remove_field_references(
    tx: &mut Tx<'_>,
    scope: &Scope,
    ids: &[Uuid],
) -> Result<(), AppError> {
    let all: Vec<View> =
        sqlx::query_as("SELECT * FROM note_database_views WHERE tenant_id=$1 AND project_id=$2")
            .bind(scope.tenant_id)
            .bind(scope.project_id)
            .fetch_all(&mut **tx)
            .await?;
    for view in all {
        let mut config: ViewConfig = serde_json::from_value(view.config.clone())
            .map_err(|_| invalid("stored view config is invalid"))?;
        config
            .filters
            .retain(|filter| !ids.contains(&filter.field_id));
        config.sorts.retain(|sort| !ids.contains(&sort.field_id));
        config.hidden_fields.retain(|id| !ids.contains(id));
        config.column_order.retain(|id| !ids.contains(id));
        config.pinned_fields.retain(|id| !ids.contains(id));
        config.column_widths.retain(|id, _| !ids.contains(id));
        let updated = serde_json::to_value(config).map_err(AppError::internal)?;
        if updated != view.config {
            sqlx::query("UPDATE note_database_views SET config=$4,version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
                .bind(scope.tenant_id).bind(scope.project_id).bind(view.id).bind(updated).execute(&mut **tx).await?;
        }
    }
    Ok(())
}

/// Validate field references, operators, typed values and layout limits.
///
/// # Errors
/// Rejects malformed, oversized, out-of-database or incompatible configuration.
pub fn validate_config(fields: &[Field], value: &Value) -> Result<ViewConfig, AppError> {
    if super::fields::contains_nul(value)
        || serde_json::to_vec(value).map_or(true, |value| value.len() > 65_536)
    {
        return Err(invalid("view config exceeds 65536 bytes"));
    }
    let config: ViewConfig =
        serde_json::from_value(value.clone()).map_err(|_| invalid("invalid view configuration"))?;
    if config.filters.len() > 50 || config.sorts.len() > 5 {
        return Err(invalid(
            "a view supports at most 50 filters and 5 sort fields",
        ));
    }
    let by_id: HashMap<_, _> = fields.iter().map(|field| (field.id, field)).collect();
    let referenced = config
        .filters
        .iter()
        .map(|filter| filter.field_id)
        .chain(config.sorts.iter().map(|sort| sort.field_id))
        .chain(config.hidden_fields.iter().copied())
        .chain(config.column_order.iter().copied())
        .chain(config.column_widths.keys().copied())
        .chain(config.pinned_fields.iter().copied());
    if referenced.into_iter().any(|id| !by_id.contains_key(&id)) {
        return Err(invalid(
            "view configuration refers to a field outside its database",
        ));
    }
    for ids in [
        &config.hidden_fields,
        &config.column_order,
        &config.pinned_fields,
    ] {
        if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
            return Err(invalid("view layout fields must not repeat"));
        }
    }
    if config
        .column_widths
        .values()
        .any(|width| !(64..=1000).contains(width))
    {
        return Err(invalid("column width must be between 64 and 1000 pixels"));
    }
    let mut sorted = HashSet::new();
    if config.sorts.iter().any(|sort| {
        !matches!(sort.direction.as_str(), "asc" | "desc") || !sorted.insert(sort.field_id)
    }) {
        return Err(invalid(
            "sorts require distinct fields and asc or desc direction",
        ));
    }
    for filter in &config.filters {
        let field = by_id[&filter.field_id];
        if !filter_valid(field, filter) {
            return Err(invalid(
                "filter operator or value is incompatible with its field",
            ));
        }
    }
    Ok(config)
}

fn filter_valid(field: &Field, filter: &ViewFilter) -> bool {
    let kind = field.field_type.as_str();
    match filter.operator.as_str() {
        "is_empty" | "is_not_empty" => true,
        "contains" => match kind {
            "text" | "long_text" | "url" | "email" => filter
                .value
                .as_str()
                .is_some_and(|value| !value.contains('\0')),
            "single_select" | "multi_select" => filter.value.as_str().is_some_and(|value| {
                field
                    .config
                    .get("options")
                    .and_then(Value::as_array)
                    .is_some_and(|options| {
                        options
                            .iter()
                            .any(|option| option.get("id").and_then(Value::as_str) == Some(value))
                    })
            }),
            "relation" => filter
                .value
                .as_str()
                .is_some_and(|value| Uuid::parse_str(value).is_ok()),
            _ => false,
        },
        "equals" | "not_equals" => match kind {
            "relation" => filter.value.as_array().is_some_and(|values| {
                values.len() <= 100
                    && values.iter().all(|value| {
                        value
                            .as_str()
                            .is_some_and(|value| Uuid::parse_str(value).is_ok())
                    })
            }),
            "rollup" => !filter.value.is_null(),
            _ => super::fields::validate_value(field, &filter.value).is_ok(),
        },
        "greater_than" | "less_than" => {
            !filter.value.is_null()
                && match kind {
                    "number" | "date" => {
                        super::fields::validate_value(field, &filter.value).is_ok()
                    }
                    "rollup" => filter.value.is_number() || parse_temporal(&filter.value).is_some(),
                    _ => false,
                }
        }
        _ => false,
    }
}

/// Apply a previously validated configuration to projected records.
///
/// # Errors
/// Rejects malformed configuration instead of silently dropping its filters.
pub fn apply_view(records: &mut Vec<Record>, value: &Value) -> Result<(), AppError> {
    let config: ViewConfig =
        serde_json::from_value(value.clone()).map_err(|_| invalid("invalid view configuration"))?;
    records.retain(|record| {
        config
            .filters
            .iter()
            .all(|filter| filter_matches(record.values.get(filter.field_id.to_string()), filter))
    });
    if !config.sorts.is_empty() {
        sort_records(records, &config.sorts);
    }
    Ok(())
}

pub fn sort_records(records: &mut [Record], sorts: &[ViewSort]) {
    records.sort_by(|left, right| {
        for sort in sorts {
            let order = compare_value(
                left.values
                    .get(sort.field_id.to_string())
                    .unwrap_or(&Value::Null),
                right
                    .values
                    .get(sort.field_id.to_string())
                    .unwrap_or(&Value::Null),
            );
            let order = if sort.direction == "desc" {
                order.reverse()
            } else {
                order
            };
            if !order.is_eq() {
                return order;
            }
        }
        left.position
            .cmp(&right.position)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn empty(value: &Value) -> bool {
    value.is_null()
        || value.as_str().is_some_and(str::is_empty)
        || value.as_array().is_some_and(Vec::is_empty)
}

fn filter_matches(value: Option<&Value>, filter: &ViewFilter) -> bool {
    let value = value.unwrap_or(&Value::Null);
    match filter.operator.as_str() {
        "is_empty" => empty(value),
        "is_not_empty" => !empty(value),
        "equals" => value == &filter.value,
        "not_equals" => value != &filter.value,
        "contains" => contains(value, &filter.value),
        "greater_than" => !empty(value) && compare_value(value, &filter.value).is_gt(),
        "less_than" => !empty(value) && compare_value(value, &filter.value).is_lt(),
        _ => false,
    }
}

fn contains(value: &Value, needle: &Value) -> bool {
    if let (Some(value), Some(needle)) = (value.as_str(), needle.as_str()) {
        return value.to_lowercase().contains(&needle.to_lowercase());
    }
    value.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item == needle || contains(item, needle))
    })
}

pub fn compare_value(left: &Value, right: &Value) -> Ordering {
    // A text column can mix timestamps and arbitrary strings. A fixed type rank
    // keeps their comparison transitive when timezone order differs from text order.
    let left_date = parse_temporal(left);
    let right_date = parse_temporal(right);
    let rank = |value: &Value, date: Option<i64>| match value {
        Value::Number(_) => 0,
        Value::String(_) if date.is_some() => 1,
        Value::String(_) => 2,
        Value::Bool(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
        Value::Null => 6,
    };
    let order = rank(left, left_date).cmp(&rank(right, right_date));
    if !order.is_eq() {
        return order;
    }
    match (left.as_f64(), right.as_f64(), left_date, right_date) {
        (Some(left), Some(right), _, _) => left.total_cmp(&right),
        (_, _, Some(left), Some(right)) => left.cmp(&right),
        _ => normalized(left).cmp(&normalized(right)),
    }
}

fn parse_temporal(value: &Value) -> Option<i64> {
    let value = value.as_str()?;
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|date| date.and_utc().timestamp_millis())
        })
}

fn normalized(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn field(id: u128, kind: &str) -> Field {
        Field {
            id: Uuid::from_u128(id),
            database_id: Uuid::from_u128(9),
            name: "Field".into(),
            field_type: kind.into(),
            position: 0,
            version: 1,
            config: empty_object(),
            relation_target_database_id: None,
            relation_cardinality: None,
            relation_owner_field_id: None,
            inverse_field_id: None,
            rollup_relation_field_id: None,
            rollup_target_field_id: None,
            rollup_operation: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
    fn record(id: u128, values: Value) -> Record {
        Record {
            id: Uuid::from_u128(id),
            database_id: Uuid::from_u128(9),
            position: i32::try_from(id).unwrap(),
            version: 1,
            values,
            content_markdown: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn config_rejects_foreign_fields_unknown_operators_and_duplicate_layout() {
        let field = field(1, "number");
        assert!(validate_config(std::slice::from_ref(&field), &serde_json::json!({"filters":[{"field_id":Uuid::from_u128(2),"operator":"equals","value":1}]})).is_err());
        assert!(validate_config(std::slice::from_ref(&field), &serde_json::json!({"filters":[{"field_id":field.id,"operator":"contains","value":"1"}]})).is_err());
        assert!(
            validate_config(
                std::slice::from_ref(&field),
                &serde_json::json!({"hidden_fields":[field.id,field.id]})
            )
            .is_err()
        );
        assert!(
            validate_config(
                std::slice::from_ref(&field),
                &serde_json::json!({"column_widths":{field.id.to_string():63}})
            )
            .is_err()
        );
        assert!(validate_config(&[field], &serde_json::json!({"unknown":true})).is_err());
    }

    #[test]
    fn filtering_excludes_empty_numeric_values_and_sorting_is_stable() {
        let field = field(1, "number");
        let key = field.id.to_string();
        let mut records = vec![
            record(3, serde_json::json!({key.clone():12})),
            record(2, serde_json::json!({key.clone():12})),
            record(1, empty_object()),
            record(4, serde_json::json!({key:5})),
        ];
        let config = serde_json::json!({"filters":[{"field_id":field.id,"operator":"greater_than","value":8}],"sorts":[{"field_id":field.id,"direction":"desc"}]});
        validate_config(&[field], &config).unwrap();
        apply_view(&mut records, &config).unwrap();
        assert_eq!(
            records.iter().map(|record| record.id).collect::<Vec<_>>(),
            vec![Uuid::from_u128(2), Uuid::from_u128(3)]
        );
        assert!(compare_value(&Value::Null, &serde_json::json!(9)).is_gt());
    }

    #[test]
    fn mixed_timestamp_and_plain_text_comparison_is_transitive() {
        let early = serde_json::json!("2026-01-02T01:00:00+14:00");
        let late = serde_json::json!("2026-01-01T23:00:00-10:00");
        let text = serde_json::json!("2026-01-01TZZ");
        assert!(compare_value(&early, &late).is_lt());
        assert!(compare_value(&late, &text).is_lt());
        assert!(compare_value(&early, &text).is_lt());
    }
}
