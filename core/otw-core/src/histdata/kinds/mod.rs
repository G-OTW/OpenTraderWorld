//! Config-driven connector kinds: a connector whose behaviour comes from its row's settings
//! (`histdata_connectors.config`), not from a Rust provider.
//!
//! A kind has an id (`http`) that the create form posts and that the providers list shows as
//! a template. Each connector created from it gets its **own** provider id (`http-1a2b3c4d`),
//! minted at creation: datasets, jobs, live subscriptions and quotas are keyed by provider
//! id, and two custom sources answering to one id would hand each other's bars to the same
//! rows. Two Binance connectors share a provider because they read the same market; two
//! custom sources never do.
//!
//! Every reader that only has a provider id (a queued job, a live seed, a stored dataset)
//! finds the settings through a process-wide registry, filled from the database at startup
//! and refreshed by every create, edit and delete. A reader that holds the row builds the
//! connector from the row itself ([`super::connector_for_row`]).

use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::{anyhow, Result};
use sqlx::PgPool;

use super::Connector;

pub(crate) mod http;
pub(crate) mod push;
pub(crate) mod value;

#[cfg(test)]
mod http_tests;

/// Kind ids, as the create form posts them.
pub const KINDS: &[&str] = &[http::KIND, push::KIND];

/// The kind a provider id belongs to: the kind's own id (the template) or one minted from it.
pub fn kind_of(provider: &str) -> Option<&'static str> {
    KINDS.iter().copied().find(|k| {
        provider == *k || provider.strip_prefix(k).is_some_and(|rest| rest.starts_with('-'))
    })
}

/// Whether `provider` is a connector minted from a kind (not the kind's template).
pub fn is_instance(provider: &str) -> bool {
    kind_of(provider).is_some_and(|k| k != provider)
}

/// Whether `provider` is a kind's template id (what the create form posts).
pub fn is_template(provider: &str) -> bool {
    KINDS.contains(&provider)
}

/// A fresh provider id for a connector of `kind`.
pub fn mint_provider(kind: &str) -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("{kind}-{}", &id[..8])
}

/// A row's settings as the flat map a connector reads: the same rendering as
/// `connectors::load_creds` (numbers and booleans as text, nulls dropped).
pub fn config_map(config: &serde_json::Value) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let serde_json::Value::Object(obj) = config {
        for (k, v) in obj {
            let text = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => continue,
                other => other.to_string(),
            };
            map.insert(k.clone(), text);
        }
    }
    map
}

/// What the registry keeps per minted provider: the connector's name and its settings.
#[derive(Clone)]
struct Entry {
    name: String,
    config: HashMap<String, String>,
}

static REGISTRY: RwLock<Option<HashMap<String, Entry>>> = RwLock::new(None);

/// Remember (or refresh) a minted connector's settings. A row of a static provider is ignored.
pub fn register(provider: &str, name: &str, config: &serde_json::Value) {
    if !is_instance(provider) {
        return;
    }
    if let Ok(mut guard) = REGISTRY.write() {
        guard.get_or_insert_with(HashMap::new).insert(
            provider.to_string(),
            Entry { name: name.to_string(), config: config_map(config) },
        );
    }
}

/// Drop a deleted connector's settings.
pub fn forget(provider: &str) {
    if let Ok(mut guard) = REGISTRY.write() {
        if let Some(map) = guard.as_mut() {
            map.remove(provider);
        }
    }
}

/// Fill the registry from every stored connector of a kind. Called once at startup, before
/// the download worker and the live hub can ask for one.
pub async fn load(pool: &PgPool) -> Result<usize> {
    let rows = otw_store::connectors::list(pool).await?;
    let mut n = 0;
    for r in rows.iter().filter(|r| is_instance(&r.provider)) {
        register(&r.provider, &r.name, &r.config);
        n += 1;
    }
    Ok(n)
}

/// A connector of a kind, built from a provider id, a name and its settings.
pub fn build(provider: &str, name: &str, config: HashMap<String, String>) -> Result<Box<dyn Connector>> {
    match kind_of(provider) {
        Some(http::KIND) if provider == http::KIND => Ok(Box::new(http::HttpSource::template())),
        Some(http::KIND) => Ok(Box::new(http::HttpSource::new(provider, name, config))),
        Some(push::KIND) if provider == push::KIND => Ok(Box::new(push::PushSource::template())),
        Some(push::KIND) => Ok(Box::new(push::PushSource::new(provider, name, config))),
        _ => Err(anyhow!("unknown provider: {provider}")),
    }
}

/// A connector of a kind by provider id alone: the template, or a minted one from the registry.
pub fn connector_for(provider: &str) -> Result<Box<dyn Connector>> {
    if is_template(provider) {
        return build(provider, "", HashMap::new());
    }
    let entry = REGISTRY
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(provider).cloned()))
        .ok_or_else(|| {
            anyhow!("custom source {provider} is not configured: its connector was deleted or not loaded yet")
        })?;
    build(provider, &entry.name, entry.config)
}

/// A minted push connector's settings, read, from the registry.
pub(crate) fn push_spec(provider: &str) -> Result<push::Spec> {
    if kind_of(provider) != Some(push::KIND) || !is_instance(provider) {
        return Err(anyhow!("{provider} is not a push source"));
    }
    let entry = REGISTRY
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(provider).cloned()))
        .ok_or_else(|| anyhow!("custom source {provider} is not configured: its connector was deleted or not loaded yet"))?;
    push::Spec::parse(&entry.config)
}

/// A minted connector's settings, from the registry.
fn registered_config(provider: &str) -> Option<HashMap<String, String>> {
    REGISTRY.read().ok().and_then(|g| g.as_ref().and_then(|m| m.get(provider).map(|e| e.config.clone())))
}

/// A minted `http` connector as its concrete type, from the registry: what a caller needs to
/// reach past the `Connector` trait (a value source's observations).
pub(crate) fn http_instance(provider: &str) -> Result<http::HttpSource> {
    if kind_of(provider) != Some(http::KIND) || !is_instance(provider) {
        return Err(anyhow!("{provider} is not a custom HTTP source"));
    }
    let entry = REGISTRY
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(provider).cloned()))
        .ok_or_else(|| anyhow!("custom source {provider} is not configured: its connector was deleted or not loaded yet"))?;
    Ok(http::HttpSource::new(provider, &entry.name, entry.config))
}

/// The value mapping of a minted value-shaped source (`shape = value`), or `None` for a
/// candle source, a template, or anything else.
pub(crate) fn value_mapping(provider: &str) -> Option<value::ValueMapping> {
    if !is_instance(provider) {
        return None;
    }
    let cfg = registered_config(provider)?;
    if !value::is_value_shape(&cfg) {
        return None;
    }
    value::ValueMapping::parse(&cfg).ok()
}

/// Whether a provider is a custom value source: one more Fundamentals macro provider.
pub fn serves_values(provider: &str) -> bool {
    is_instance(provider) && registered_config(provider).is_some_and(|c| value::is_value_shape(&c))
}

/// One template per kind, for the providers list and the create form.
pub fn templates() -> Vec<Box<dyn Connector>> {
    KINDS.iter().filter_map(|k| build(k, "", HashMap::new()).ok()).collect()
}

/// Every registered minted connector.
pub fn instances() -> Vec<Box<dyn Connector>> {
    let entries: Vec<(String, Entry)> = REGISTRY
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()))
        .unwrap_or_default();
    entries
        .into_iter()
        .filter_map(|(p, e)| build(&p, &e.name, e.config).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_ids_belong_to_their_kind_and_never_collide_with_it() {
        let p = mint_provider("http");
        assert!(p.starts_with("http-") && p.len() == 13, "{p}");
        assert_eq!(kind_of(&p), Some("http"));
        assert!(is_instance(&p));
        assert!(!is_instance("http"));
        assert!(is_template("http"));
        assert_eq!(kind_of("binance"), None);
        // A static provider that merely starts with the kind's letters is not one of it.
        assert_eq!(kind_of("httpx"), None);
        assert_ne!(mint_provider("http"), p);
    }

    #[test]
    fn an_unregistered_minted_id_is_a_named_error() {
        let Err(e) = connector_for("http-00000000") else { panic!("nothing is registered under that id") };
        assert!(e.to_string().contains("http-00000000"), "{e}");
    }
}
