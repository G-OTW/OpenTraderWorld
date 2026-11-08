//! The session and same-origin guards, and the status codes handlers answer with.

use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;

use super::{app, request, send, signed_in, state_over};

#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_protected_route_needs_a_live_session(pool: PgPool) {
    let token = signed_in(&pool).await;
    let app = app(state_over(pool.clone()).await);

    let (status, body) = send(&app, request("GET", "/api/todos", None, None, &[])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    insta::assert_yaml_snapshot!("unauthorized_body", body);

    let (status, _) = send(&app, request("GET", "/api/todos", None, Some("forged"), &[])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "an unknown token is no session");

    let (status, body) = send(&app, request("GET", "/api/todos", None, Some(&token), &[])).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "todos": [] }));

    // The same token once its session expired.
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'").execute(&pool).await.unwrap();
    let (status, _) = send(&app, request("GET", "/api/todos", None, Some(&token), &[])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A write from another site is refused as such, before the session is even looked at,
/// and nothing is written. A GET from anywhere still answers: reads are not the attack.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_cross_site_write_is_refused_and_writes_nothing(pool: PgPool) {
    let token = signed_in(&pool).await;
    let app = app(state_over(pool.clone()).await);
    let body = r#"{"name":"from elsewhere"}"#;

    let cross = [("sec-fetch-site", "cross-site")];
    let (status, err) = send(&app, request("POST", "/api/todos", Some(body), Some(&token), &cross)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["code"], "cross_origin");

    let foreign = [("origin", "https://evil.example")];
    let (status, err) = send(&app, request("POST", "/api/todos", Some(body), Some(&token), &foreign)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["code"], "cross_origin");

    // Refused before auth: no cookie gets the same answer, not a 401.
    let (status, _) = send(&app, request("POST", "/api/todos", Some(body), None, &cross)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, list) = send(&app, request("GET", "/api/todos", None, Some(&token), &cross)).await;
    assert_eq!(list, json!({ "todos": [] }), "nothing was written");

    let same = [("origin", "http://otw.test"), ("sec-fetch-site", "same-origin")];
    let (status, _) = send(&app, request("POST", "/api/todos", Some(body), Some(&token), &same)).await;
    assert_eq!(status, StatusCode::OK, "the app's own page writes");
}

/// Validation and lookup errors carry the right status and a plain `error` sentence, and
/// a body the extractor cannot read never reaches the handler.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn handlers_answer_bad_input_with_a_4xx_and_a_reason(pool: PgPool) {
    let token = signed_in(&pool).await;
    let app = app(state_over(pool.clone()).await);
    let t = Some(token.as_str());
    let missing = uuid::Uuid::new_v4();

    let mut seen = serde_json::Map::new();
    let cases: Vec<(&str, &str, String, Option<&str>, StatusCode)> = vec![
        ("blank_name", "POST", "/api/todos".into(), Some(r#"{"name":"   "}"#), StatusCode::BAD_REQUEST),
        ("unknown_id", "GET", format!("/api/todos/{missing}"), None, StatusCode::NOT_FOUND),
        ("unknown_id_on_delete", "DELETE", format!("/api/todos/{missing}"), None, StatusCode::NOT_FOUND),
        (
            "unknown_category",
            "POST",
            "/api/journal/trades".into(),
            Some(r#"{"category_id":"00000000-0000-0000-0000-00000000c0de","ticker":"AAPL"}"#),
            StatusCode::BAD_REQUEST,
        ),
        (
            "unknown_asset_class",
            "POST",
            "/api/journal/trades".into(),
            Some(r#"{"category_id":"00000000-0000-0000-0000-00000000c0de","asset_class":"tulips"}"#),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (name, method, path, body, want) in cases {
        let (status, err) = send(&app, request(method, &path, body, t, &[])).await;
        assert_eq!(status, want, "{name}: {err}");
        assert!(err["error"].as_str().is_some_and(|s| !s.is_empty()), "{name}: {err}");
        seen.insert(name.into(), err);
    }
    insta::assert_yaml_snapshot!("handler_error_bodies", seen);

    // Bodies the extractor refuses: malformed JSON, a wrong type, no JSON content type.
    let (status, _) = send(&app, request("POST", "/api/todos", Some("{not json"), t, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(&app, request("POST", "/api/todos", Some(r#"{"name":42}"#), t, &[])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut plain = request("POST", "/api/todos", None, t, &[]);
    *plain.body_mut() = axum::body::Body::from(r#"{"name":"x"}"#);
    let (status, _) = send(&app, plain).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // A path id that is not a UUID.
    let (status, _) = send(&app, request("GET", "/api/todos/not-a-uuid", None, t, &[])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, list) = send(&app, request("GET", "/api/todos", None, t, &[])).await;
    assert_eq!(list, json!({ "todos": [] }), "no refused request wrote anything");
}

/// A name the database already holds is the caller's conflict, never a server error.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_duplicate_strategy_name_is_a_4xx(pool: PgPool) {
    let token = signed_in(&pool).await;
    let app = app(state_over(pool.clone()).await);
    let body = json!({
        "name": "twice",
        "settings": {
            "mode": "long",
            "long": { "entry": { "logic": "all", "conditions": [] } },
            "sizing": { "mode": "fixed_qty", "qty": 1.0 },
            "starting_capital": 10000.0
        }
    })
    .to_string();
    let (status, err) = send(&app, request("POST", "/api/backtest/strategies", Some(&body), Some(&token), &[])).await;
    assert_eq!(status, StatusCode::OK, "{err}");
    let (status, err) = send(&app, request("POST", "/api/backtest/strategies", Some(&body), Some(&token), &[])).await;
    assert!(status.is_client_error(), "got {status}: {err}");
}

/// The public health route answers without a session and names no build.
#[sqlx::test(migrations = "../otw-store/migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn health_is_public_and_anonymous_callers_get_no_version(pool: PgPool) {
    let token = signed_in(&pool).await;
    let app = app(state_over(pool).await);
    let (status, body) = send(&app, request("GET", "/api/health", None, None, &[])).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }), "an anonymous caller is told the status only");
    let (_, body) = send(&app, request("GET", "/api/health", None, Some(&token), &[])).await;
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"), "the owner sees the build: {body}");
}
