//! HTTP router composition.

use axum::{
    Router,
    body::Body,
    http::{HeaderName, HeaderValue, Method, header},
    response::Response,
    routing::get,
};
use tower_http::{
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

use crate::{
    AppState, ai_settings, ai_tasks, attachments, auth, avatars, conversations, email, health,
    inbox_routing, integrations, knowledge, operator, operator_presence, projects, quality,
    realtime, reply_templates, teams, telegram_bot,
};

/// Builds the API router from domain-owned route groups.
///
/// # Panics
///
/// Panics when a configured CORS origin is not a valid HTTP header value.
pub fn router(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    let mut router = Router::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(crate::mobile_push::router())
        .merge(attachments::router())
        .merge(avatars::router())
        .merge(ai_settings::router())
        .merge(crate::ai_skills::router())
        .merge(crate::ai_api::router())
        .merge(crate::ai_orchestration::router())
        .merge(crate::provider_reply::agent_tests::router())
        .merge(ai_tasks::router())
        .merge(conversations::router())
        .merge(email::router())
        .merge(inbox_routing::router())
        .merge(integrations::router())
        .merge(knowledge::router())
        .merge(crate::notes::router())
        .merge(crate::note_databases::router())
        .merge(crate::note_shares::router())
        .merge(operator::router())
        .merge(crate::custom_ai_channels::router())
        .merge(operator_presence::router())
        .merge(projects::router())
        .merge(crate::departments::router())
        .merge(crate::company_structure::router())
        .merge(crate::job_positions::router())
        .merge(crate::resource_visibility::router())
        .merge(quality::router())
        .merge(reply_templates::router())
        .merge(teams::router())
        .merge(telegram_bot::router())
        .merge(realtime::router())
        .route("/openapi.yaml", get(openapi))
        .with_state(state.clone())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));

    if !state.config.server.allowed_origins.is_empty() {
        let origins = state
            .config
            .server
            .allowed_origins
            .iter()
            .map(|origin| HeaderValue::from_str(origin).expect("validated CORS origin"))
            .collect::<Vec<_>>();
        router = router.layer(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_credentials(true)
                .expose_headers([header::CONTENT_DISPOSITION])
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers([
                    header::AUTHORIZATION,
                    header::CONTENT_TYPE,
                    header::ORIGIN,
                    auth::csrf_header_name(),
                    HeaderName::from_static("idempotency-key"),
                    HeaderName::from_static("x-support-rating-token"),
                    HeaderName::from_static(crate::config::PROJECT_HEADER),
                ]),
        );
    }

    router
}

async fn openapi() -> Response {
    let body = Body::from(include_str!("../openapi/openapi.yaml"));
    Response::builder()
        .header(header::CONTENT_TYPE, "application/yaml; charset=utf-8")
        .body(body)
        .expect("static OpenAPI response must be valid")
}
