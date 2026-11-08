//! Global search API — title-only lookups backing the top-bar search.
//!
//! The frontend matches module names and settings sections client-side; this endpoint
//! covers the DB-backed scopes. Deliberately NOT in the MCP catalog.

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};
use otw_store::search;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/search", get(search_titles))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Params {
    q: String,
    /// Comma-separated scope keys (see `search::known_scopes`). Empty means every scope;
    /// an unknown key is a 400 that lists the valid ones.
    #[serde(default)]
    scopes: String,
}

async fn search_titles(
    State(state): State<AppState>,
    Query(p): Query<Params>,
) -> Result<Json<Value>, ApiError> {
    let q = p.q.trim();
    if q.is_empty() {
        return Err(ApiError::bad_request("query required"));
    }
    if q.chars().count() > 200 {
        return Err(ApiError::bad_request("query too long"));
    }
    let scopes = parse_scopes(&p.scopes)?;
    let hits = search::search_titles(&state.pool, &scopes, q).await?;
    Ok(Json(json!({ "hits": hits })))
}

crate::util::vocab::vocab! {
    /// A DB-backed search scope. Mirrors `otw_store::search::known_scopes` (a test keeps
    /// the two equal).
    pub SearchScope("scope") {
        Resources = "resources",
        Documents = "documents",
        Goals = "goals",
        Events = "events",
        Todos = "todos",
        Routines = "routines",
        Reminders = "reminders",
        Prompts = "prompts",
        Mailbox = "mailbox",
        CommunityDocs = "community-docs",
        FundCompanies = "fund-companies",
        FundSeries = "fund-series",
        FundDocuments = "fund-documents",
    }
}

/// Read the `scopes` list: blank means every scope, a key is matched case-insensitively,
/// and an unknown key is refused with the full list rather than silently searching less.
fn parse_scopes(raw: &str) -> Result<Vec<&'static str>, ApiError> {
    let mut out = Vec::new();
    for key in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let k = SearchScope::parse(key)
            .map_err(|e| ApiError::bad_request(&format!("{e} (empty means all)")))?
            .as_str();
        if !out.contains(&k) {
            out.push(k);
        }
    }
    if out.is_empty() {
        out = SearchScope::VALUES.to_vec();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vocab_matches_the_store() {
        let store: Vec<&str> = search::known_scopes().collect();
        assert_eq!(SearchScope::VALUES, store.as_slice());
    }

    #[test]
    fn blank_scopes_mean_all() {
        let all: Vec<&str> = search::known_scopes().collect();
        assert_eq!(parse_scopes("").ok(), Some(all.clone()));
        assert_eq!(parse_scopes(" , ").ok(), Some(all));
    }

    #[test]
    fn scopes_are_case_insensitive_and_deduplicated() {
        assert_eq!(parse_scopes("Goals, todos,goals").ok(), Some(vec!["goals", "todos"]));
    }

    #[test]
    fn an_unknown_scope_is_refused_with_the_list() {
        let e = parse_scopes("goals,trades").unwrap_err();
        assert!(e.message().starts_with("unknown scope \"trades\"; valid: resources, documents"), "{}", e.message());
    }
}
