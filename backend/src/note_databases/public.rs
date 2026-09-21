//! Database access through revocable note publications. All reads and writes retain the share lock.
use std::collections::HashSet;

use axum::{Json, extract::State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    fields,
    model::{CreateField, CreateRecord, Database, Field, Record, UpdateField, UpdateRecord},
    records,
};
use crate::{AppState, error::AppError, note_shares};

#[derive(Deserialize)]
pub struct Request {
    token: String,
    access_token: Option<String>,
    #[serde(flatten)]
    action: Action,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    List,
    Embeds {
        note_id: Uuid,
    },
    Detail {
        database_id: Uuid,
    },
    Records {
        database_id: Uuid,
        #[serde(default)]
        view_id: Option<Uuid>,
        #[serde(default)]
        q: String,
        page: Option<usize>,
        per_page: Option<usize>,
    },
    Record {
        database_id: Uuid,
        record_id: Uuid,
    },
    CreateRecord {
        database_id: Uuid,
        values: Value,
        content_markdown: String,
    },
    UpdateRecord {
        database_id: Uuid,
        record_id: Uuid,
        values: Value,
        content_markdown: String,
        expected_version: i32,
    },
    DeleteRecord {
        database_id: Uuid,
        record_id: Uuid,
        expected_version: i32,
    },
    CreateField {
        database_id: Uuid,
        field: CreateField,
    },
    UpdateField {
        database_id: Uuid,
        field_id: Uuid,
        field: UpdateField,
    },
    DeleteField {
        database_id: Uuid,
        field_id: Uuid,
        expected_version: i32,
    },
}

impl Action {
    fn is_write(&self) -> bool {
        matches!(
            self,
            Self::CreateRecord { .. }
                | Self::UpdateRecord { .. }
                | Self::DeleteRecord { .. }
                | Self::CreateField { .. }
                | Self::UpdateField { .. }
                | Self::DeleteField { .. }
        )
    }
    fn database_id(&self) -> Option<Uuid> {
        match self {
            Self::List | Self::Embeds { .. } => None,
            Self::Detail { database_id }
            | Self::Records { database_id, .. }
            | Self::Record { database_id, .. }
            | Self::CreateRecord { database_id, .. }
            | Self::UpdateRecord { database_id, .. }
            | Self::DeleteRecord { database_id, .. }
            | Self::CreateField { database_id, .. }
            | Self::UpdateField { database_id, .. }
            | Self::DeleteField { database_id, .. } => Some(*database_id),
        }
    }
}

/// # Errors
/// Returns publication, scope, validation, conflict or storage errors.
pub async fn dispatch(
    State(state): State<AppState>,
    Json(input): Json<Request>,
) -> Result<Json<Value>, AppError> {
    let mut transaction = state.db.begin().await?;
    let (scope, note_ids) = note_shares::authorize_databases(
        &mut transaction,
        &state,
        &input.token,
        input.access_token.as_deref(),
        input.action.is_write(),
    )
    .await?;
    let visible_databases: HashSet<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT database_id FROM note_database_embeds WHERE tenant_id = $1 AND project_id = $2 AND note_id = ANY($3)",
    ).bind(scope.tenant_id).bind(scope.project_id).bind(&note_ids).fetch_all(&mut *transaction).await?.into_iter().collect();
    let mut all_fields = Vec::new();
    let mut allowed = HashSet::new();
    if let Some(database_id) = input.action.database_id() {
        if !visible_databases.contains(&database_id) {
            return Err(AppError::NotFound);
        }
        all_fields = fields::list(&mut transaction, &scope, database_id).await?;
        allowed = visible_field_ids(&all_fields, &visible_databases);
    }
    let result = match input.action {
        Action::List => {
            let items: Vec<Database> = sqlx::query_as(
                "SELECT id, project_id, name, description, icon, version, created_at, updated_at FROM note_databases
                 WHERE tenant_id = $1 AND project_id = $2 AND id = ANY($3) ORDER BY name, id",
            ).bind(scope.tenant_id).bind(scope.project_id).bind(visible_databases.into_iter().collect::<Vec<_>>()).fetch_all(&mut *transaction).await?;
            json!({"items": items})
        }
        Action::Embeds { note_id } => {
            if !note_ids.contains(&note_id) {
                return Err(AppError::NotFound);
            }
            json!({"items": super::embeds::list(&mut transaction, &scope, note_id).await?})
        }
        Action::Detail { database_id } => {
            let mut detail = super::database_detail(&mut transaction, &scope, database_id).await?;
            detail.fields.retain(|field| allowed.contains(&field.id));
            for view in &mut detail.views {
                project_view(&mut view.config, &allowed);
            }
            json!(detail)
        }
        Action::Records {
            database_id,
            view_id,
            q,
            page,
            per_page,
        } => {
            let query = records::Query {
                view_id,
                q,
                page: page.unwrap_or(1),
                per_page: per_page.unwrap_or(200),
            };
            json!(
                records::list(
                    &mut transaction,
                    &scope,
                    database_id,
                    &query,
                    Some(&allowed)
                )
                .await?
            )
        }
        Action::Record {
            database_id,
            record_id,
        } => {
            let mut record = records::get_projected(
                &mut transaction,
                &scope,
                database_id,
                record_id,
                Some(&allowed),
            )
            .await?;
            project_record(&mut record, &allowed);
            json!(record)
        }
        Action::CreateRecord {
            database_id,
            values,
            content_markdown,
        } => {
            require_visible_values(&values, &all_fields, &allowed)?;
            let mut record = records::create_projected(
                &mut transaction,
                &scope,
                database_id,
                CreateRecord {
                    values,
                    content_markdown,
                },
                None,
                Some(&allowed),
            )
            .await?;
            project_record(&mut record, &allowed);
            json!(record)
        }
        Action::UpdateRecord {
            database_id,
            record_id,
            mut values,
            content_markdown,
            expected_version,
        } => {
            require_visible_values(&values, &all_fields, &allowed)?;
            // Full snapshots from a public editor omit private fields. Preserve those values on the server.
            let writable_fields = all_fields
                .iter()
                .filter(|field| field.field_type != "rollup")
                .map(|field| field.id)
                .collect();
            let previous = records::get_projected(
                &mut transaction,
                &scope,
                database_id,
                record_id,
                Some(&writable_fields),
            )
            .await?;
            let submitted = values
                .as_object_mut()
                .ok_or_else(|| AppError::BadRequest("values must be an object".into()))?;
            for field in &all_fields {
                if !allowed.contains(&field.id) && field.field_type != "rollup" {
                    let key = field.id.to_string();
                    submitted.insert(
                        key.clone(),
                        previous.values.get(&key).cloned().unwrap_or(Value::Null),
                    );
                }
            }
            let mut record = records::update_projected(
                &mut transaction,
                &scope,
                database_id,
                record_id,
                UpdateRecord {
                    values,
                    content_markdown,
                    expected_version,
                },
                None,
                Some(&allowed),
            )
            .await?;
            project_record(&mut record, &allowed);
            json!(record)
        }
        Action::DeleteRecord {
            database_id,
            record_id,
            expected_version,
        } => {
            records::delete(
                &mut transaction,
                &scope,
                database_id,
                record_id,
                expected_version,
                None,
            )
            .await?;
            Value::Null
        }
        Action::CreateField { database_id, field } => {
            if field
                .relation
                .as_ref()
                .is_some_and(|relation| !visible_databases.contains(&relation.target_database_id))
                || field
                    .rollup
                    .as_ref()
                    .is_some_and(|rollup| !allowed.contains(&rollup.relation_field_id))
            {
                return Err(AppError::NotFound);
            }
            json!(fields::create(&mut transaction, &scope, database_id, field, None).await?)
        }
        Action::UpdateField {
            database_id,
            field_id,
            field,
        } => {
            if !allowed.contains(&field_id) {
                return Err(AppError::NotFound);
            }
            json!(
                fields::update(&mut transaction, &scope, database_id, field_id, field, None)
                    .await?
            )
        }
        Action::DeleteField {
            database_id,
            field_id,
            expected_version,
        } => {
            if !allowed.contains(&field_id) {
                return Err(AppError::NotFound);
            }
            fields::delete(
                &mut transaction,
                &scope,
                database_id,
                field_id,
                expected_version,
                None,
            )
            .await?;
            Value::Null
        }
    };
    transaction.commit().await?;
    Ok(Json(result))
}

fn visible_field_ids(fields: &[Field], databases: &HashSet<Uuid>) -> HashSet<Uuid> {
    fields
        .iter()
        .filter(|field| match field.field_type.as_str() {
            "relation" => field
                .relation_target_database_id
                .is_some_and(|id| databases.contains(&id)),
            "rollup" => fields
                .iter()
                .find(|relation| Some(relation.id) == field.rollup_relation_field_id)
                .and_then(|relation| relation.relation_target_database_id)
                .is_some_and(|id| databases.contains(&id)),
            _ => true,
        })
        .map(|field| field.id)
        .collect()
}

fn project_record(record: &mut Record, allowed: &HashSet<Uuid>) {
    if let Some(values) = record.values.as_object_mut() {
        values.retain(|key, _| Uuid::parse_str(key).is_ok_and(|id| allowed.contains(&id)));
    }
}

fn project_view(config: &mut Value, allowed: &HashSet<Uuid>) {
    let visible = |value: &Value| {
        value
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .is_some_and(|id| allowed.contains(&id))
    };
    for key in ["filters", "sorts"] {
        if let Some(items) = config.get_mut(key).and_then(Value::as_array_mut) {
            items.retain(|item| visible(&item["field_id"]));
        }
    }
    for key in ["hidden_fields", "column_order", "pinned_fields"] {
        if let Some(items) = config.get_mut(key).and_then(Value::as_array_mut) {
            items.retain(visible);
        }
    }
    if let Some(widths) = config
        .get_mut("column_widths")
        .and_then(Value::as_object_mut)
    {
        widths.retain(|key, _| Uuid::parse_str(key).is_ok_and(|id| allowed.contains(&id)));
    }
}

fn require_visible_values(
    values: &Value,
    fields: &[Field],
    allowed: &HashSet<Uuid>,
) -> Result<(), AppError> {
    let values = values
        .as_object()
        .ok_or_else(|| AppError::BadRequest("values must be an object".into()))?;
    for key in values.keys() {
        let id =
            Uuid::parse_str(key).map_err(|_| AppError::BadRequest("invalid field ID".into()))?;
        if !allowed.contains(&id)
            || fields
                .iter()
                .any(|field| field.id == id && field.field_type == "rollup")
        {
            return Err(AppError::BadRequest(
                "field is not editable through this publication".into(),
            ));
        }
    }
    Ok(())
}
