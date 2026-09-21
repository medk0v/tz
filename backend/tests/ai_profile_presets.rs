use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_settings};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

#[sqlx::test(migrations = false)]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn presets_backfill_and_seed_projects_without_overwriting_user_profiles(db: PgPool) {
    for migration in sqlx::migrate!()
        .iter()
        .filter(|migration| migration.version < 81)
    {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Preset tenant');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'presets@example.test', 'Owner');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Existing', 'existing');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, instructions, created_by)
        VALUES ('{custom}', '{tenant}', '{project}', 'Агент продаж', 'Keep user instructions', '{user}');
    "#, tenant=id(1), user=id(2), project=id(3), custom=id(4))).execute(&db).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/0081_ai_profile_presets.sql"))
        .execute(&db)
        .await
        .unwrap();

    let profiles: Vec<(Option<String>, String, String)> = sqlx::query_as("SELECT preset_key,name,instructions FROM ai_profiles WHERE project_id=$1 ORDER BY preset_key NULLS FIRST")
        .bind(id(3)).fetch_all(&db).await.unwrap();
    assert_eq!(profiles.len(), 6);
    assert_eq!(
        profiles[0],
        (None, "Агент продаж".into(), "Keep user instructions".into())
    );
    assert!(
        profiles
            .iter()
            .any(|(key, name, _)| key.as_deref() == Some("sales") && name == "Агент продаж (1)")
    );
    assert!(
        profiles
            .iter()
            .filter(|(key, _, _)| key.is_some())
            .all(|(_, _, instructions)| instructions.chars().count() > 500)
    );
    let safe: bool = sqlx::query_scalar("SELECT bool_and(status='draft' AND provider_connection_id IS NULL AND model IS NULL AND created_by IS NULL AND NOT auto_join_new_conversations AND NOT can_resolve_conversations AND NOT capability_http_get AND NOT capability_http_post AND NOT capability_shell AND NOT telegram_notify_on_new_visitor AND NOT telegram_notify_on_new_message AND NOT telegram_notify_on_operator_request AND cardinality(http_allowed_hosts)=0) FROM ai_profiles WHERE project_id=$1 AND preset_key IS NOT NULL")
        .bind(id(3)).fetch_one(&db).await.unwrap();
    assert!(safe);
    sqlx::query("UPDATE ai_profiles SET name='Edited preset',instructions='Edited instructions',description='Edited purpose' WHERE project_id=$1 AND preset_key='quality'").bind(id(3)).execute(&db).await.unwrap();
    sqlx::query("SELECT seed_ai_profile_presets($1,$2)")
        .bind(id(1))
        .bind(id(3))
        .execute(&db)
        .await
        .unwrap();
    let edited: (String,String,String) = sqlx::query_as("SELECT name,instructions,description FROM ai_profiles WHERE project_id=$1 AND preset_key='quality'").bind(id(3)).fetch_one(&db).await.unwrap();
    assert_eq!(
        edited,
        (
            "Edited preset".into(),
            "Edited instructions".into(),
            "Edited purpose".into()
        )
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_profiles WHERE project_id=$1")
        .bind(id(3))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 6);

    sqlx::query("INSERT INTO projects (id,tenant_id,name,slug) VALUES ($1,$2,'New','new')")
        .bind(id(5))
        .bind(id(1))
        .execute(&db)
        .await
        .unwrap();
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT preset_key FROM ai_profiles WHERE project_id=$1 ORDER BY preset_key",
    )
    .bind(id(5))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        keys,
        vec!["hr", "operations", "quality", "sales", "universal"]
    );
    sqlx::query("SELECT seed_ai_profile_presets($1,$2)")
        .bind(id(999))
        .bind(id(5))
        .execute(&db)
        .await
        .unwrap();
    let invalid_creator = sqlx::query("INSERT INTO ai_profiles (id,tenant_id,project_id,name,created_by) VALUES ($1,$2,$3,'No creator',NULL)")
        .bind(id(6)).bind(id(1)).bind(id(5)).execute(&db).await;
    assert_eq!(
        invalid_creator
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(input.map_or_else(Body::empty, |input| Body::from(input.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[sqlx::test(migrations = false)]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn support_catalog_upgrades_only_untouched_operations_and_retains_legacy_data(db: PgPool) {
    for migration in sqlx::migrate!()
        .iter()
        .filter(|migration| migration.version < 82)
    {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Support catalog');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'support-presets@example.test', 'Owner');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
            ('{untouched}', '{tenant}', 'Untouched', 'untouched'),
            ('{customized}', '{tenant}', 'Customized', 'customized'),
            ('{configured}', '{tenant}', 'Configured', 'configured');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, instructions, created_by)
        VALUES ('{custom}', '{tenant}', '{untouched}', 'Агент службы поддержки', 'User support profile', '{user}');
        UPDATE ai_profiles SET name = 'My operations agent', description = 'My purpose',
            instructions = 'My instructions' WHERE project_id = '{customized}' AND preset_key = 'operations';
        UPDATE ai_profiles SET capability_http_get = true
            WHERE project_id = '{configured}' AND preset_key = 'operations';
    "#, tenant=id(1), user=id(2), untouched=id(3), customized=id(4), configured=id(5), custom=id(6)))
        .execute(&db).await.unwrap();
    let operations_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM ai_profiles WHERE project_id=$1 AND preset_key='operations'",
    )
    .bind(id(3))
    .fetch_one(&db)
    .await
    .unwrap();
    let preserved_before: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 AND (preset_key='universal' OR preset_key IS NULL OR (project_id IN ($2,$3) AND preset_key='operations')) ORDER BY id")
        .bind(id(1)).bind(id(4)).bind(id(5)).fetch_all(&db).await.unwrap();

    sqlx::raw_sql(include_str!("../migrations/0082_ai_support_preset.sql"))
        .execute(&db)
        .await
        .unwrap();

    let support: (Uuid, String, String) = sqlx::query_as(
        "SELECT id,name,instructions FROM ai_profiles WHERE project_id=$1 AND preset_key='support'",
    )
    .bind(id(3))
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(support.0, operations_id);
    assert_eq!(support.1, "Агент службы поддержки (1)");
    assert!(support.2.contains("базу знаний"));
    assert!(support.2.contains("подготовь передачу оператору"));
    let preserved_after: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 AND (preset_key='universal' OR preset_key IS NULL OR (project_id IN ($2,$3) AND preset_key='operations')) ORDER BY id")
        .bind(id(1)).bind(id(4)).bind(id(5)).fetch_all(&db).await.unwrap();
    assert_eq!(preserved_before, preserved_after);
    for project in [id(3), id(4), id(5)] {
        let keys: Vec<String> = sqlx::query_scalar("SELECT preset_key FROM ai_profiles WHERE project_id=$1 AND preset_key IN ('coordinator','support') ORDER BY preset_key")
            .bind(project).fetch_all(&db).await.unwrap();
        assert_eq!(keys, vec!["coordinator", "support"]);
    }

    sqlx::query(
        "INSERT INTO projects (id,tenant_id,name,slug) VALUES ($1,$2,'New catalog','new-catalog')",
    )
    .bind(id(7))
    .bind(id(1))
    .execute(&db)
    .await
    .unwrap();
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT preset_key FROM ai_profiles WHERE project_id=$1 ORDER BY preset_key",
    )
    .bind(id(7))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        keys,
        vec!["coordinator", "hr", "quality", "sales", "support"]
    );
    let safe: bool = sqlx::query_scalar("SELECT bool_and(status='draft' AND provider_connection_id IS NULL AND model IS NULL AND created_by IS NULL AND NOT auto_join_new_conversations AND NOT can_resolve_conversations AND NOT capability_http_get AND NOT capability_http_post AND NOT capability_shell AND NOT telegram_notify_on_new_visitor AND NOT telegram_notify_on_new_message AND NOT telegram_notify_on_operator_request AND cardinality(http_allowed_hosts)=0) FROM ai_profiles WHERE tenant_id=$1 AND preset_key IN ('coordinator','support')")
        .bind(id(1)).fetch_one(&db).await.unwrap();
    assert!(safe);
    sqlx::query("SELECT seed_ai_profile_presets($1,$2)")
        .bind(id(1))
        .bind(id(7))
        .execute(&db)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_profiles WHERE project_id=$1")
        .bind(id(7))
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 5);
}

#[sqlx::test(migrations = false)]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn finance_and_legal_presets_backfill_and_seed_without_overwriting_profiles(db: PgPool) {
    for migration in sqlx::migrate!()
        .iter()
        .filter(|migration| migration.version < 91)
    {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Finance and legal presets');
        INSERT INTO users (id,email,display_name) VALUES ('{user}','finance-legal@example.test','Owner');
        INSERT INTO projects (id,tenant_id,name,slug)
        VALUES ('{project}','{tenant}','Existing project','existing');
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,instructions,created_by,preset_key) VALUES
            ('{custom_finance}','{tenant}','{project}','АГЕНТ-ФИНАНСИСТ','User finance instructions','{user}',NULL),
            ('{custom_finance_suffix}','{tenant}','{project}','Агент-финансист (1)','Other finance instructions','{user}',NULL),
            ('{custom_legal}','{tenant}','{project}','Агент-юрист','User legal instructions','{user}',NULL),
            ('{operations}','{tenant}','{project}','My operations agent','Legacy operations instructions',NULL,'operations'),
            ('{universal}','{tenant}','{project}','My universal agent','Legacy universal instructions',NULL,'universal');
        UPDATE ai_profiles SET name='Edited sales',instructions='Edited sales instructions',
            description='Edited sales purpose',capability_http_get=true
        WHERE project_id='{project}' AND preset_key='sales';
    "#, tenant=id(1),user=id(2),project=id(3),custom_finance=id(4),custom_finance_suffix=id(5),custom_legal=id(6),operations=id(7),universal=id(8)))
        .execute(&db).await.unwrap();
    let preserved_before: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE project_id=$1 ORDER BY id")
            .bind(id(3))
            .fetch_all(&db)
            .await
            .unwrap();

    sqlx::raw_sql(include_str!(
        "../migrations/0091_ai_finance_legal_presets.sql"
    ))
    .execute(&db)
    .await
    .unwrap();

    let presets: Vec<(String, String, String, String)> = sqlx::query_as("SELECT preset_key,name,description,instructions FROM ai_profiles WHERE project_id=$1 AND preset_key IN ('finance','legal') ORDER BY preset_key")
        .bind(id(3)).fetch_all(&db).await.unwrap();
    assert_eq!(presets.len(), 2);
    assert_eq!(presets[0].0, "finance");
    assert_eq!(presets[0].1, "Агент-финансист (2)");
    assert_eq!(presets[1].0, "legal");
    assert_eq!(presets[1].1, "Агент-юрист (1)");
    assert!(presets.iter().all(|(_, _, description, instructions)| {
        !description.is_empty() && instructions.chars().count() > 500
    }));
    let preserved_after: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE project_id=$1 AND (preset_key IS NULL OR preset_key NOT IN ('finance','legal')) ORDER BY id")
        .bind(id(3)).fetch_all(&db).await.unwrap();
    assert_eq!(preserved_before, preserved_after);

    sqlx::query("INSERT INTO projects (id,tenant_id,name,slug) VALUES ($1,$2,'New project','new')")
        .bind(id(9))
        .bind(id(1))
        .execute(&db)
        .await
        .unwrap();
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT preset_key FROM ai_profiles WHERE project_id=$1 ORDER BY preset_key",
    )
    .bind(id(9))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        keys,
        vec![
            "coordinator",
            "finance",
            "hr",
            "legal",
            "quality",
            "sales",
            "support"
        ]
    );
    let safe: bool = sqlx::query_scalar("SELECT bool_and(status='draft' AND provider_connection_id IS NULL AND model IS NULL AND created_by IS NULL AND language='ru' AND max_output_tokens=2000 AND tool_instructions='' AND blacklist_reply_text='' AND blacklist_reply_match_language AND NOT auto_join_new_conversations AND NOT can_resolve_conversations AND NOT capability_http_get AND NOT capability_http_post AND NOT capability_shell AND NOT telegram_notify_on_new_visitor AND NOT telegram_notify_on_new_message AND NOT telegram_notify_on_operator_request AND cardinality(http_allowed_hosts)=0) FROM ai_profiles WHERE tenant_id=$1 AND preset_key IN ('finance','legal')")
        .bind(id(1)).fetch_one(&db).await.unwrap();
    assert!(safe);

    sqlx::query("UPDATE ai_profiles SET name='Customized ' || preset_key,instructions='Customized instructions',description='Customized purpose',language='en',max_output_tokens=800,capability_http_get=true WHERE project_id=$1 AND preset_key IN ('finance','legal')")
        .bind(id(3)).execute(&db).await.unwrap();
    let before_rerun: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 ORDER BY id")
            .bind(id(1))
            .fetch_all(&db)
            .await
            .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/0091_ai_finance_legal_presets.sql"
    ))
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("SELECT seed_ai_profile_presets($1,$2)")
        .bind(id(999))
        .bind(id(9))
        .execute(&db)
        .await
        .unwrap();
    let after_rerun: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 ORDER BY id")
            .bind(id(1))
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(before_rerun, after_rerun);
}

#[sqlx::test(migrations = false)]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn marketing_preset_backfills_and_seeds_without_overwriting_profiles(db: PgPool) {
    for migration in sqlx::migrate!()
        .iter()
        .filter(|migration| migration.version < 92)
    {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Marketing presets');
        INSERT INTO users (id,email,display_name) VALUES ('{user}','marketing-presets@example.test','Owner');
        INSERT INTO projects (id,tenant_id,name,slug)
        VALUES ('{project}','{tenant}','Existing project','existing');
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,instructions,created_by)
        VALUES ('{custom}','{tenant}','{project}','АГЕНТ-МАРКЕТОЛОГ','User marketing instructions','{user}');
        UPDATE ai_profiles SET name='Customized ' || preset_key,instructions='Customized instructions',
            description='Customized purpose',language='en',max_output_tokens=800,capability_http_get=true
        WHERE project_id='{project}' AND preset_key IN ('finance','legal');
    "#, tenant=id(1),user=id(2),project=id(3),custom=id(4)))
        .execute(&db).await.unwrap();
    let preserved_before: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE project_id=$1 ORDER BY id")
            .bind(id(3))
            .fetch_all(&db)
            .await
            .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0092_ai_marketing_preset.sql"))
        .execute(&db)
        .await
        .unwrap();
    let marketing: (String, String, String) = sqlx::query_as("SELECT name,description,instructions FROM ai_profiles WHERE project_id=$1 AND preset_key='marketing'")
        .bind(id(3)).fetch_one(&db).await.unwrap();
    assert_eq!(marketing.0, "Агент-маркетолог (1)");
    assert!(!marketing.1.is_empty());
    assert!(marketing.2.chars().count() > 500);
    let preserved_after: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE project_id=$1 AND preset_key IS DISTINCT FROM 'marketing' ORDER BY id")
        .bind(id(3)).fetch_all(&db).await.unwrap();
    assert_eq!(preserved_before, preserved_after);

    sqlx::query("INSERT INTO projects (id,tenant_id,name,slug) VALUES ($1,$2,'New project','new')")
        .bind(id(5))
        .bind(id(1))
        .execute(&db)
        .await
        .unwrap();
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT preset_key FROM ai_profiles WHERE project_id=$1 ORDER BY preset_key",
    )
    .bind(id(5))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        keys,
        vec![
            "coordinator",
            "finance",
            "hr",
            "legal",
            "marketing",
            "quality",
            "sales",
            "support"
        ]
    );
    let safe: bool = sqlx::query_scalar("SELECT bool_and(status='draft' AND provider_connection_id IS NULL AND model IS NULL AND created_by IS NULL AND language='ru' AND max_output_tokens=2000 AND tool_instructions='' AND blacklist_reply_text='' AND blacklist_reply_match_language AND NOT auto_join_new_conversations AND NOT can_resolve_conversations AND NOT capability_http_get AND NOT capability_http_post AND NOT capability_shell AND NOT telegram_notify_on_new_visitor AND NOT telegram_notify_on_new_message AND NOT telegram_notify_on_operator_request AND cardinality(http_allowed_hosts)=0) FROM ai_profiles WHERE tenant_id=$1 AND preset_key='marketing'")
        .bind(id(1)).fetch_one(&db).await.unwrap();
    assert!(safe);

    sqlx::query("UPDATE ai_profiles SET name='My marketing agent',instructions='Edited marketing instructions',description='Edited marketing purpose',capability_http_get=true WHERE project_id=$1 AND preset_key='marketing'")
        .bind(id(3)).execute(&db).await.unwrap();
    let before_rerun: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 ORDER BY id")
            .bind(id(1))
            .fetch_all(&db)
            .await
            .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0092_ai_marketing_preset.sql"))
        .execute(&db)
        .await
        .unwrap();
    let after_rerun: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 ORDER BY id")
            .bind(id(1))
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(before_rerun, after_rerun);
}

#[sqlx::test(migrations = false)]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn programmer_preset_backfills_and_seeds_without_overwriting_profiles(db: PgPool) {
    for migration in sqlx::migrate!()
        .iter()
        .filter(|migration| migration.version < 94)
    {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Programmer presets');
        INSERT INTO users (id,email,display_name) VALUES ('{user}','programmer-presets@example.test','Owner');
        INSERT INTO projects (id,tenant_id,name,slug)
        VALUES ('{project}','{tenant}','Existing project','existing');
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,instructions,created_by)
        VALUES ('{custom}','{tenant}','{project}','АГЕНТ-ПРОГРАММИСТ','User programmer instructions','{user}');
        UPDATE ai_profiles SET name='Customized ' || preset_key,instructions='Customized instructions',
            description='Customized purpose',language='en',max_output_tokens=800,capability_http_get=true
        WHERE project_id='{project}' AND preset_key IN ('finance','legal');
    "#, tenant=id(1),user=id(2),project=id(3),custom=id(4)))
        .execute(&db).await.unwrap();
    let preserved_before: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE project_id=$1 ORDER BY id")
            .bind(id(3))
            .fetch_all(&db)
            .await
            .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0094_ai_programmer_preset.sql"))
        .execute(&db)
        .await
        .unwrap();
    let programmer: (String, String, String) = sqlx::query_as("SELECT name,description,instructions FROM ai_profiles WHERE project_id=$1 AND preset_key='programmer'")
        .bind(id(3)).fetch_one(&db).await.unwrap();
    assert_eq!(programmer.0, "Агент-программист (1)");
    assert!(!programmer.1.is_empty());
    assert!(programmer.2.chars().count() > 500);
    let preserved_after: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE project_id=$1 AND preset_key IS DISTINCT FROM 'programmer' ORDER BY id")
        .bind(id(3)).fetch_all(&db).await.unwrap();
    assert_eq!(preserved_before, preserved_after);

    sqlx::query("INSERT INTO projects (id,tenant_id,name,slug) VALUES ($1,$2,'New project','new')")
        .bind(id(5))
        .bind(id(1))
        .execute(&db)
        .await
        .unwrap();
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT preset_key FROM ai_profiles WHERE project_id=$1 ORDER BY preset_key",
    )
    .bind(id(5))
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        keys,
        vec![
            "coordinator",
            "finance",
            "hr",
            "legal",
            "marketing",
            "programmer",
            "quality",
            "sales",
            "support"
        ]
    );
    let safe: bool = sqlx::query_scalar("SELECT bool_and(status='draft' AND provider_connection_id IS NULL AND model IS NULL AND created_by IS NULL AND language='ru' AND max_output_tokens=2000 AND tool_instructions='' AND blacklist_reply_text='' AND blacklist_reply_match_language AND NOT auto_join_new_conversations AND NOT can_resolve_conversations AND NOT capability_http_get AND NOT capability_http_post AND NOT capability_shell AND NOT telegram_notify_on_new_visitor AND NOT telegram_notify_on_new_message AND NOT telegram_notify_on_operator_request AND cardinality(http_allowed_hosts)=0) FROM ai_profiles WHERE tenant_id=$1 AND preset_key='programmer'")
        .bind(id(1)).fetch_one(&db).await.unwrap();
    assert!(safe);

    sqlx::query("UPDATE ai_profiles SET name='My programmer agent',instructions='Edited programmer instructions',description='Edited programmer purpose',capability_http_get=true WHERE project_id=$1 AND preset_key='programmer'")
        .bind(id(3)).execute(&db).await.unwrap();
    let before_rerun: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 ORDER BY id")
            .bind(id(1))
            .fetch_all(&db)
            .await
            .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0094_ai_programmer_preset.sql"))
        .execute(&db)
        .await
        .unwrap();
    let after_rerun: Vec<Value> =
        sqlx::query_scalar("SELECT to_jsonb(p) FROM ai_profiles p WHERE tenant_id=$1 ORDER BY id")
            .bind(id(1))
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(before_rerun, after_rerun);
}

#[sqlx::test(migrations = false)]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn frontend_presets_migration_removes_only_untouched_seeded_presets(db: PgPool) {
    for migration in sqlx::migrate!()
        .iter()
        .filter(|migration| migration.version < 119)
    {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Preset tenant');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'frontend-presets@example.test', 'Owner');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES ('{project}', '{tenant}', 'Existing', 'existing');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, instructions, created_by)
        VALUES ('{custom}', '{tenant}', '{project}', 'Custom', 'Keep user instructions', '{user}');
        UPDATE ai_profiles SET instructions = 'Edited', updated_at = now() + interval '1 second'
        WHERE project_id = '{project}' AND preset_key = 'legal';
        UPDATE ai_profiles SET name = 'Our sales', updated_at = now() + interval '1 second'
        WHERE project_id = '{project}' AND preset_key = 'sales';
        UPDATE ai_profiles SET updated_at = now() + interval '1 second'
        WHERE project_id = '{project}' AND preset_key = 'support';
        INSERT INTO ai_skills (id, tenant_id, project_id, name, instructions)
        VALUES ('{skill}', '{tenant}', '{project}', 'Labour code', 'Use the labour code');
        INSERT INTO ai_profile_skills (tenant_id, project_id, skill_id, ai_profile_id)
        SELECT '{tenant}', '{project}', '{skill}', id FROM ai_profiles WHERE project_id = '{project}' AND preset_key = 'hr';
    "#, tenant=id(1), user=id(2), project=id(3), custom=id(4), skill=id(5))).execute(&db).await.unwrap();
    let seeded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ai_profiles WHERE project_id=$1 AND preset_key IS NOT NULL",
    )
    .bind(id(3))
    .fetch_one(&db)
    .await
    .unwrap();
    assert!(seeded > 2);

    sqlx::raw_sql(include_str!("../migrations/0119_frontend_ai_presets.sql"))
        .execute(&db)
        .await
        .unwrap();

    let remaining: Vec<(Option<String>, String)> = sqlx::query_as(
        "SELECT preset_key, instructions FROM ai_profiles WHERE project_id=$1 ORDER BY preset_key NULLS FIRST",
    )
    .bind(id(3))
    .fetch_all(&db)
    .await
    .unwrap();
    // Unchanged presets go even when re-saved (support) or linked to a skill (hr);
    // edited ones stay as the project's own agents.
    assert_eq!(remaining.len(), 3, "{remaining:?}");
    assert_eq!(remaining[0], (None, "Keep user instructions".into()));
    assert_eq!(remaining[1], (Some("legal".into()), "Edited".into()));
    assert_eq!(remaining[2].0.as_deref(), Some("sales"));
    let skill_links: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_profile_skills")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(skill_links, 0);

    sqlx::query("INSERT INTO projects (id,tenant_id,name,slug) VALUES ($1,$2,'New','new')")
        .bind(id(6))
        .bind(id(1))
        .execute(&db)
        .await
        .unwrap();
    let new_project_profiles: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ai_profiles WHERE project_id=$1")
            .bind(id(6))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(new_project_profiles, 0);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn profiles_saved_from_presets_keep_read_only_preset_key(db: PgPool) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Preset API');
        INSERT INTO users (id,email,display_name) VALUES ('{user}','preset-api@example.test','Owner');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES ('{project}','{tenant}','Project','project');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,permissions)
        VALUES ('{tenant}','{project}','manager','Manager','manager',ARRAY['ai:manage','knowledge:manage']);
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role) VALUES ('{user}','{tenant}','{project}','{user}','manager');
    "#,tenant=id(1),user=id(2),project=id(3))).execute(&db).await.unwrap();
    for (token, scope) in [("manager", None), ("scoped", Some(vec![id(99)]))] {
        sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,inbox_scope,role,expires_at) VALUES ($1,$2,$3,$4,$5,$6,ARRAY['ai:manage','knowledge:manage'],$7,'manager',now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(3)).bind(id(2)).bind(token).bind(Sha256::digest(token.as_bytes()).to_vec()).bind(scope).execute(&db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.redis.url = None;
    config.redis.required = false;
    config.pg.migrate_on_start = false;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = ai_settings::router().with_state(state);
    let (status, body) = request(&app, Method::GET, "/api/v1/ai/profiles", "manager", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["items"],
        json!([]),
        "projects no longer get stored presets"
    );

    let input = json!({"name":"Finance agent","description":"Plans budgets","status":"draft","provider_connection_id":null,"model":null,"instructions":"Preset instructions","language":"en","max_output_tokens":2000,"public_identities":[{"language":"en","display_name":"Assistant"}]});
    let mut preset_input = input.clone();
    preset_input["preset_key"] = json!("finance");
    let (status, preset) = request(
        &app,
        Method::POST,
        "/api/v1/ai/profiles",
        "manager",
        Some(preset_input.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{preset}");
    assert_eq!(preset["preset_key"], "finance");
    preset_input["name"] = json!("Second finance agent");
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/ai/profiles",
            "manager",
            Some(preset_input)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    for key in ["universal", "operations", "unknown"] {
        let mut invalid = input.clone();
        invalid["name"] = json!(format!("Invalid {key}"));
        invalid["preset_key"] = json!(key);
        assert_eq!(
            request(
                &app,
                Method::POST,
                "/api/v1/ai/profiles",
                "manager",
                Some(invalid)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }

    let path = format!("/api/v1/ai/profiles/{}", preset["id"].as_str().unwrap());
    let mut edited = input.clone();
    edited["name"] = json!("My finance agent");
    edited["description"] = json!("Updated purpose");
    let (status, updated) =
        request(&app, Method::PATCH, &path, "manager", Some(edited.clone())).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["preset_key"], "finance");
    assert_eq!(updated["description"], "Updated purpose");
    let mut legacy = edited.clone();
    legacy.as_object_mut().unwrap().remove("description");
    let (_, preserved) = request(&app, Method::PATCH, &path, "manager", Some(legacy)).await;
    assert_eq!(preserved["description"], "Updated purpose");
    let mut spoofed = edited.clone();
    spoofed["preset_key"] = json!("sales");
    assert_eq!(
        request(&app, Method::PATCH, &path, "manager", Some(spoofed))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, Method::PATCH, &path, "scoped", Some(edited))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, scoped) = request(&app, Method::GET, "/api/v1/ai/profiles", "scoped", None).await;
    assert_eq!(scoped["items"], json!([]));

    let mut custom_input = input;
    custom_input["name"] = json!("Custom");
    let (status, custom) = request(
        &app,
        Method::POST,
        "/api/v1/ai/profiles",
        "manager",
        Some(custom_input),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{custom}");
    assert!(custom["preset_key"].is_null());
}
