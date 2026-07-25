//! Data management for the Settings page: per-module storage size and per-module wipe.
//!
//! Each module owns a fixed set of tables. We report on-disk size via
//! `pg_total_relation_size` (table + indexes + TOAST) and wipe via `TRUNCATE … CASCADE`.
//! Table lists are fixed identifiers (never user input), so dynamic SQL is safe here.
//!
//! Core tables (users, sessions, app_settings, app_logs, findb reference data) are not
//! exposed as wipeable modules — clearing them would break auth or reference data.

use anyhow::Context;
use serde::Serialize;
use sqlx::PgPool;

/// A module and the tables it owns. `id` matches the frontend registry module id.
struct ModuleTables {
    id: &'static str,
    name: &'static str,
    tables: &'static [&'static str],
}

/// Single source of truth mapping modules to their tables. Order is display order.
const MODULES: &[ModuleTables] = &[
    ModuleTables {
        id: "editor",
        name: "Editor",
        tables: &["documents", "database_columns", "database_rows"],
    },
    ModuleTables {
        id: "news",
        name: "News",
        tables: &["feeds", "feed_items", "feed_secrets"],
    },
    ModuleTables {
        id: "journal",
        name: "Trading Journal",
        tables: &[
            "journal_trades",
            "journal_categories",
            "journal_strategies",
            "journal_templates",
            "journal_capital_events",
            "journal_fee_schedules",
            "journal_fx_rates",
            "journal_fx_pending",
            "journal_settings",
        ],
    },
    ModuleTables {
        id: "subscriptions",
        name: "Subscriptions",
        tables: &["subscriptions", "subscription_settings"],
    },
    ModuleTables {
        id: "time",
        name: "Time Tracker",
        tables: &["time_entries", "time_projects", "time_state"],
    },
    ModuleTables {
        id: "wealth",
        name: "MyWealth",
        tables: &[
            "wealth_assets",
            "wealth_revisions",
            "wealth_templates",
            "wealth_settings",
        ],
    },
    ModuleTables { id: "goals", name: "Goals", tables: &["goals"] },
    ModuleTables { id: "todos", name: "ToDo", tables: &["todos"] },
    ModuleTables {
        id: "routines",
        name: "Trading Routines",
        tables: &[
            "trader_routine_checks",
            "trader_routine_items",
            "trader_routines",
            "trader_tasks",
        ],
    },
    ModuleTables {
        id: "mindset",
        name: "Mindset",
        tables: &["mindset_entries", "mindset_prompts"],
    },
    ModuleTables { id: "remindme", name: "RemindMe", tables: &["reminders", "notifications"] },
    ModuleTables {
        id: "findb",
        name: "FinanceDatabase (favorites)",
        // Reference instrument data is preserved; only user favorites/folders are wipeable.
        tables: &["findb_favorites", "findb_folders"],
    },
    ModuleTables { id: "calendar", name: "Calendar", tables: &["calendar_events"] },
    ModuleTables {
        id: "histdata",
        name: "Historical Data",
        tables: &["histdata_bars", "histdata_datasets", "histdata_jobs"],
    },
    // Charts existing histdata tables; owns none of its own.
    ModuleTables { id: "histviz", name: "Historical Data Visualization", tables: &[] },
    ModuleTables { id: "backtest", name: "Backtest", tables: &["backtest_runs"] },
    ModuleTables {
        id: "mportfolios",
        name: "Managers' Portfolios",
        tables: &["manager_holdings", "manager_portfolios"],
    },
    ModuleTables {
        id: "portfolios",
        name: "Portfolio Tracker",
        tables: &[
            "portfolio_assets",
            "portfolio_operations",
            "portfolio_snapshots",
            "portfolios",
            "portfolio_coingecko_cache",
        ],
    },
    ModuleTables {
        id: "watchlists",
        name: "Watchlists",
        tables: &["watchlist_items", "watchlists"],
    },
    ModuleTables {
        id: "taxcalc",
        name: "Tax Calculator",
        tables: &["taxcalc_profiles", "taxcalc_scenarios"],
    },
    ModuleTables { id: "resources", name: "Resources", tables: &["resources", "resource_categories"] },
    ModuleTables {
        id: "prompt-store",
        name: "Prompt Store",
        // Versions cascade-delete with their prompt, but list both so size/rows are complete.
        tables: &["prompt_store_versions", "prompt_store_prompts"],
    },
    ModuleTables { id: "community-docs", name: "Community Docs", tables: &["community_docs"] },
    ModuleTables {
        id: "webhooks",
        name: "Webhooks",
        tables: &["webhook_events", "webhook_endpoints"],
    },
    ModuleTables {
        id: "agent",
        name: "Agent",
        tables: &[
            "agent_messages",
            "agent_conversations",
            "agent_memories",
            "agent_skills",
            "agent_agents",
            "agent_providers",
        ],
    },
    // Display-only modules with no storage of their own (TradingView embed / analytics over
    // stored data). Listed so they're installable; nothing to size or wipe.
    ModuleTables { id: "economics", name: "Economic Calendar", tables: &[] },
    ModuleTables { id: "quant", name: "Quant Tools", tables: &[] },
];

#[derive(Debug, Clone, Serialize)]
pub struct ModuleUsage {
    pub id: String,
    pub name: String,
    /// On-disk bytes for all of the module's tables.
    pub size_bytes: i64,
    /// Total live rows across the module's tables (approximate via reltuples).
    pub rows: i64,
}

/// System tables that aren't part of any feature module but still consume space. Shown in
/// usage for visibility; not wipeable via `wipe_module` (clearing them would break auth /
/// settings). Logs are cleared via the Settings "Logs" section instead.
const SYSTEM_TABLES: &[(&str, &str)] = &[
    ("app_logs", "Application logs"),
    ("app_settings", "Settings"),
];

#[derive(Debug, Clone, Serialize)]
pub struct DataUsage {
    pub modules: Vec<ModuleUsage>,
    /// Core/system tables (logs, settings) — informational, not module-wipeable.
    pub system: Vec<ModuleUsage>,
    /// Whole-database on-disk size in bytes.
    pub database_bytes: i64,
}

fn module_by_id(id: &str) -> Option<&'static ModuleTables> {
    MODULES.iter().find(|m| m.id == id)
}

/// On-disk size + row estimate summed over a set of tables (missing tables count as zero).
async fn size_tables(pool: &PgPool, tables: &[&str]) -> anyhow::Result<(i64, i64)> {
    let mut size_bytes: i64 = 0;
    let mut rows: i64 = 0;
    for &t in tables {
        // to_regclass is NULL if the table is missing, guarding partial schemas.
        let row: (Option<i64>, Option<f32>) = sqlx::query_as(
            "SELECT \
               CASE WHEN to_regclass($1) IS NULL THEN 0 ELSE pg_total_relation_size($1) END, \
               (SELECT reltuples FROM pg_class WHERE oid = to_regclass($1))",
        )
        .bind(t)
        .fetch_one(pool)
        .await
        .with_context(|| format!("sizing table {t}"))?;
        size_bytes += row.0.unwrap_or(0);
        rows += row.1.unwrap_or(0.0).max(0.0) as i64;
    }
    Ok((size_bytes, rows))
}

/// Size + row estimate for every module and system table, plus the total database size.
pub async fn usage(pool: &PgPool) -> anyhow::Result<DataUsage> {
    let mut modules = Vec::with_capacity(MODULES.len());
    for m in MODULES {
        // Display-only modules own no tables; omit them from data usage/wipe.
        if m.tables.is_empty() {
            continue;
        }
        let (size_bytes, rows) = size_tables(pool, m.tables).await?;
        modules.push(ModuleUsage {
            id: m.id.to_string(),
            name: m.name.to_string(),
            size_bytes,
            rows,
        });
    }

    let mut system = Vec::with_capacity(SYSTEM_TABLES.len());
    for &(table, name) in SYSTEM_TABLES {
        let (size_bytes, rows) = size_tables(pool, &[table]).await?;
        system.push(ModuleUsage {
            id: table.to_string(),
            name: name.to_string(),
            size_bytes,
            rows,
        });
    }

    let db: (i64,) = sqlx::query_as("SELECT pg_database_size(current_database())")
        .fetch_one(pool)
        .await
        .context("sizing database")?;

    Ok(DataUsage { modules, system, database_bytes: db.0 })
}

/// Wipe all data for one module (TRUNCATE … RESTART IDENTITY CASCADE). Returns the module
/// display name on success, or `None` if the id is unknown.
pub async fn wipe_module(pool: &PgPool, id: &str) -> anyhow::Result<Option<String>> {
    let Some(m) = module_by_id(id) else {
        return Ok(None);
    };
    // Display-only modules own no tables; nothing to wipe.
    if m.tables.is_empty() {
        return Ok(Some(m.name.to_string()));
    }
    // Identifiers are compile-time constants from MODULES, never user input.
    let list = m.tables.join(", ");
    let sql = format!("TRUNCATE TABLE {list} RESTART IDENTITY CASCADE");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .with_context(|| format!("wiping module {id}"))?;
    Ok(Some(m.name.to_string()))
}

// ── Installed (available) modules ───────────────────────────────────────────────
//
// Which feature modules are "installed" (browsable/usable) is stored in `app_settings`
// under `installed_modules` as a JSON array of module ids. "Install" makes a module
// available; "detach" hides it (and may wipe its data). This is purely a visibility flag
// — every module's schema always exists; nothing is downloaded or compiled.
//
// On a fresh install the key is absent, which we treat as "all known modules installed",
// so existing deployments keep every module visible until the operator detaches one.

/// Setting key holding the JSON array of installed module ids.
const INSTALLED_KEY: &str = "installed_modules";

/// Returns the set of known module ids in display order (every feature module).
pub fn known_module_ids() -> Vec<String> {
    MODULES.iter().map(|m| m.id.to_string()).collect()
}

/// True if `id` is a known feature module.
pub fn is_known_module(id: &str) -> bool {
    module_by_id(id).is_some()
}

/// The currently installed module ids. Absent setting → all known modules (default-on).
pub async fn installed_modules(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    let raw = crate::settings::get(pool, INSTALLED_KEY).await?;
    let Some(raw) = raw else {
        return Ok(known_module_ids());
    };
    let ids: Vec<String> = serde_json::from_str(&raw).unwrap_or_else(|_| known_module_ids());
    // Keep only ids we still recognize, in registry display order.
    Ok(known_module_ids().into_iter().filter(|k| ids.contains(k)).collect())
}

async fn save_installed(pool: &PgPool, ids: &[String]) -> anyhow::Result<()> {
    let json = serde_json::to_string(ids).context("encoding installed modules")?;
    crate::settings::set(pool, INSTALLED_KEY, &json).await
}

/// Make a module available. No-op if already installed. Returns `false` for unknown ids.
pub async fn install_module(pool: &PgPool, id: &str) -> anyhow::Result<bool> {
    if !is_known_module(id) {
        return Ok(false);
    }
    let mut ids = installed_modules(pool).await?;
    if !ids.iter().any(|i| i == id) {
        ids.push(id.to_string());
        save_installed(pool, &ids).await?;
    }
    Ok(true)
}

/// Detach (hide) a module. When `wipe` is set, its data is also truncated. No-op if not
/// installed. Returns `false` for unknown ids.
pub async fn detach_module(pool: &PgPool, id: &str, wipe: bool) -> anyhow::Result<bool> {
    if !is_known_module(id) {
        return Ok(false);
    }
    let mut ids = installed_modules(pool).await?;
    ids.retain(|i| i != id);
    save_installed(pool, &ids).await?;
    if wipe {
        wipe_module(pool, id).await?;
    }
    Ok(true)
}
