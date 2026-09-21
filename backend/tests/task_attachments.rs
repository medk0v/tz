//! Screenshots and files survive task saves and obey the same visibility as the task.

use std::{
    io::{Cursor, Write},
    path::PathBuf,
};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use tz_backend::{AppState, Config, ai_tasks, attachments, config::AttachmentScanMode};
use uuid::Uuid;

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

struct Fixture {
    app: Router,
    state: AppState,
    storage: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.storage);
    }
}

async fn fixture(db: &PgPool) -> Fixture {
    sqlx::raw_sql(&format!(r"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Screenshots');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES ('{project}','{tenant}','Screenshots','screenshots');
        INSERT INTO users (id,email,display_name) VALUES
          ('{manager}','manager@example.test','Manager'), ('{employee}','employee@example.test','Employee'),
          ('{other}','other@example.test','Other'), ('{second}','second@example.test','Second manager');
        INSERT INTO project_roles (tenant_id,project_id,id,name,base_role,is_system,permissions) VALUES
          ('{tenant}','{project}','manager','Manager','manager',false,ARRAY['tasks:own','tasks:manage']),
          ('{tenant}','{project}','operator','Operator','operator',false,ARRAY['tasks:own']);
        INSERT INTO memberships (id,tenant_id,project_id,user_id,role) VALUES
          ('{manager}','{tenant}','{project}','{manager}','manager'),
          ('{employee}','{tenant}','{project}','{employee}','operator'),
          ('{other}','{tenant}','{project}','{other}','operator'),
          ('{second}','{tenant}','{project}','{second}','manager');
    ",tenant=id(1),project=id(2),manager=id(3),employee=id(4),other=id(5),second=id(6)))
        .execute(db).await.unwrap();
    for (token, user, role, permissions) in [
        ("manager", 3, "manager", vec!["tasks:own", "tasks:manage"]),
        ("employee", 4, "operator", vec!["tasks:own"]),
        ("other", 5, "operator", vec!["tasks:own"]),
        ("second", 6, "manager", vec!["tasks:own", "tasks:manage"]),
    ] {
        sqlx::query("INSERT INTO api_keys (id,tenant_id,project_id,actor_user_id,name,token_hash,permissions,role,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,now()+interval '1 hour')")
            .bind(Uuid::now_v7()).bind(id(1)).bind(id(2)).bind(id(user)).bind(token)
            .bind(Sha256::digest(token.as_bytes()).to_vec()).bind(permissions).bind(role)
            .execute(db).await.unwrap();
    }
    let mut config: Config = toml::from_str(include_str!("../Config.example.toml")).unwrap();
    config.pg.url = std::env::var("DATABASE_URL").unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.scan_mode = AttachmentScanMode::Disabled;
    config.attachments.clamav_address = None;
    let storage = std::env::temp_dir().join(format!("tzomet-screenshot-tests-{}", Uuid::now_v7()));
    config.attachments.storage_path.clone_from(&storage);
    config.openclaw.grant_directory = storage.join("grants");
    config.openclaw.action_directory = storage.join("actions");
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();
    let app = ai_tasks::router().with_state(state.clone());
    Fixture {
        app,
        state,
        storage,
    }
}

fn png() -> Vec<u8> {
    STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aDQAAAABJRU5ErkJggg==").unwrap()
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn docx(extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
        ("word/document.xml", br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Requirements</w:t></w:r></w:p></w:body></w:document>"#),
    ];
    entries.extend_from_slice(extra);
    zip(&entries)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), attachments::MAX_VIDEO_BYTES)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

async fn json_call(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let (status, _, bytes) = request(
        app,
        method,
        path,
        token,
        "application/json",
        body.to_string().into_bytes(),
    )
    .await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn upload(
    app: &Router,
    token: &str,
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> (StatusCode, Value) {
    let mut body = format!("--task-image\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: {mime}\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--task-image--\r\n");
    let (status, _, result) = request(
        app,
        Method::POST,
        "/api/v1/ai/task-attachments",
        token,
        "multipart/form-data; boundary=task-image",
        body,
    )
    .await;
    (status, serde_json::from_slice(&result).unwrap())
}

fn task_input() -> Value {
    json!({"text":"Fix the screenshot issue","schedule":{"kind":"manual"},"agent_ids":[],"assignee_ids":[id(4)]})
}

async fn upload_png(app: &Router) -> String {
    let (status, body) = upload(app, "manager", "screen.png", "image/png", &png()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn screenshots_survive_save_and_follow_task_visibility(db: PgPool) {
    let fixture = fixture(&db).await;
    let app = &fixture.app;
    let attachment = upload_png(app).await;
    let path = format!("/api/v1/ai/task-attachments/{attachment}");
    for token in ["employee", "other", "second"] {
        assert_eq!(
            request(app, Method::GET, &path, token, "", vec![]).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let mut input = task_input();
    input["attachment_ids"] = json!([attachment]);
    let (status, task) = json_call(
        app,
        Method::POST,
        "/api/v1/ai/tasks",
        "manager",
        input.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    assert_eq!(task["attachment_ids"], input["attachment_ids"]);
    for token in ["manager", "employee", "second"] {
        let (status, headers, bytes) = request(app, Method::GET, &path, token, "", vec![]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, png());
        assert!(
            headers["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
        assert_eq!(headers["x-content-type-options"], "nosniff");
    }
    assert_eq!(
        request(app, Method::GET, &path, "other", "", vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, list) = json_call(
        app,
        Method::GET,
        "/api/v1/ai/tasks",
        "employee",
        Value::Null,
    )
    .await;
    assert_eq!(list["items"][0]["attachment_ids"], input["attachment_ids"]);
    let task_path = format!("/api/v1/ai/tasks/{}", task["id"].as_str().unwrap());
    let (status, updated) =
        json_call(app, Method::PATCH, &task_path, "manager", task_input()).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["attachment_ids"], input["attachment_ids"]);
    // The cleanup job must retain screenshots attached to a task.
    sqlx::query("UPDATE ai_task_attachments SET created_at=now()-interval '2 days'")
        .execute(&db)
        .await
        .unwrap();
    attachments::purge_stale_storage(&fixture.state)
        .await
        .unwrap();
    assert_eq!(
        request(app, Method::GET, &path, "employee", "", vec![])
            .await
            .0,
        StatusCode::OK
    );
    input["attachment_ids"] = json!([]);
    let (status, updated) = json_call(app, Method::PATCH, &task_path, "manager", input).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["attachment_ids"], json!([]));
    assert_eq!(
        request(app, Method::GET, &path, "manager", "", vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn uploads_reject_invalid_files_and_unavailable_draft_ids(db: PgPool) {
    let fixture = fixture(&db).await;
    let app = &fixture.app;
    assert_eq!(
        upload(app, "employee", "screen.png", "image/png", &png())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        upload(app, "manager", "screen.png", "image/png", b"not an image")
            .await
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        upload(app, "manager", "screen.png", "image/svg+xml", b"<svg/>")
            .await
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let attachment = upload_png(app).await;
    let mut input = task_input();
    input["attachment_ids"] = json!([attachment]);
    assert_eq!(
        json_call(
            app,
            Method::POST,
            "/api/v1/ai/tasks",
            "second",
            input.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    input["attachment_ids"] = json!([attachment, attachment]);
    assert_eq!(
        json_call(
            app,
            Method::POST,
            "/api/v1/ai/tasks",
            "manager",
            input.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    input["attachment_ids"] = json!((0..6).map(|_| Uuid::now_v7()).collect::<Vec<_>>());
    assert_eq!(
        json_call(
            app,
            Method::POST,
            "/api/v1/ai/tasks",
            "manager",
            input.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    input["attachment_ids"] = json!([attachment, Uuid::now_v7()]);
    assert_eq!(
        json_call(
            app,
            Method::POST,
            "/api/v1/ai/tasks",
            "manager",
            input.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // Failed saves must roll back the first attachment so the draft can be retried.
    let linked: Option<Uuid> =
        sqlx::query_scalar("SELECT task_id FROM ai_task_attachments WHERE id=$1")
            .bind(Uuid::parse_str(&attachment).unwrap())
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(linked, None);
    sqlx::query("UPDATE ai_task_attachments SET created_at=now()-interval '25 hours'")
        .execute(&db)
        .await
        .unwrap();
    input["attachment_ids"] = json!([attachment]);
    assert_eq!(
        json_call(app, Method::POST, "/api/v1/ai/tasks", "manager", input)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    attachments::purge_stale_storage(&fixture.state)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_task_attachments")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn files_survive_save_with_original_names_and_task_visibility(db: PgPool) {
    let fixture = fixture(&db).await;
    let app = &fixture.app;
    let document = docx(&[]);
    let archive = zip(&[("notes.txt", b"notes"), ("requirements.docx", &document)]);
    let files: [(&str, &str, &[u8], &str); 5] = [
        (
            "Требования + 1.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            &document,
            "application/octet-stream",
        ),
        (
            "notes.txt",
            "text/plain",
            b"notes",
            "application/octet-stream",
        ),
        (
            "archive.zip",
            "application/zip",
            &archive,
            "application/octet-stream",
        ),
        (
            "document.pdf",
            "application/pdf",
            b"%PDF-1.7\nbody\n%%EOF\n",
            "application/pdf",
        ),
        (
            "clip.mp4",
            "video/mp4",
            &[0, 0, 0, 12, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm'],
            "video/mp4",
        ),
    ];
    let mut ids = Vec::new();
    for (name, mime, bytes, _) in &files {
        let (status, body) = upload(app, "manager", name, mime, bytes).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        ids.push(body["id"].as_str().unwrap().to_owned());
    }
    let mut input = task_input();
    input["attachment_ids"] = json!(ids);
    let (status, task) = json_call(app, Method::POST, "/api/v1/ai/tasks", "manager", input).await;
    assert_eq!(status, StatusCode::CREATED, "{task}");
    assert_eq!(task["attachment_ids"], json!(ids));
    for (id, (name, _, content, mime)) in ids.iter().zip(&files) {
        let path = format!("/api/v1/ai/task-attachments/{id}");
        let (status, headers, bytes) =
            request(app, Method::GET, &path, "employee", "", vec![]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(&bytes, content);
        assert_eq!(headers["content-type"], *mime);
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(
            headers["content-security-policy"],
            "sandbox; default-src 'none'"
        );
        let disposition = headers["content-disposition"].to_str().unwrap();
        assert!(disposition.starts_with("attachment;"));
        assert_eq!(
            percent_encoding::percent_decode_str(
                disposition.split("filename*=UTF-8''").nth(1).unwrap()
            )
            .decode_utf8()
            .unwrap(),
            *name
        );
        assert_eq!(
            request(app, Method::GET, &path, "other", "", vec![])
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn files_use_their_own_size_limits_and_reject_active_formats(db: PgPool) {
    let fixture = fixture(&db).await;
    let app = &fixture.app;
    let mut pdf = b"%PDF-1.7\n".to_vec();
    pdf.resize(attachments::MAX_IMAGE_BYTES + 1, b' ');
    pdf.extend_from_slice(b"\n%%EOF\n");
    assert_eq!(
        upload(app, "manager", "large.pdf", "application/pdf", &pdf)
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        upload(
            app,
            "manager",
            "too-large.txt",
            "text/plain",
            &vec![b'x'; attachments::MAX_FILE_BYTES + 1]
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        upload(app, "manager", "empty.txt", "text/plain", b"")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        upload(app, "manager", "../notes.txt", "text/plain", b"notes")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, body) = upload(
        app,
        "manager",
        "page.html",
        "text/html",
        b"<script>alert(1)</script>",
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn unsafe_business_uploads_never_create_attachment_records(db: PgPool) {
    let fixture = fixture(&db).await;
    for (name, bytes) in [
        ("setup.exe", b"MZpayload".to_vec()),
        ("fake.docx", b"not a Word document".to_vec()),
        ("renamed.txt", b"MZ\x00\x01binary".to_vec()),
        ("macro.docx", docx(&[("word/vbaProject.bin", b"macro")])),
        ("slip.zip", zip(&[("../notes.txt", b"notes")])),
        ("program.zip", zip(&[("program.exe", b"MZbinary")])),
        (
            "nested.zip",
            zip(&[("nested.zip", &zip(&[("notes.txt", b"notes")]))]),
        ),
    ] {
        let (status, body) = upload(
            &fixture.app,
            "manager",
            name,
            "application/octet-stream",
            &bytes,
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{name}: {body}");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_task_attachments")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    for directory in ["quarantine", "objects"] {
        assert_eq!(
            std::fs::read_dir(fixture.storage.join(directory))
                .unwrap()
                .count(),
            0
        );
    }
}

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn screenshots_respect_the_workspace_storage_quota(db: PgPool) {
    let mut fixture = fixture(&db).await;
    std::sync::Arc::make_mut(&mut fixture.state.config)
        .attachments
        .max_tenant_bytes = u64::try_from(png().len()).unwrap();
    let app = ai_tasks::router().with_state(fixture.state.clone());
    upload_png(&app).await;
    assert_eq!(
        upload(&app, "manager", "screen.png", "image/png", &png())
            .await
            .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_task_attachments")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(remaining, 1);
}
