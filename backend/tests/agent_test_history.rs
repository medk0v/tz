use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, provider_reply::agent_tests};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

async fn clear(app: &Router, profile: Uuid, scenario: Uuid, token: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!(
                    "/api/v1/ai/profiles/{profile}/tests/{scenario}/runs"
                ))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn clears_only_completed_runs_of_the_scoped_test_including_older_history(db: PgPool) {
    sqlx::raw_sql(&format!(r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'History');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'history@example.test', 'Manager');
        INSERT INTO projects (id, tenant_id, name, slug) VALUES
          ('{project}', '{tenant}', 'Project', 'project'),
          ('{other_project}', '{tenant}', 'Other', 'other');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions)
          VALUES ('{tenant}', '{project}', 'manager', 'Manager', 'manager', ARRAY['ai:manage']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role)
          VALUES ('{user}', '{tenant}', '{project}', '{user}', 'manager');
        INSERT INTO ai_profiles (id, tenant_id, project_id, name, instructions, created_by) VALUES
          ('{profile}', '{tenant}', '{project}', 'Agent', '', '{user}'),
          ('{other_profile}', '{tenant}', '{project}', 'Other agent', '', '{user}'),
          ('{foreign_profile}', '{tenant}', '{other_project}', 'Other project', '', '{user}');
        INSERT INTO ai_test_scenarios (id, tenant_id, project_id, profile_id, scenario) VALUES
          ('{scenario}', '{tenant}', '{project}', '{profile}', '{{}}'),
          ('{neighbor}', '{tenant}', '{project}', '{profile}', '{{}}'),
          ('{other_scenario}', '{tenant}', '{project}', '{other_profile}', '{{}}'),
          ('{foreign_scenario}', '{tenant}', '{other_project}', '{foreign_profile}', '{{}}');
    "#,
        tenant = id(1), project = id(2), user = id(3), profile = id(4), scenario = id(5),
        neighbor = id(6), other_profile = id(7), other_scenario = id(8), other_project = id(9),
        foreign_profile = id(10), foreign_scenario = id(11),
    )).execute(&db).await.unwrap();
    for (token, permissions, inbox_scope) in [
        ("manager", vec!["ai:manage"], None),
        ("reader", vec!["ai:read"], None),
        ("scoped", vec!["ai:manage"], Some(vec![id(99)])),
    ] {
        sqlx::query("INSERT INTO api_keys (id, tenant_id, project_id, actor_user_id, name, token_hash, permissions, inbox_scope, role, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'manager', now() + interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(3)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(permissions).bind(inbox_scope)
            .execute(&db).await.unwrap();
    }
    for n in 0..59 {
        let (project, profile, scenario, status) = match n {
            55 => (id(2), id(4), id(5), "running"),
            56 => (id(2), id(4), id(6), "passed"),
            57 => (id(2), id(7), id(8), "passed"),
            58 => (id(9), id(10), id(11), "passed"),
            _ => (id(2), id(4), id(5), "passed"),
        };
        sqlx::query("INSERT INTO ai_test_runs (id, tenant_id, project_id, profile_id, scenario_id, scenario_revision, mode, status, fingerprint, snapshot, created_by) VALUES ($1, $2, $3, $4, $5, 1, 'fixtures', $6, 'test', '{}', $7)")
            .bind(Uuid::now_v7()).bind(id(1)).bind(project).bind(profile).bind(scenario)
            .bind(status).bind(id(3)).execute(&db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = agent_tests::router().with_state(state);

    for (profile, scenario, token, expected_status) in [
        (id(4), id(5), "reader", StatusCode::FORBIDDEN),
        (id(4), id(5), "scoped", StatusCode::NOT_FOUND),
        (id(10), id(11), "manager", StatusCode::NOT_FOUND),
    ] {
        let (status, _) = clear(&app, profile, scenario, token).await;
        assert_eq!(status, expected_status);
    }
    // A scenario from another profile cannot be cleared through the current profile.
    let (status, result) = clear(&app, id(4), id(8), "manager").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["deleted"], 0);
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_test_runs")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(before, 59);

    let (status, result) = clear(&app, id(4), id(5), "manager").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["deleted"], 55);
    let remaining: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT scenario_id, status FROM ai_test_runs ORDER BY scenario_id")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        remaining,
        vec![
            (id(5), "running".into()),
            (id(6), "passed".into()),
            (id(8), "passed".into()),
            (id(11), "passed".into()),
        ]
    );
    let scenarios: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ai_test_scenarios WHERE deleted_at IS NULL")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(scenarios, 4);
    let (_, result) = clear(&app, id(4), id(5), "manager").await;
    assert_eq!(result["deleted"], 0);
}
