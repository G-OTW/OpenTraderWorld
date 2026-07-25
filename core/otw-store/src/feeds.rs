//! Storage for the news-feed module: feeds, their fetched items, and secrets.
//!
//! Secrets are encrypted at rest (see [`crate::crypto`]) and never returned in
//! plaintext through these read paths — only names and "is set" status leak out.

use anyhow::Context;
use serde::Serialize;
use sqlx::types::JsonValue;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::crypto::SecretCipher;

// ── Feeds ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Feed {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub config: JsonValue,
    pub enabled: bool,
    pub interval_secs: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_fetched_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_success_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Number of items stored for this feed.
    pub item_count: i64,
}

type FeedRow = (
    Uuid,
    String,
    String,
    JsonValue,
    bool,
    i32,
    Option<OffsetDateTime>,
    Option<OffsetDateTime>,
    Option<String>,
    OffsetDateTime,
    i64,
);

const FEED_COLS: &str = "id, name, kind, config, enabled, interval_secs, \
                         last_fetched_at, last_success_at, last_error, updated_at";

/// Same columns as [`FEED_COLS`] but with the table aliased `f` and the item
/// count appended — used where we want each feed's stored-item total.
const FEED_COLS_WITH_COUNT: &str =
    "f.id, f.name, f.kind, f.config, f.enabled, f.interval_secs, \
     f.last_fetched_at, f.last_success_at, f.last_error, f.updated_at, \
     (SELECT count(*) FROM feed_items i WHERE i.feed_id = f.id) AS item_count";

fn to_feed(r: FeedRow) -> Feed {
    Feed {
        id: r.0,
        name: r.1,
        kind: r.2,
        config: r.3,
        enabled: r.4,
        interval_secs: r.5,
        last_fetched_at: r.6,
        last_success_at: r.7,
        last_error: r.8,
        updated_at: r.9,
        item_count: r.10,
    }
}

pub async fn list_feeds(pool: &PgPool) -> anyhow::Result<Vec<Feed>> {
    let sql = format!(
        "SELECT {FEED_COLS_WITH_COUNT} FROM feeds f ORDER BY lower(f.name), f.created_at"
    );
    let rows = sqlx::query_as::<_, FeedRow>(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing feeds")?;
    Ok(rows.into_iter().map(to_feed).collect())
}

pub async fn get_feed(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Feed>> {
    let sql = format!("SELECT {FEED_COLS_WITH_COUNT} FROM feeds f WHERE f.id = $1");
    let row = sqlx::query_as::<_, FeedRow>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(to_feed))
}

pub async fn create_feed(
    pool: &PgPool,
    name: &str,
    kind: &str,
    config: &JsonValue,
    interval_secs: i32,
) -> anyhow::Result<Feed> {
    let id = Uuid::new_v4();
    // A new feed has no items yet, so the count is statically 0.
    let sql = format!(
        "INSERT INTO feeds (id, name, kind, config, interval_secs, next_run_at) \
         VALUES ($1, $2, $3, $4, $5, now()) RETURNING {FEED_COLS}, 0::bigint AS item_count"
    );
    let row = sqlx::query_as::<_, FeedRow>(AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .bind(kind)
        .bind(config)
        .bind(interval_secs)
        .fetch_one(pool)
        .await
        .context("creating feed")?;
    Ok(to_feed(row))
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct FeedPatch {
    pub name: Option<String>,
    pub config: Option<JsonValue>,
    pub enabled: Option<bool>,
    pub interval_secs: Option<i32>,
}

pub async fn update_feed(pool: &PgPool, id: Uuid, p: &FeedPatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE feeds SET \
            name = COALESCE($2, name), \
            config = COALESCE($3, config), \
            enabled = COALESCE($4, enabled), \
            interval_secs = COALESCE($5, interval_secs), \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(p.name.as_deref())
    .bind(p.config.as_ref())
    .bind(p.enabled)
    .bind(p.interval_secs)
    .execute(pool)
    .await
    .context("updating feed")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_feed(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM feeds WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Scheduling support ───────────────────────────────────────────────────────

/// Claim feeds that are due, advancing their next_run_at so another scheduler
/// tick won't pick the same one. Returns the claimed feeds.
///
/// A source is due when it is referenced by at least one *started* dashboard and
/// `next_run_at <= now()`. Its effective cadence is the shortest `interval_secs`
/// among the started dashboards that reference it (so the most-eager dashboard
/// wins). Sources in no started dashboard are never polled. SKIP LOCKED keeps
/// this safe under concurrency.
pub async fn claim_due_feeds(pool: &PgPool, limit: i64) -> anyhow::Result<Vec<Feed>> {
    let sql = format!(
        "WITH eff AS ( \
             SELECT ds.feed_id, min(ds.interval_secs) AS interval_secs \
             FROM dashboard_sources ds \
             JOIN feed_dashboards d ON d.id = ds.dashboard_id \
             WHERE d.started \
             GROUP BY ds.feed_id \
         ) \
         UPDATE feeds SET next_run_at = now() + (eff.interval_secs || ' seconds')::interval \
         FROM eff \
         WHERE feeds.id = eff.feed_id AND feeds.id IN ( \
             SELECT f.id FROM feeds f JOIN eff e ON e.feed_id = f.id \
             WHERE f.next_run_at <= now() \
             ORDER BY f.next_run_at \
             LIMIT $1 FOR UPDATE OF f SKIP LOCKED \
         ) RETURNING \
         feeds.id, feeds.name, feeds.kind, feeds.config, feeds.enabled, feeds.interval_secs, \
         feeds.last_fetched_at, feeds.last_success_at, feeds.last_error, feeds.updated_at, \
         (SELECT count(*) FROM feed_items i WHERE i.feed_id = feeds.id) AS item_count"
    );
    let rows = sqlx::query_as::<_, FeedRow>(AssertSqlSafe(sql))
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("claiming due feeds")?;
    Ok(rows.into_iter().map(to_feed).collect())
}

pub async fn mark_success(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE feeds SET last_fetched_at = now(), last_success_at = now(), last_error = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_error(pool: &PgPool, id: Uuid, error: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE feeds SET last_fetched_at = now(), last_error = $2 WHERE id = $1")
        .bind(id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Items ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FeedItem {
    pub id: Uuid,
    pub feed_id: Uuid,
    pub title: String,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub source_name: String,
    pub source_type: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub fetched_at: OffsetDateTime,
    /// Pagination key: coalesce(published_at, fetched_at). The client echoes
    /// this (with `id`) back as the cursor for the next page.
    #[serde(with = "time::serde::rfc3339")]
    pub sort_key: OffsetDateTime,
}

/// A normalized item to insert. `dedup_key` distinguishes items within a feed.
pub struct NewItem {
    pub dedup_key: String,
    pub title: String,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub source_name: String,
    pub source_type: String,
    pub published_at: Option<OffsetDateTime>,
    pub raw: Option<JsonValue>,
}

/// Insert items, skipping duplicates (by feed_id + dedup_key). Returns how many
/// new rows were actually inserted.
pub async fn insert_items(
    pool: &PgPool,
    feed_id: Uuid,
    items: &[NewItem],
) -> anyhow::Result<u64> {
    let mut inserted = 0u64;
    for it in items {
        let res = sqlx::query(
            "INSERT INTO feed_items \
                (id, feed_id, dedup_key, title, url, summary, source_name, source_type, published_at, raw) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (feed_id, dedup_key) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(feed_id)
        .bind(&it.dedup_key)
        .bind(&it.title)
        .bind(&it.url)
        .bind(&it.summary)
        .bind(&it.source_name)
        .bind(&it.source_type)
        .bind(it.published_at)
        .bind(&it.raw)
        .execute(pool)
        .await
        .context("inserting feed item")?;
        inserted += res.rows_affected();
    }
    Ok(inserted)
}

/// Filter parameters for listing items (simple filters; rich query comes later).
#[derive(Debug, Default)]
pub struct ItemFilter {
    pub q: Option<String>,            // substring in title/summary
    pub source_names: Vec<String>,    // match any of these (empty = all)
    pub source_type: Option<String>,  // exact (rss | api | …)
    pub feed_id: Option<Uuid>,
    /// Restrict to items from sources linked to this dashboard.
    pub dashboard_id: Option<Uuid>,
    pub since: Option<OffsetDateTime>,
    pub until: Option<OffsetDateTime>,
    /// Keyset cursor for infinite scroll: return items strictly *older* than
    /// this (sort_key, id) pair. The sort key is coalesce(published_at,
    /// fetched_at) — non-NULL and matched by the returned items' `sort_key`.
    pub before_key: Option<OffsetDateTime>,
    pub before_id: Option<Uuid>,
    pub limit: i64,
}

type ItemRow = (
    Uuid,
    Uuid,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<OffsetDateTime>,
    OffsetDateTime,
);

fn to_item(r: ItemRow) -> FeedItem {
    FeedItem {
        id: r.0,
        feed_id: r.1,
        title: r.2,
        url: r.3,
        summary: r.4,
        source_name: r.5,
        source_type: r.6,
        published_at: r.7,
        fetched_at: r.8,
        sort_key: r.7.unwrap_or(r.8),
    }
}

pub async fn list_items(pool: &PgPool, f: &ItemFilter) -> anyhow::Result<Vec<FeedItem>> {
    let mut sql = String::from(
        "SELECT id, feed_id, title, url, summary, source_name, source_type, published_at, fetched_at \
         FROM feed_items WHERE 1=1",
    );
    let mut idx = 0;
    if f.q.is_some() {
        idx += 1;
        sql.push_str(&format!(" AND (title ILIKE ${idx} OR summary ILIKE ${idx})"));
    }
    if !f.source_names.is_empty() {
        idx += 1;
        sql.push_str(&format!(" AND source_name = ANY(${idx})"));
    }
    if f.source_type.is_some() {
        idx += 1;
        sql.push_str(&format!(" AND source_type = ${idx}"));
    }
    if f.feed_id.is_some() {
        idx += 1;
        sql.push_str(&format!(" AND feed_id = ${idx}"));
    }
    if f.dashboard_id.is_some() {
        idx += 1;
        sql.push_str(&format!(
            " AND feed_id IN (SELECT feed_id FROM dashboard_sources WHERE dashboard_id = ${idx})"
        ));
    }
    if f.since.is_some() {
        idx += 1;
        sql.push_str(&format!(" AND published_at >= ${idx}"));
    }
    if f.until.is_some() {
        idx += 1;
        sql.push_str(&format!(" AND published_at <= ${idx}"));
    }
    // Keyset cursor: items strictly older than (before_key, before_id) under the
    // sort below. Both binds are present together (set by the caller as a pair).
    if f.before_key.is_some() && f.before_id.is_some() {
        let k = idx + 1;
        let i = idx + 2;
        idx += 2;
        sql.push_str(&format!(
            " AND (coalesce(published_at, fetched_at), id) < (${k}, ${i})"
        ));
    }
    idx += 1;
    // Sort on a single non-NULL key so the keyset comparison above is exact.
    sql.push_str(&format!(
        " ORDER BY coalesce(published_at, fetched_at) DESC, id DESC LIMIT ${idx}"
    ));

    let mut query = sqlx::query_as::<_, ItemRow>(AssertSqlSafe(sql));
    if let Some(q) = &f.q {
        query = query.bind(format!("%{q}%"));
    }
    if !f.source_names.is_empty() {
        query = query.bind(f.source_names.clone());
    }
    if let Some(v) = &f.source_type {
        query = query.bind(v.clone());
    }
    if let Some(v) = f.feed_id {
        query = query.bind(v);
    }
    if let Some(v) = f.dashboard_id {
        query = query.bind(v);
    }
    if let Some(v) = f.since {
        query = query.bind(v);
    }
    if let Some(v) = f.until {
        query = query.bind(v);
    }
    if let (Some(k), Some(i)) = (f.before_key, f.before_id) {
        query = query.bind(k);
        query = query.bind(i);
    }
    query = query.bind(f.limit.clamp(1, 500));

    let rows = query.fetch_all(pool).await.context("listing feed items")?;
    Ok(rows.into_iter().map(to_item).collect())
}

/// Distinct source names/types present, for populating filter dropdowns.
pub async fn distinct_sources(pool: &PgPool) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let names: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT source_name FROM feed_items WHERE source_name <> '' ORDER BY source_name",
    )
    .fetch_all(pool)
    .await?;
    let types: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT source_type FROM feed_items WHERE source_type <> '' ORDER BY source_type",
    )
    .fetch_all(pool)
    .await?;
    Ok((
        names.into_iter().map(|r| r.0).collect(),
        types.into_iter().map(|r| r.0).collect(),
    ))
}

// ── Secrets ──────────────────────────────────────────────────────────────────

/// Store (or replace) a secret for a feed. Plaintext is encrypted immediately.
/// A direct value replaces any vault reference the secret had.
pub async fn set_secret(
    pool: &PgPool,
    cipher: &SecretCipher,
    feed_id: Uuid,
    name: &str,
    plaintext: &str,
) -> anyhow::Result<()> {
    let (nonce, ciphertext) = cipher.seal(plaintext)?;
    sqlx::query(
        "INSERT INTO feed_secrets (id, feed_id, name, nonce, ciphertext) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (feed_id, name) \
         DO UPDATE SET nonce = EXCLUDED.nonce, ciphertext = EXCLUDED.ciphertext, \
                       vault_item_id = NULL, updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(feed_id)
    .bind(name)
    .bind(nonce)
    .bind(ciphertext)
    .execute(pool)
    .await
    .context("storing secret")?;
    Ok(())
}

/// Plug a centralized vault item as a feed secret (no local sealed copy is kept).
pub async fn set_secret_ref(
    pool: &PgPool,
    feed_id: Uuid,
    name: &str,
    vault_item_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO feed_secrets (id, feed_id, name, nonce, ciphertext, vault_item_id) \
         VALUES ($1, $2, $3, NULL, NULL, $4) \
         ON CONFLICT (feed_id, name) \
         DO UPDATE SET nonce = NULL, ciphertext = NULL, vault_item_id = $4, updated_at = now()",
    )
    .bind(Uuid::new_v4())
    .bind(feed_id)
    .bind(name)
    .bind(vault_item_id)
    .execute(pool)
    .await
    .context("storing secret reference")?;
    Ok(())
}

pub async fn delete_secret(pool: &PgPool, feed_id: Uuid, name: &str) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM feed_secrets WHERE feed_id = $1 AND name = $2")
        .bind(feed_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// List secret names for a feed (never the values).
pub async fn list_secret_names(pool: &PgPool, feed_id: Uuid) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM feed_secrets WHERE feed_id = $1 ORDER BY name")
            .bind(feed_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Secret names + their vault reference (if plugged), for the source form UI.
pub async fn list_secret_meta(
    pool: &PgPool,
    feed_id: Uuid,
) -> anyhow::Result<Vec<(String, Option<Uuid>)>> {
    Ok(sqlx::query_as(
        "SELECT name, vault_item_id FROM feed_secrets WHERE feed_id = $1 ORDER BY name",
    )
    .bind(feed_id)
    .fetch_all(pool)
    .await?)
}

/// Decrypt all secrets for a feed into a name→plaintext map (fetcher use only).
/// Vault-plugged secrets resolve from the vault; each distinct vault used is counted
/// once against its vault-wide quota (one poll = one logical request).
pub async fn load_secrets(
    pool: &PgPool,
    cipher: &SecretCipher,
    feed_id: Uuid,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>, Option<Uuid>)> = sqlx::query_as(
        "SELECT fs.name, COALESCE(vi.nonce, fs.nonce), COALESCE(vi.ciphertext, fs.ciphertext), \
                fs.vault_item_id \
         FROM feed_secrets fs LEFT JOIN vault_items vi ON vi.id = fs.vault_item_id \
         WHERE fs.feed_id = $1",
    )
    .bind(feed_id)
    .fetch_all(pool)
    .await?;
    let mut map = std::collections::HashMap::new();
    let mut vault_items = Vec::new();
    for (name, nonce, ct, ref_id) in rows {
        if let (Some(nonce), Some(ct)) = (nonce, ct) {
            map.insert(name, cipher.open(&nonce, &ct)?);
            if let Some(id) = ref_id {
                vault_items.push(id);
            }
        }
    }
    crate::vault::bump_vaults_for_items(pool, &vault_items).await?;
    Ok(map)
}

// ── Source dedup ──────────────────────────────────────────────────────────────

/// Stable identity hash of a source: kind + normalized config + sorted secret
/// names. Two sources sharing a hash are "the same" for the reuse prompt. Secret
/// *values* are never part of the hash (only names) — they live encrypted and
/// aren't available here, and the names already pin which credentials are used.
pub fn dedup_hash(kind: &str, config: &JsonValue, secret_names: &[String]) -> String {
    use sha2::{Digest, Sha256};
    // serde_json sorts object keys when using to_value→canonical? It does not by
    // default, so build a canonical string ourselves for object configs.
    let canon = canonical_json(config);
    let mut names = secret_names.to_vec();
    names.sort();
    let mut h = Sha256::new();
    h.update(kind.as_bytes());
    h.update([0]);
    h.update(canon.as_bytes());
    h.update([0]);
    h.update(names.join(",").as_bytes());
    format!("{:x}", h.finalize())
}

/// Deterministic JSON serialization with sorted object keys (recursive), so
/// logically-equal configs hash identically regardless of key order.
fn canonical_json(v: &JsonValue) -> String {
    match v {
        JsonValue::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{:?}:{}", k, canonical_json(&map[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        JsonValue::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

/// Recompute and store a feed's dedup hash from its current kind, config and
/// secret names. Call after any create/config-change/secret-change.
pub async fn recompute_dedup_hash(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    let row: Option<(String, JsonValue)> =
        sqlx::query_as("SELECT kind, config FROM feeds WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let Some((kind, config)) = row else {
        return Ok(());
    };
    let names = list_secret_names(pool, id).await?;
    let hash = dedup_hash(&kind, &config, &names);
    sqlx::query("UPDATE feeds SET dedup_hash = $2 WHERE id = $1")
        .bind(id)
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// A source matching a dedup hash, plus the dashboards it already belongs to.
#[derive(Debug, Serialize)]
pub struct DedupMatch {
    pub feed_id: Uuid,
    pub feed_name: String,
    pub dashboard_names: Vec<String>,
}

/// Find an existing source with this dedup hash (if any), reporting which
/// dashboards already contain it. Used to drive the "reuse this source?" prompt.
pub async fn find_by_hash(pool: &PgPool, hash: &str) -> anyhow::Result<Option<DedupMatch>> {
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM feeds WHERE dedup_hash = $1 LIMIT 1")
            .bind(hash)
            .fetch_optional(pool)
            .await?;
    let Some((feed_id, feed_name)) = row else {
        return Ok(None);
    };
    let names: Vec<(String,)> = sqlx::query_as(
        "SELECT d.name FROM dashboard_sources ds \
         JOIN feed_dashboards d ON d.id = ds.dashboard_id \
         WHERE ds.feed_id = $1 ORDER BY d.name",
    )
    .bind(feed_id)
    .fetch_all(pool)
    .await?;
    Ok(Some(DedupMatch {
        feed_id,
        feed_name,
        dashboard_names: names.into_iter().map(|r| r.0).collect(),
    }))
}

// ── Dashboards ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Dashboard {
    pub id: Uuid,
    pub name: String,
    pub is_default: bool,
    pub started: bool,
    pub favorite: bool,
    pub position: i32,
    /// Number of sources linked to this dashboard.
    pub source_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

type DashRow = (Uuid, String, bool, bool, bool, i32, i64, OffsetDateTime);

fn to_dash(r: DashRow) -> Dashboard {
    Dashboard {
        id: r.0,
        name: r.1,
        is_default: r.2,
        started: r.3,
        favorite: r.4,
        position: r.5,
        source_count: r.6,
        updated_at: r.7,
    }
}

const DASH_COLS: &str = "d.id, d.name, d.is_default, d.started, d.favorite, d.position, \
     (SELECT count(*) FROM dashboard_sources ds WHERE ds.dashboard_id = d.id) AS source_count, \
     d.updated_at";

pub async fn list_dashboards(pool: &PgPool) -> anyhow::Result<Vec<Dashboard>> {
    let sql = format!(
        "SELECT {DASH_COLS} FROM feed_dashboards d ORDER BY d.position, lower(d.name), d.created_at"
    );
    let rows = sqlx::query_as::<_, DashRow>(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .context("listing dashboards")?;
    Ok(rows.into_iter().map(to_dash).collect())
}

pub async fn get_dashboard(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Dashboard>> {
    let sql = format!("SELECT {DASH_COLS} FROM feed_dashboards d WHERE d.id = $1");
    let row = sqlx::query_as::<_, DashRow>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(to_dash))
}

pub async fn create_dashboard(pool: &PgPool, name: &str) -> anyhow::Result<Dashboard> {
    let id = Uuid::new_v4();
    // New dashboards are started by default and have no sources yet (count 0).
    let sql = format!(
        "INSERT INTO feed_dashboards (id, name) VALUES ($1, $2) \
         RETURNING id, name, is_default, started, favorite, position, 0::bigint AS source_count, updated_at"
    );
    let row = sqlx::query_as::<_, DashRow>(AssertSqlSafe(sql))
        .bind(id)
        .bind(name)
        .fetch_one(pool)
        .await
        .context("creating dashboard")?;
    Ok(to_dash(row))
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct DashboardPatch {
    pub name: Option<String>,
    pub started: Option<bool>,
    pub favorite: Option<bool>,
    pub position: Option<i32>,
}

pub async fn update_dashboard(pool: &PgPool, id: Uuid, p: &DashboardPatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE feed_dashboards SET \
            name = COALESCE($2, name), \
            started = COALESCE($3, started), \
            favorite = COALESCE($4, favorite), \
            position = COALESCE($5, position), \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(p.name.as_deref())
    .bind(p.started)
    .bind(p.favorite)
    .bind(p.position)
    .execute(pool)
    .await
    .context("updating dashboard")?;
    Ok(res.rows_affected() > 0)
}

/// Make `id` the sole default dashboard (clears any previous default) in one tx.
pub async fn set_default_dashboard(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE feed_dashboards SET is_default = FALSE WHERE is_default")
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query("UPDATE feed_dashboards SET is_default = TRUE, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_dashboard(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    // Sources linked only to this dashboard are left intact (still listable,
    // just unpolled until placed in a started dashboard). Cascade removes links.
    let res = sqlx::query("DELETE FROM feed_dashboards WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Link a source to a dashboard with a poll interval. Idempotent: re-linking
/// updates the interval. Resets the source's next_run_at so it polls promptly.
pub async fn link_source(
    pool: &PgPool,
    dashboard_id: Uuid,
    feed_id: Uuid,
    interval_secs: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO dashboard_sources (dashboard_id, feed_id, interval_secs) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (dashboard_id, feed_id) DO UPDATE SET interval_secs = EXCLUDED.interval_secs",
    )
    .bind(dashboard_id)
    .bind(feed_id)
    .bind(interval_secs)
    .execute(pool)
    .await
    .context("linking source to dashboard")?;
    sqlx::query("UPDATE feeds SET next_run_at = now() WHERE id = $1")
        .bind(feed_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn unlink_source(pool: &PgPool, dashboard_id: Uuid, feed_id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM dashboard_sources WHERE dashboard_id = $1 AND feed_id = $2")
        .bind(dashboard_id)
        .bind(feed_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// List the sources linked to a dashboard, in link order, with their per-link
/// interval. Reuses [`Feed`] for the source fields plus the link interval.
pub async fn dashboard_sources(pool: &PgPool, dashboard_id: Uuid) -> anyhow::Result<Vec<Feed>> {
    // Note: the returned Feed.interval_secs is overridden with the *link*
    // interval so the UI shows the cadence that actually applies here.
    let sql = format!(
        "SELECT f.id, f.name, f.kind, f.config, f.enabled, ds.interval_secs, \
                f.last_fetched_at, f.last_success_at, f.last_error, f.updated_at, \
                (SELECT count(*) FROM feed_items i WHERE i.feed_id = f.id) AS item_count \
         FROM dashboard_sources ds JOIN feeds f ON f.id = ds.feed_id \
         WHERE ds.dashboard_id = $1 ORDER BY ds.position, lower(f.name)"
    );
    let rows = sqlx::query_as::<_, FeedRow>(AssertSqlSafe(sql))
        .bind(dashboard_id)
        .fetch_all(pool)
        .await
        .context("listing dashboard sources")?;
    Ok(rows.into_iter().map(to_feed).collect())
}

/// Total new items across every source in a (single) dashboard, polled now.
/// Returns the list of (feed_id, link interval) so the caller can poll each.
pub async fn dashboard_source_ids(pool: &PgPool, dashboard_id: Uuid) -> anyhow::Result<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT feed_id FROM dashboard_sources WHERE dashboard_id = $1")
            .bind(dashboard_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}
