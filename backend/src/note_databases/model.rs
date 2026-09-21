use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub struct Scope {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct Database {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct Field {
    pub id: Uuid,
    pub database_id: Uuid,
    pub name: String,
    pub field_type: String,
    pub position: i32,
    pub version: i32,
    pub config: Value,
    pub relation_target_database_id: Option<Uuid>,
    pub relation_cardinality: Option<String>,
    pub relation_owner_field_id: Option<Uuid>,
    pub inverse_field_id: Option<Uuid>,
    pub rollup_relation_field_id: Option<Uuid>,
    pub rollup_target_field_id: Option<Uuid>,
    pub rollup_operation: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct Record {
    pub id: Uuid,
    pub database_id: Uuid,
    pub position: i32,
    pub version: i32,
    pub values: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_markdown: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct View {
    pub id: Uuid,
    pub database_id: Uuid,
    pub name: String,
    pub position: i32,
    pub version: i32,
    pub config: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct Embed {
    pub id: Uuid,
    pub note_id: Uuid,
    pub database_id: Uuid,
    pub view_id: Option<Uuid>,
    pub position: i32,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct List<T> {
    pub items: Vec<T>,
}

#[derive(Serialize)]
pub struct DatabaseDetail {
    pub database: Database,
    pub fields: Vec<Field>,
    pub views: Vec<View>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDatabase {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateDatabase {
    pub name: String,
    pub description: String,
    pub icon: String,
    pub expected_version: i32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationInput {
    pub target_database_id: Uuid,
    pub cardinality: String,
    pub inverse_name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollupInput {
    pub relation_field_id: Uuid,
    pub target_field_id: Option<Uuid>,
    pub operation: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateField {
    pub name: String,
    pub field_type: String,
    #[serde(default = "empty_object")]
    pub config: Value,
    pub relation: Option<RelationInput>,
    pub rollup: Option<RollupInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateField {
    pub name: String,
    pub field_type: String,
    pub config: Value,
    pub expected_version: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRecord {
    #[serde(default = "empty_object")]
    pub values: Value,
    #[serde(default)]
    pub content_markdown: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRecord {
    pub values: Value,
    pub content_markdown: String,
    pub expected_version: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderInput {
    pub ids: Vec<Uuid>,
    pub expected_version: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionQuery {
    pub expected_version: i32,
}

pub fn empty_object() -> Value {
    serde_json::json!({})
}
