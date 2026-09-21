use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, auth, operator, operator_presence};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(160_000 + value)
}

async fn fixture(db: &PgPool) -> Router {
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id, name) VALUES ('{tenant}', 'Project windows'), ('{foreign_tenant}', 'Foreign');
        INSERT INTO users (id, email, display_name) VALUES ('{user}', 'windows@example.test', 'Operator');
        INSERT INTO projects (id, tenant_id, name, slug, status) VALUES
            ('{a}', '{tenant}', 'Project A', 'a', 'active'),
            ('{b}', '{tenant}', 'Project B', 'b', 'active'),
            ('{inaccessible}', '{tenant}', 'Inaccessible', 'inaccessible', 'active'),
            ('{disabled}', '{tenant}', 'Disabled', 'disabled', 'disabled'),
            ('{foreign}', '{foreign_tenant}', 'Foreign', 'foreign', 'active');
        INSERT INTO project_roles (tenant_id, project_id, id, name, base_role, permissions) VALUES
            ('{tenant}', '{a}', 'lead', 'Lead', 'manager',
                ARRAY['projects:read','conversations:read','conversations:reply','channels:read','channels:manage']),
            ('{tenant}', '{b}', 'operator', 'Operator', 'operator',
                ARRAY['projects:read','conversations:read','conversations:reply','channels:read']),
            ('{tenant}', '{disabled}', 'operator', 'Operator', 'operator', ARRAY['projects:read']);
        INSERT INTO memberships (id, tenant_id, project_id, user_id, role, director_access) VALUES
            ('{member_a}', '{tenant}', '{a}', '{user}', 'lead', true),
            ('{member_b}', '{tenant}', '{b}', '{user}', 'operator', false),
            ('{member_disabled}', '{tenant}', '{disabled}', '{user}', 'operator', false);
        INSERT INTO departments (id, tenant_id, project_id, name) VALUES
            ('{a_default}', '{tenant}', '{a}', 'A default'), ('{a_other}', '{tenant}', '{a}', 'A other'),
            ('{b_default}', '{tenant}', '{b}', 'B default'), ('{b_other}', '{tenant}', '{b}', 'B other');
        UPDATE projects SET default_department_id = '{a_default}' WHERE id = '{a}';
        UPDATE projects SET default_department_id = '{b_default}' WHERE id = '{b}';
        INSERT INTO inboxes (id, tenant_id, project_id, department_id, name) VALUES
            ('{inbox_a}', '{tenant}', '{a}', '{a_default}', 'A default'),
            ('{inbox_a_other}', '{tenant}', '{a}', '{a_other}', 'A other'),
            ('{inbox_b}', '{tenant}', '{b}', '{b_default}', 'B default'),
            ('{inbox_b_other}', '{tenant}', '{b}', '{b_other}', 'B other');
        INSERT INTO operator_sessions (id, tenant_id, project_id, user_id, membership_id,
            token_hash, csrf_token_hash, idle_expires_at, absolute_expires_at) VALUES
            ('{session}', '{tenant}', '{a}', '{user}', '{member_a}',
             decode('{token_hash:x}', 'hex'), decode('{csrf_hash:x}', 'hex'),
             now() + interval '1 hour', now() + interval '2 hours');
        "#,
        tenant = id(1), foreign_tenant = id(2), user = id(3), session = id(4),
        a = id(10), a_default = id(11), a_other = id(12), member_a = id(13),
        inbox_a = id(14), inbox_a_other = id(15),
        b = id(20), b_default = id(21), b_other = id(22), member_b = id(23),
        inbox_b = id(24), inbox_b_other = id(25),
        inaccessible = id(30), disabled = id(31), foreign = id(32), member_disabled = id(33),
        token_hash = Sha256::digest(b"project-windows"), csrf_hash = Sha256::digest(b"windows-csrf"),
    ))
    .execute(db)
    .await
    .unwrap();
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    auth::router()
        .merge(operator::router())
        .merge(operator_presence::router())
        .with_state(state)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    project: Option<Uuid>,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let project = project.map(|value| value.to_string());
    raw_request(app, method, path, project.as_deref(), input).await
}

async fn raw_request(
    app: &Router,
    method: Method,
    path: &str,
    project: Option<&str>,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::COOKIE,
            "tzomet_session=project-windows; tzomet_csrf=windows-csrf",
        )
        .header("x-csrf-token", "windows-csrf");
    if let Some(project) = project {
        builder = builder.header("X-Tzomet-Project-Id", project);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(input.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 100_000).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn actor(app: &Router, project: Option<Uuid>) -> Value {
    let (status, body) = request(app, Method::GET, "/api/v1/me", project, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn project_headers_keep_membership_permissions_and_inboxes_independent(db: PgPool) {
    let app = fixture(&db).await;
    for project in [id(10), id(20), id(10), id(20)] {
        let current = actor(&app, Some(project)).await;
        let is_a = project == id(10);
        assert_eq!(current["project_id"], json!(project));
        assert_eq!(current["role"], if is_a { "manager" } else { "operator" });
        assert_eq!(current["role_id"], if is_a { "lead" } else { "operator" });
        assert_eq!(
            current["department_id"],
            json!(if is_a { id(11) } else { id(21) })
        );
        assert_eq!(
            current["permissions"]
                .as_array()
                .unwrap()
                .contains(&json!("channels:manage")),
            is_a
        );
        let (status, inboxes) =
            request(&app, Method::GET, "/api/v1/inboxes", Some(project), None).await;
        assert_eq!(status, StatusCode::OK, "{inboxes}");
        assert_eq!(inboxes["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            inboxes["items"][0]["id"],
            json!(if is_a { id(14) } else { id(24) })
        );
    }
    let (a, b) = tokio::join!(actor(&app, Some(id(10))), actor(&app, Some(id(20))));
    assert_eq!(a["project_id"], json!(id(10)));
    assert_eq!(b["project_id"], json!(id(20)));
    assert_eq!(actor(&app, None).await["project_id"], json!(id(10)));
    let legacy: (Uuid, Uuid) =
        sqlx::query_as("SELECT project_id, membership_id FROM operator_sessions WHERE id=$1")
            .bind(id(4))
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        legacy,
        (id(10), id(13)),
        "request headers must not rewrite the shared session selection"
    );

    sqlx::query("UPDATE project_roles SET permissions=array_remove(permissions,'conversations:reply') WHERE project_id=$1 AND id='operator'")
        .bind(id(20)).execute(&db).await.unwrap();
    let b = actor(&app, Some(id(20))).await;
    assert!(
        !b["permissions"]
            .as_array()
            .unwrap()
            .contains(&json!("conversations:reply"))
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/operator-presence",
            Some(id(20)),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/operator-presence",
            Some(id(10)),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn project_headers_reject_invalid_inaccessible_and_revoked_scopes(db: PgPool) {
    let app = fixture(&db).await;
    let (status, body) =
        raw_request(&app, Method::GET, "/api/v1/me", Some("not-a-uuid"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    for project in [id(30), id(31), id(32), id(999)] {
        let (status, body) = request(&app, Method::GET, "/api/v1/me", Some(project), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{project}: {body}");
    }
    sqlx::query("UPDATE memberships SET revoked_at=now() WHERE id=$1")
        .bind(id(23))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", Some(id(20)), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(actor(&app, Some(id(10))).await["role_id"], "lead");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn department_and_director_preferences_survive_other_project_windows(db: PgPool) {
    let app = fixture(&db).await;
    for (project, department) in [(id(10), id(12)), (id(20), id(22))] {
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/auth/department",
            Some(project),
            Some(json!({"department_id":department})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["project_id"], json!(project));
        assert_eq!(body["department_id"], json!(department));
    }
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/auth/department",
        Some(id(10)),
        Some(json!({"department_id":id(22)})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(
        actor(&app, Some(id(10))).await["department_id"],
        json!(id(12))
    );
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/auth/department",
        Some(id(10)),
        Some(json!({"department_id":null})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["is_director"], true);
    assert_eq!(
        actor(&app, Some(id(20))).await["department_id"],
        json!(id(22))
    );
    let (status, body) = request(
        &app,
        Method::POST,
        "/api/v1/auth/department",
        Some(id(20)),
        Some(json!({"department_id":null})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    for (source, target) in [(id(10), id(20)), (id(20), id(10))] {
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/auth/project",
            Some(source),
            Some(json!({"project_id":target})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["project_id"], json!(target));
        assert_eq!(actor(&app, Some(id(10))).await["is_director"], true);
        let b = actor(&app, Some(id(20))).await;
        assert_eq!(b["is_director"], false);
        assert_eq!(b["department_id"], json!(id(22)));
    }
    let (_, a_inboxes) = request(&app, Method::GET, "/api/v1/inboxes", Some(id(10)), None).await;
    let (_, b_inboxes) = request(&app, Method::GET, "/api/v1/inboxes", Some(id(20)), None).await;
    assert_eq!(a_inboxes["items"].as_array().unwrap().len(), 2);
    assert_eq!(b_inboxes["items"].as_array().unwrap().len(), 1);
    assert_eq!(b_inboxes["items"][0]["id"], json!(id(25)));
}

async fn online(db: &PgPool, project: Uuid, inbox: Uuid) -> bool {
    operator_presence::inbox_has_online_operator(db, id(1), project, inbox)
        .await
        .unwrap()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn presence_is_independent_and_rechecks_membership_and_session_lifetime(db: PgPool) {
    let app = fixture(&db).await;
    sqlx::query("UPDATE operator_sessions SET presence_last_seen_at=now() WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    assert!(
        online(&db, id(10), id(14)).await,
        "legacy presence remains available before a project workspace exists"
    );
    assert!(!online(&db, id(20), id(24)).await);
    for project in [id(20), id(10)] {
        let (status, body) = request(
            &app,
            Method::POST,
            "/api/v1/operator-presence",
            Some(project),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    assert!(online(&db, id(10), id(14)).await);
    assert!(online(&db, id(20), id(24)).await);
    assert!(!online(&db, id(10), id(15)).await);
    let (status, body) = request(
        &app,
        Method::DELETE,
        "/api/v1/operator-presence",
        Some(id(10)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    sqlx::query("UPDATE operator_sessions SET presence_last_seen_at=now() WHERE id=$1")
        .bind(id(4))
        .execute(&db)
        .await
        .unwrap();
    assert!(
        !online(&db, id(10), id(14)).await,
        "cleared workspace presence must not fall back to the shared session heartbeat"
    );
    assert!(online(&db, id(20), id(24)).await);
    sqlx::query("UPDATE memberships SET revoked_at=now() WHERE id=$1")
        .bind(id(23))
        .execute(&db)
        .await
        .unwrap();
    assert!(!online(&db, id(20), id(24)).await);
    sqlx::query("UPDATE memberships SET revoked_at=NULL WHERE id=$1")
        .bind(id(23))
        .execute(&db)
        .await
        .unwrap();
    assert!(online(&db, id(20), id(24)).await);
    sqlx::query(
        "UPDATE operator_sessions SET idle_expires_at=now()-interval '1 minute' WHERE id=$1",
    )
    .bind(id(4))
    .execute(&db)
    .await
    .unwrap();
    assert!(!online(&db, id(20), id(24)).await);
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", Some(id(20)), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE operator_sessions SET idle_expires_at=now()+interval '1 hour', revoked_at=now() WHERE id=$1")
        .bind(id(4)).execute(&db).await.unwrap();
    assert!(!online(&db, id(20), id(24)).await);
    assert_eq!(
        request(&app, Method::GET, "/api/v1/me", Some(id(20)), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
