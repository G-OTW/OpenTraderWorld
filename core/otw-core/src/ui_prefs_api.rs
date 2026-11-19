//! `/api/ui-prefs`: the user's interface preferences (theme, privacy mask, rail widths,
//! picks), one JSON value per key, so they follow the user to another browser.
//!
//! The frontend's `prefs.svelte.js` is the only client: it reads the whole map once at
//! boot, writes one key at a time (debounced), and uses the bulk write once to upload what
//! an existing browser held in localStorage before this table existed.
//!
//! Not exposed over MCP: it is screen state, nothing an agent needs to read or change.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;

use otw_store::ui_prefs as store;

use crate::{ApiError, AppState};

/// Longest key accepted (the leading character plus 99 more).
const KEY_MAX: usize = 100;
/// Largest value accepted, measured as serialized JSON.
const VALUE_MAX: usize = 256 * 1024;
/// Most keys one bulk write may carry.
const BULK_MAX: usize = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/ui-prefs", get(list).put(put_many))
        .route("/api/ui-prefs/{key}", put(put_one).delete(remove))
}

/// A key is 1 to 100 characters of letters, digits, `.`, `_` and `-`, starting with a
/// letter or a digit. Upper case is allowed because the existing localStorage names carry
/// it (`otw.histviz.railW`) and the keys stay those names.
fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else { return false };
    key.len() <= KEY_MAX
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn check_key(key: &str) -> Result<(), ApiError> {
    if valid_key(key) {
        Ok(())
    } else {
        Err(ApiError::bad_request(&format!(
            "invalid preference key {key:?}: use 1 to {KEY_MAX} letters, digits, '.', '_' or '-', starting with a letter or a digit"
        )))
    }
}

/// A value is any JSON but `null` (clearing a preference is a DELETE), at most
/// `VALUE_MAX` bytes once serialized.
fn check_value(key: &str, value: &Value) -> Result<(), ApiError> {
    if value.is_null() {
        return Err(ApiError::bad_request(&format!(
            "preference {key:?} has a null value: DELETE the key to clear it"
        )));
    }
    let size = serde_json::to_vec(value).map(|v| v.len()).unwrap_or(usize::MAX);
    if size > VALUE_MAX {
        return Err(ApiError::bad_request(&format!(
            "preference {key:?} is {size} bytes, the limit is {VALUE_MAX}"
        )));
    }
    Ok(())
}

/// `{ "value": ... }` to the value, checked.
fn one_from_body(key: &str, body: &Value) -> Result<Value, ApiError> {
    check_key(key)?;
    let value = body
        .get("value")
        .ok_or_else(|| ApiError::bad_request("the body must be { \"value\": ... }"))?;
    check_value(key, value)?;
    Ok(value.clone())
}

/// `{ "prefs": { key: value, ... } }` to the map, every key and value checked: one bad
/// entry refuses the whole call, so a bulk write lands whole or not at all.
fn many_from_body(body: &Value) -> Result<BTreeMap<String, Value>, ApiError> {
    let prefs = body
        .get("prefs")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::bad_request("the body must be { \"prefs\": { key: value, ... } }"))?;
    if prefs.len() > BULK_MAX {
        return Err(ApiError::bad_request(&format!(
            "{} preferences in one call, the limit is {BULK_MAX}: split the upload",
            prefs.len()
        )));
    }
    let mut out = BTreeMap::new();
    for (key, value) in prefs {
        check_key(key)?;
        check_value(key, value)?;
        out.insert(key.clone(), value.clone());
    }
    Ok(out)
}

async fn list_prefs(pool: &PgPool) -> Result<Value, ApiError> {
    Ok(json!({ "prefs": store::all(pool).await? }))
}

async fn put_one_pref(pool: &PgPool, key: &str, body: &Value) -> Result<Value, ApiError> {
    let value = one_from_body(key, body)?;
    store::set(pool, key, &value).await?;
    Ok(json!({ "key": key, "value": value }))
}

async fn put_many_prefs(pool: &PgPool, body: &Value) -> Result<Value, ApiError> {
    let prefs = many_from_body(body)?;
    store::set_many(pool, &prefs).await?;
    Ok(json!({ "saved": prefs.len() }))
}

async fn remove_pref(pool: &PgPool, key: &str) -> Result<Value, ApiError> {
    check_key(key)?;
    let deleted = store::delete(pool, key).await?;
    Ok(json!({ "deleted": deleted }))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(list_prefs(&state.pool).await?))
}

async fn put_one(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(put_one_pref(&state.pool, &key, &body).await?))
}

async fn put_many(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(put_many_prefs(&state.pool, &body).await?))
}

async fn remove(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(remove_pref(&state.pool, &key).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T>(r: Result<T, ApiError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("refused: {}", e.message()),
        }
    }

    fn status<T>(r: Result<T, ApiError>) -> u16 {
        match r {
            Ok(_) => panic!("accepted"),
            Err(e) => e.status_code().as_u16(),
        }
    }

    #[test]
    fn the_existing_local_storage_names_are_valid_keys() {
        for key in [
            "otw-theme",
            "otw.privacy.hidden",
            "otw.alert.sound",
            "otw.rail.open",
            "otw.rail.folded",
            "otw.histviz.railW",
            "otw.histviz.railOpen",
            "otw.histviz.rail.v1",
            "otw_x",
            "0",
        ] {
            assert!(valid_key(key), "{key}");
        }
        assert!(valid_key(&"a".repeat(KEY_MAX)));
    }

    #[test]
    fn a_key_outside_the_charset_or_too_long_is_refused() {
        for key in ["", ".otw", "-otw", "_otw", "otw theme", "otw/theme", "otw:theme", "otw\u{e9}", "otw\n"] {
            assert!(!valid_key(key), "{key:?}");
        }
        assert!(!valid_key(&"a".repeat(KEY_MAX + 1)));
    }

    #[test]
    fn a_null_or_oversized_value_is_refused() {
        assert!(check_value("k", &Value::Null).is_err());
        assert!(check_value("k", &json!(true)).is_ok());
        assert!(check_value("k", &json!({ "a": [1, 2] })).is_ok());
        // A JSON string serializes with its two quotes.
        assert!(check_value("k", &json!("x".repeat(VALUE_MAX - 2))).is_ok());
        assert!(check_value("k", &json!("x".repeat(VALUE_MAX - 1))).is_err());
    }

    #[test]
    fn a_body_without_its_field_is_refused() {
        assert!(one_from_body("k", &json!({})).is_err());
        assert!(one_from_body("k", &json!({ "value": 1 })).is_ok());
        assert!(one_from_body("bad key", &json!({ "value": 1 })).is_err());
        assert!(many_from_body(&json!({})).is_err());
        assert!(many_from_body(&json!({ "prefs": [1] })).is_err());
        assert!(many_from_body(&json!({ "prefs": {} })).is_ok());
    }

    #[test]
    fn a_bulk_write_over_the_cap_or_with_one_bad_entry_is_refused_whole() {
        let at_cap: serde_json::Map<String, Value> =
            (0..BULK_MAX).map(|i| (format!("k{i}"), json!(i))).collect();
        assert_eq!(ok(many_from_body(&json!({ "prefs": at_cap }))).len(), BULK_MAX);
        let over: serde_json::Map<String, Value> =
            (0..=BULK_MAX).map(|i| (format!("k{i}"), json!(i))).collect();
        assert!(many_from_body(&json!({ "prefs": over })).is_err());
        assert!(many_from_body(&json!({ "prefs": { "ok": 1, "bad key": 2 } })).is_err());
        assert!(many_from_body(&json!({ "prefs": { "ok": 1, "nul": null } })).is_err());
    }

    async fn scratch_pool() -> Option<PgPool> {
        let url = std::env::var("OTW_TEST_DATABASE_URL").ok()?;
        Some(otw_store::connect_and_migrate(&url).await.unwrap())
    }

    fn prefs_body(entries: &[(&str, Value)]) -> Value {
        let map: serde_json::Map<String, Value> =
            entries.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        json!({ "prefs": map })
    }

    /// Keys unique to one run, so a reused scratch database never answers from an old one.
    fn run_key(name: &str) -> String {
        format!("test.{}.{name}", uuid::Uuid::new_v4().simple())
    }

    /// Over a scratch database (`OTW_TEST_DATABASE_URL`, skipped without it): a value is
    /// written, read back in the map, overwritten, and deleted.
    #[tokio::test]
    async fn a_pref_round_trips_through_the_table() {
        let Some(pool) = scratch_pool().await else { return };
        let key = run_key("theme");
        ok(put_one_pref(&pool, &key, &json!({ "value": "dark" })).await);
        assert_eq!(ok(list_prefs(&pool).await)["prefs"][&key], json!("dark"));
        ok(put_one_pref(&pool, &key, &json!({ "value": { "w": 240, "open": true } })).await);
        assert_eq!(store::get(&pool, &key).await.unwrap(), Some(json!({ "w": 240, "open": true })));
        assert_eq!(ok(remove_pref(&pool, &key).await), json!({ "deleted": true }));
        assert_eq!(store::get(&pool, &key).await.unwrap(), None);
        assert_eq!(ok(remove_pref(&pool, &key).await), json!({ "deleted": false }));
    }

    /// Refused writes leave nothing behind.
    #[tokio::test]
    async fn a_refused_write_stores_nothing() {
        let Some(pool) = scratch_pool().await else { return };
        let key = run_key("big");
        let big = json!({ "value": "x".repeat(VALUE_MAX) });
        assert_eq!(status(put_one_pref(&pool, &key, &big).await), 400);
        assert_eq!(status(put_one_pref(&pool, &key, &json!({ "value": null })).await), 400);
        assert_eq!(status(put_one_pref(&pool, "bad key", &json!({ "value": 1 })).await), 400);
        assert_eq!(store::get(&pool, &key).await.unwrap(), None);

        let kept = run_key("kept");
        let body = prefs_body(&[(&kept, json!(1)), ("bad key", json!(2))]);
        assert_eq!(status(put_many_prefs(&pool, &body).await), 400);
        assert_eq!(store::get(&pool, &kept).await.unwrap(), None);
    }

    /// The bulk write upserts every key it carries and leaves the others alone.
    #[tokio::test]
    async fn a_bulk_write_upserts_every_key() {
        let Some(pool) = scratch_pool().await else { return };
        let (a, b, other) = (run_key("a"), run_key("b"), run_key("other"));
        ok(put_one_pref(&pool, &a, &json!({ "value": "old" })).await);
        ok(put_one_pref(&pool, &other, &json!({ "value": 7 })).await);
        let body = prefs_body(&[(&a, json!("new")), (&b, json!([1, 2, 3]))]);
        assert_eq!(ok(put_many_prefs(&pool, &body).await), json!({ "saved": 2 }));
        let all = ok(list_prefs(&pool).await);
        assert_eq!(all["prefs"][&a], json!("new"));
        assert_eq!(all["prefs"][&b], json!([1, 2, 3]));
        assert_eq!(all["prefs"][&other], json!(7));
        for k in [&a, &b, &other] {
            store::delete(&pool, k).await.unwrap();
        }
    }
}
