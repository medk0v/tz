//! Canonical relation edges and read-only rollups, under the caller's project lock.

use std::collections::{HashMap, HashSet};

use serde_json::{Number, Value, json};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use super::model::{CreateField, Field, Record, Scope};
use crate::error::AppError;

type Tx<'a> = Transaction<'a, Postgres>;

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.to_owned())
}

async fn field(tx: &mut Tx<'_>, scope: &Scope, id: Uuid) -> Result<Field, AppError> {
    sqlx::query_as(
        "SELECT * FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
    )
    .bind(scope.tenant_id)
    .bind(scope.project_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)
}

/// Complete a newly inserted relation/rollup field and create the automatic inverse.
///
/// # Errors
/// Rejects invalid settings, inaccessible targets, full databases or database failures.
pub async fn initialize_field(
    tx: &mut Tx<'_>,
    scope: &Scope,
    current: &Field,
    input: &CreateField,
) -> Result<Field, AppError> {
    match current.field_type.as_str() {
        "relation" => {
            let relation = input
                .relation
                .as_ref()
                .ok_or_else(|| invalid("relation settings are required"))?;
            if input.rollup.is_some()
                || !matches!(
                    relation.cardinality.as_str(),
                    "one_to_one" | "one_to_many" | "many_to_many"
                )
            {
                return Err(invalid("invalid relation settings"));
            }
            let name = relation.inverse_name.trim();
            if name.is_empty() || name.chars().count() > 160 || name.contains('\0') {
                return Err(invalid(
                    "inverse_name must contain 1 to 160 characters without NUL",
                ));
            }
            super::get_database(tx, scope, relation.target_database_id).await?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3")
                .bind(scope.tenant_id).bind(scope.project_id).bind(relation.target_database_id).fetch_one(&mut **tx).await?;
            if count >= 100 {
                return Err(AppError::Conflict(
                    "the inverse database cannot contain more than 100 fields".to_owned(),
                ));
            }
            let inverse_id = Uuid::now_v7();
            let position: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(position), -1)+1 FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3")
                .bind(scope.tenant_id).bind(scope.project_id).bind(relation.target_database_id).fetch_one(&mut **tx).await?;
            sqlx::query("INSERT INTO note_database_fields (id,tenant_id,project_id,database_id,name,field_type,position,config,relation_target_database_id,relation_cardinality,relation_owner_field_id,inverse_field_id) VALUES ($1,$2,$3,$4,$5,'relation',$6,'{}',$7,$8,$9,$9)")
                .bind(inverse_id).bind(scope.tenant_id).bind(scope.project_id).bind(relation.target_database_id).bind(name).bind(position)
                .bind(current.database_id).bind(&relation.cardinality).bind(current.id).execute(&mut **tx).await?;
            sqlx::query("UPDATE note_database_fields SET inverse_field_id=$4 WHERE tenant_id=$1 AND project_id=$2 AND id=$3")
                .bind(scope.tenant_id).bind(scope.project_id).bind(current.id).bind(inverse_id).execute(&mut **tx).await?;
            if relation.target_database_id != current.database_id {
                super::bump_database(tx, scope, relation.target_database_id).await?;
            }
            field(tx, scope, current.id).await
        }
        "rollup" => {
            let rollup = input
                .rollup
                .as_ref()
                .ok_or_else(|| invalid("rollup settings are required"))?;
            if input.relation.is_some()
                || !matches!(
                    rollup.operation.as_str(),
                    "count" | "sum" | "min" | "max" | "show_values"
                )
            {
                return Err(invalid("invalid rollup operation"));
            }
            let relation = field(tx, scope, rollup.relation_field_id).await?;
            if relation.database_id != current.database_id || relation.field_type != "relation" {
                return Err(invalid(
                    "rollup must reference a relation field in the same database",
                ));
            }
            let target_database = relation
                .relation_target_database_id
                .ok_or_else(|| invalid("relation target is missing"))?;
            if rollup.operation == "count" && rollup.target_field_id.is_none() {
                return Ok(current.clone());
            }
            let target = field(
                tx,
                scope,
                rollup
                    .target_field_id
                    .ok_or_else(|| invalid("rollup target field is required"))?,
            )
            .await?;
            if target.database_id != target_database {
                return Err(AppError::NotFound);
            }
            validate_rollup_target(&rollup.operation, &target.field_type)?;
            Ok(current.clone())
        }
        _ if input.relation.is_some() || input.rollup.is_some() => Err(invalid(
            "relation or rollup settings do not match field_type",
        )),
        _ => Ok(current.clone()),
    }
}

fn validate_rollup_target(operation: &str, field_type: &str) -> Result<(), AppError> {
    if matches!(field_type, "relation" | "rollup")
        || (operation == "sum" && field_type != "number")
        || (matches!(operation, "min" | "max") && !matches!(field_type, "number" | "date"))
    {
        return Err(invalid(
            "rollup operation is incompatible with its target field",
        ));
    }
    Ok(())
}

/// Prevent type changes that would invalidate existing computed fields.
///
/// # Errors
/// Rejects structural type changes, incompatible rollups or database failures.
pub async fn validate_field_update(
    tx: &mut Tx<'_>,
    scope: &Scope,
    current: &Field,
    next_type: &str,
) -> Result<(), AppError> {
    if current.field_type != next_type
        && (matches!(current.field_type.as_str(), "relation" | "rollup")
            || matches!(next_type, "relation" | "rollup"))
    {
        return Err(AppError::Conflict(
            "relation and rollup field types cannot be changed".to_owned(),
        ));
    }
    let operations: Vec<String> = sqlx::query_scalar("SELECT rollup_operation FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND rollup_target_field_id=$3")
        .bind(scope.tenant_id).bind(scope.project_id).bind(current.id).fetch_all(&mut **tx).await?;
    if operations
        .iter()
        .any(|operation| validate_rollup_target(operation, next_type).is_err())
    {
        return Err(AppError::Conflict(
            "field type is used by an incompatible rollup".to_owned(),
        ));
    }
    Ok(())
}

/// Delete the inverse too, but require dependent rollups to be removed explicitly.
///
/// # Errors
/// Rejects missing inverse fields, dependent rollups or database failures.
pub async fn delete_field(tx: &mut Tx<'_>, scope: &Scope, current: &Field) -> Result<(), AppError> {
    let mut ids = vec![current.id];
    let mut inverse = None;
    if current.field_type == "relation" {
        let opposite = current.inverse_field_id.or(current.relation_owner_field_id);
        if let Some(id) = opposite.filter(|id| *id != current.id) {
            inverse = Some(field(tx, scope, id).await?);
            ids.push(id);
        }
    }
    let dependent: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND (rollup_relation_field_id=ANY($3) OR rollup_target_field_id=ANY($3)))")
        .bind(scope.tenant_id).bind(scope.project_id).bind(&ids).fetch_one(&mut **tx).await?;
    if dependent {
        return Err(AppError::Conflict(
            "remove dependent rollup fields first".to_owned(),
        ));
    }
    super::views::remove_field_references(tx, scope, &ids).await?;
    if let Some(inverse) = inverse {
        sqlx::query("UPDATE note_database_fields SET inverse_field_id=NULL, relation_owner_field_id=NULL WHERE tenant_id=$1 AND project_id=$2 AND id=ANY($3)")
            .bind(scope.tenant_id).bind(scope.project_id).bind(&ids).execute(&mut **tx).await?;
        sqlx::query(
            "DELETE FROM note_database_fields WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
        )
        .bind(scope.tenant_id)
        .bind(scope.project_id)
        .bind(inverse.id)
        .execute(&mut **tx)
        .await?;
        if inverse.database_id != current.database_id {
            super::bump_database(tx, scope, inverse.database_id).await?;
        }
    }
    Ok(())
}

#[derive(FromRow)]
struct Context {
    owner_id: Uuid,
    source_database_id: Uuid,
    target_database_id: Uuid,
    cardinality: String,
    source_side: bool,
}

async fn context(tx: &mut Tx<'_>, scope: &Scope, current: &Field) -> Result<Context, AppError> {
    sqlx::query_as("SELECT owner.id AS owner_id,owner.database_id AS source_database_id,owner.relation_target_database_id AS target_database_id,owner.relation_cardinality AS cardinality,(owner.id=field.id) AS source_side FROM note_database_fields field JOIN note_database_fields owner ON owner.tenant_id=field.tenant_id AND owner.project_id=field.project_id AND owner.id=COALESCE(field.relation_owner_field_id,field.id) WHERE field.tenant_id=$1 AND field.project_id=$2 AND field.id=$3 AND field.field_type='relation' AND owner.field_type='relation'")
        .bind(scope.tenant_id).bind(scope.project_id).bind(current.id).fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)
}

fn target_ids(value: Option<&Value>, record_id: Uuid) -> Result<Vec<Uuid>, AppError> {
    if value.is_none_or(Value::is_null) {
        return Ok(Vec::new());
    }
    let items = value
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("relation value must be an array of record UUIDs"))?;
    if items.len() > 100 {
        return Err(invalid(
            "a relation can link at most 100 records per request",
        ));
    }
    let mut seen = HashSet::new();
    items
        .iter()
        .map(|item| {
            let id = item
                .as_str()
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| invalid("invalid relation record UUID"))?;
            if id == record_id || !seen.insert(id) {
                return Err(invalid(
                    "relation records must be unique and cannot reference themselves",
                ));
            }
            Ok(id)
        })
        .collect()
}

/// Replace submitted relation values; keep only scalar values in the record's JSON.
///
/// # Errors
/// Rejects malformed targets, cross-scope records, cardinality conflicts or database failures.
pub async fn replace_record_relations(
    tx: &mut Tx<'_>,
    scope: &Scope,
    fields: &[Field],
    record_id: Uuid,
    values: &mut Value,
) -> Result<(), AppError> {
    let values = values
        .as_object_mut()
        .ok_or_else(|| invalid("values must be an object"))?;
    for current in fields {
        let key = current.id.to_string();
        if current.field_type == "rollup" && values.contains_key(&key) {
            return Err(invalid("rollup values are read-only"));
        }
        if current.field_type != "relation" {
            continue;
        }
        let ids = target_ids(values.get(&key), record_id)?;
        let relation = context(tx, scope, current).await?;
        if ids.len() > 1
            && (relation.cardinality == "one_to_one"
                || (!relation.source_side && relation.cardinality == "one_to_many"))
        {
            return Err(AppError::Conflict(
                "relation cardinality allows only one related record".to_owned(),
            ));
        }
        let target_database = if relation.source_side {
            relation.target_database_id
        } else {
            relation.source_database_id
        };
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND database_id=$3 AND id=ANY($4)")
            .bind(scope.tenant_id).bind(scope.project_id).bind(target_database).bind(&ids).fetch_one(&mut **tx).await?;
        if count != i64::try_from(ids.len()).map_err(AppError::internal)? {
            return Err(AppError::NotFound);
        }
        let old = related_ids(tx, scope, &relation, record_id).await?;
        if old.iter().copied().collect::<HashSet<_>>()
            != ids.iter().copied().collect::<HashSet<_>>()
        {
            let deletion = if relation.source_side {
                "DELETE FROM note_record_relations WHERE tenant_id=$1 AND project_id=$2 AND relation_field_id=$3 AND source_record_id=$4"
            } else {
                "DELETE FROM note_record_relations WHERE tenant_id=$1 AND project_id=$2 AND relation_field_id=$3 AND target_record_id=$4"
            };
            sqlx::query(deletion)
                .bind(scope.tenant_id)
                .bind(scope.project_id)
                .bind(relation.owner_id)
                .bind(record_id)
                .execute(&mut **tx)
                .await?;
            for id in &ids {
                let (source, target) = if relation.source_side {
                    (record_id, *id)
                } else {
                    (*id, record_id)
                };
                sqlx::query("INSERT INTO note_record_relations (id,tenant_id,project_id,relation_field_id,source_database_id,target_database_id,source_record_id,target_record_id,cardinality) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                    .bind(Uuid::now_v7()).bind(scope.tenant_id).bind(scope.project_id).bind(relation.owner_id).bind(relation.source_database_id).bind(relation.target_database_id)
                    .bind(source).bind(target).bind(&relation.cardinality).execute(&mut **tx).await.map_err(|error| {
                        if error.as_database_error().is_some_and(sqlx::error::DatabaseError::is_unique_violation) {
                            AppError::Conflict("relation violates its cardinality".to_owned())
                        } else { AppError::Database(error) }
                    })?;
            }
            let changed: Vec<_> = old
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .symmetric_difference(&ids.iter().copied().collect())
                .copied()
                .collect();
            sqlx::query("UPDATE note_database_records SET version=version+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND project_id=$2 AND id=ANY($3) AND id<>$4")
                .bind(scope.tenant_id).bind(scope.project_id).bind(changed).bind(record_id).execute(&mut **tx).await?;
        }
        values.remove(&key);
    }
    Ok(())
}

async fn related_ids(
    tx: &mut Tx<'_>,
    scope: &Scope,
    relation: &Context,
    record_id: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    let query = if relation.source_side {
        "SELECT target_record_id FROM note_record_relations WHERE tenant_id=$1 AND project_id=$2 AND relation_field_id=$3 AND source_record_id=$4"
    } else {
        "SELECT source_record_id FROM note_record_relations WHERE tenant_id=$1 AND project_id=$2 AND relation_field_id=$3 AND target_record_id=$4"
    };
    Ok(sqlx::query_scalar(query)
        .bind(scope.tenant_id)
        .bind(scope.project_id)
        .bind(relation.owner_id)
        .bind(record_id)
        .fetch_all(&mut **tx)
        .await?)
}

#[derive(FromRow)]
struct Related {
    record_id: Uuid,
    target_id: Uuid,
}

/// Project related UUIDs and aggregate scalar targets without recursive computed fields.
///
/// # Errors
/// Rejects inconsistent field scopes, invalid aggregates or database failures.
pub async fn project_records(
    tx: &mut Tx<'_>,
    scope: &Scope,
    database_id: Uuid,
    fields: &[Field],
    records: &mut [Record],
) -> Result<(), AppError> {
    if records.is_empty() {
        return Ok(());
    }
    if fields.iter().any(|field| field.database_id != database_id)
        || records
            .iter()
            .any(|record| record.database_id != database_id)
    {
        return Err(AppError::NotFound);
    }
    let record_ids: Vec<_> = records.iter().map(|record| record.id).collect();
    let mut related: HashMap<Uuid, HashMap<Uuid, Vec<Related>>> = HashMap::new();
    for current in fields.iter().filter(|field| field.field_type == "relation") {
        let relation = context(tx, scope, current).await?;
        let query = if relation.source_side {
            "SELECT edge.source_record_id AS record_id,target.id AS target_id FROM note_record_relations edge JOIN note_database_records target ON target.tenant_id=edge.tenant_id AND target.project_id=edge.project_id AND target.database_id=edge.target_database_id AND target.id=edge.target_record_id WHERE edge.tenant_id=$1 AND edge.project_id=$2 AND edge.relation_field_id=$3 AND edge.source_record_id=ANY($4) ORDER BY target.position,target.id"
        } else {
            "SELECT edge.target_record_id AS record_id,target.id AS target_id FROM note_record_relations edge JOIN note_database_records target ON target.tenant_id=edge.tenant_id AND target.project_id=edge.project_id AND target.database_id=edge.source_database_id AND target.id=edge.source_record_id WHERE edge.tenant_id=$1 AND edge.project_id=$2 AND edge.relation_field_id=$3 AND edge.target_record_id=ANY($4) ORDER BY target.position,target.id"
        };
        let rows: Vec<Related> = sqlx::query_as(query)
            .bind(scope.tenant_id)
            .bind(scope.project_id)
            .bind(relation.owner_id)
            .bind(&record_ids)
            .fetch_all(&mut **tx)
            .await?;
        let mut grouped: HashMap<Uuid, Vec<Related>> = HashMap::new();
        for row in rows {
            grouped.entry(row.record_id).or_default().push(row);
        }
        for record in records.iter_mut() {
            let ids: Vec<_> = grouped
                .get(&record.id)
                .into_iter()
                .flatten()
                .map(|row| row.target_id)
                .collect();
            record.values[current.id.to_string()] = json!(ids);
        }
        related.insert(current.id, grouped);
    }
    for current in fields.iter().filter(|field| field.field_type == "rollup") {
        let relation_id = current
            .rollup_relation_field_id
            .ok_or_else(|| invalid("rollup relation is missing"))?;
        let operation = current
            .rollup_operation
            .as_deref()
            .ok_or_else(|| invalid("rollup operation is missing"))?;
        let grouped = related
            .get(&relation_id)
            .ok_or_else(|| invalid("rollup relation is missing"))?;
        // Load only the scalar being aggregated, once per distinct target. Relation
        // displays and counts never need to copy an entire target record's values.
        let target_values: HashMap<Uuid, Value> = if operation == "count" {
            HashMap::new()
        } else {
            let key = current
                .rollup_target_field_id
                .ok_or_else(|| invalid("rollup target is missing"))?
                .to_string();
            let ids: Vec<_> = grouped
                .values()
                .flatten()
                .map(|target| target.target_id)
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let values: Vec<(Uuid, Value)> = sqlx::query_as("SELECT id,field_values->$4 FROM note_database_records WHERE tenant_id=$1 AND project_id=$2 AND id=ANY($3) AND field_values ? $4")
                .bind(scope.tenant_id).bind(scope.project_id).bind(ids).bind(key).fetch_all(&mut **tx).await?;
            values.into_iter().collect()
        };
        for record in records.iter_mut() {
            let targets = grouped
                .get(&record.id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let value = if operation == "count" {
                json!(targets.len())
            } else {
                let values: Vec<_> = targets
                    .iter()
                    .filter_map(|target| target_values.get(&target.target_id))
                    .filter(|value| !value.is_null())
                    .cloned()
                    .collect();
                aggregate(operation, &values)?
            };
            record.values[current.id.to_string()] = value;
        }
    }
    Ok(())
}

/// Aggregate already validated scalar target values.
///
/// # Errors
/// Rejects unsupported operations and sums outside the safe numeric range.
pub fn aggregate(operation: &str, values: &[Value]) -> Result<Value, AppError> {
    match operation {
        "show_values" => Ok(Value::Array(values.to_vec())),
        "sum" => {
            let sum: f64 = values.iter().filter_map(Value::as_f64).sum();
            if !sum.is_finite() || sum.abs() > 9_007_199_254_740_991_f64 {
                return Err(AppError::Conflict(
                    "rollup sum exceeds the safe numeric range".to_owned(),
                ));
            }
            Number::from_f64(sum)
                .map(Value::Number)
                .ok_or_else(|| invalid("invalid rollup sum"))
        }
        "min" | "max" => Ok(values
            .iter()
            .cloned()
            .reduce(|left, right| {
                let comparison = super::views::compare_value(&left, &right);
                if (operation == "min" && comparison.is_gt())
                    || (operation == "max" && comparison.is_lt())
                {
                    right
                } else {
                    left
                }
            })
            .unwrap_or(Value::Null)),
        _ => Err(invalid("unknown rollup operation")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_input_rejects_duplicates_self_links_and_oversized_sets() {
        let source = Uuid::from_u128(1);
        let target = Uuid::from_u128(2);
        assert!(target_ids(Some(&json!([source])), source).is_err());
        assert!(target_ids(Some(&json!([target, target])), source).is_err());
        assert!(target_ids(Some(&json!("not an array")), source).is_err());
        assert!(
            target_ids(
                Some(&json!((2..103).map(Uuid::from_u128).collect::<Vec<_>>())),
                source
            )
            .is_err()
        );
        assert_eq!(target_ids(None, source).unwrap(), Vec::<Uuid>::new());
    }

    #[test]
    fn rollups_bound_sums_and_compare_dates_by_instant() {
        assert!(aggregate("sum", &[json!(9_007_199_254_740_991_u64), json!(1)]).is_err());
        assert_eq!(aggregate("sum", &[]).unwrap(), json!(0.0));
        assert_eq!(aggregate("min", &[]).unwrap(), Value::Null);
        let values = [
            json!("2026-01-01T01:00:00+02:00"),
            json!("2026-01-01T00:00:00Z"),
        ];
        assert_eq!(aggregate("min", &values).unwrap(), values[0]);
        assert_eq!(
            aggregate("show_values", &[json!("A"), json!("B")]).unwrap(),
            json!(["A", "B"])
        );
        assert!(validate_rollup_target("sum", "date").is_err());
        assert!(validate_rollup_target("show_values", "relation").is_err());
    }
}
