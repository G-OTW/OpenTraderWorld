//! Demo fixtures — `otw-core --seed-demo`.
//!
//! Fills the database pointed at by DATABASE_URL with a small curated showcase dataset
//! (demo account, journal trades, prompts, watchlist, todos/goals, an OpenRouter provider
//! with **no key** — the key comes from the host env at run time). The demo deploy runs
//! this once against a scratch database, then keeps it as the `otw_seed` template that the
//! 15-minute reset restores from. Idempotent: an existing `demo` user short-circuits.
//!
//! Everything here ships in the public repo — no secrets, ever.

use sqlx::PgPool;
use uuid::Uuid;

use crate::auth;

pub async fn seed(pool: &PgPool) -> anyhow::Result<()> {
    if otw_store::find_user_by_username(pool, "demo").await?.is_some() {
        println!("demo user already present — nothing to do");
        return Ok(());
    }

    // The demo account. Auto-login bypasses the password in demo mode; the value is
    // public and irrelevant, it only needs to satisfy the hasher.
    let hash = auth::hash_password("demo-sandbox-resets-every-15min")?;
    otw_store::create_admin(pool, "demo", &hash, false).await?;

    // One statement per query (sqlx prepared statements are single-statement).
    for sql in STATEMENTS {
        sqlx::query(*sql).execute(pool).await?;
    }
    trades(pool).await?;
    agent_token(pool).await?;
    portfolio(pool).await?;
    watchlist_quotes(pool).await?;
    documents(pool).await?;
    av_key(pool).await?;
    datasets(pool).await?;
    chart(pool).await?;
    automator(pool).await?;
    managers(pool).await?;
    Ok(())
}

/// The instruments the chart workspace opens on: the four the seed downloads, each with
/// the studies a trader would actually have on that timeframe. `studies` is the pane's
/// `instances` array, exactly as the editor writes it.
///
/// (provider, asset_type, ticker, timeframe, display name, studies)
const CHART_INSTRUMENTS: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "binance",
        "crypto",
        "BTCUSDT",
        "1h",
        "Bitcoin",
        r#"[{"id":1,"type":"ema","params":{"period":50},"style":{},"visible":true},
            {"id":2,"type":"ema","params":{"period":200},"style":{},"visible":true},
            {"id":3,"type":"rsi","params":{"period":14},"style":{},"visible":true}]"#,
    ),
    (
        "binance",
        "crypto",
        "ETHUSDT",
        "15m",
        "Ether",
        r#"[{"id":1,"type":"bollinger","params":{"period":20,"mult":2},"style":{},"visible":true},
            {"id":2,"type":"macd","params":{"fast":12,"slow":26,"signal":9},"style":{},"visible":true}]"#,
    ),
    (
        "yahoo",
        "equity",
        "NVDA",
        "1h",
        "NVIDIA",
        r#"[{"id":1,"type":"vwap","params":{"period":20},"style":{},"visible":true},
            {"id":2,"type":"atr","params":{"period":14},"style":{},"visible":true}]"#,
    ),
    (
        "yahoo",
        "equity",
        "AAPL",
        "1d",
        "Apple",
        r#"[{"id":1,"type":"sma","params":{"period":50},"style":{},"visible":true},
            {"id":2,"type":"sma","params":{"period":200},"style":{},"visible":true},
            {"id":3,"type":"rsi","params":{"period":14},"style":{},"visible":true}]"#,
    ),
];

/// The chart module: what the workspace opens on, and the rail beside it.
///
/// Runs after [`datasets`] because everything here is measured from the bars that landed:
/// the drawn levels and the alert thresholds come from the seeded series rather than from
/// prices written down in this file, which would be wrong the week after they were typed.
/// An instrument whose download was skipped is simply left out.
async fn chart(pool: &PgPool) -> anyhow::Result<()> {
    // The chart reads through the broker like every data module, and the connectors the
    // migration ships are granted to Historical Data only. The two keyless providers the
    // seed already downloaded from are the two the sandbox can spend freely, so they are
    // the two the chart and the watchlists get: symbol search, panes and alerts all resolve
    // through them, and no credential is involved.
    sqlx::query(
        "INSERT INTO connector_modules (connector_id, module)
         SELECT c.id, m.module
         FROM histdata_connectors c
         CROSS JOIN (VALUES ('histviz'), ('watchlists')) AS m(module)
         WHERE c.provider IN ('binance', 'yahoo')
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await?;

    let wanted = CHART_INSTRUMENTS;

    let mut panes: Vec<serde_json::Value> = Vec::new();
    let mut crypto: Vec<serde_json::Value> = Vec::new();
    let mut equity: Vec<serde_json::Value> = Vec::new();

    for (i, (provider, asset_type, ticker, timeframe, name, studies)) in
        wanted.iter().enumerate()
    {
        let Some((hi, lo)) = range(pool, provider, asset_type, ticker, timeframe).await? else {
            eprintln!("chart: {ticker} {timeframe} has no bars — left out of the workspace");
            continue;
        };
        let instances: serde_json::Value = serde_json::from_str(studies)?;
        // Two horizontal lines on the quarter's extremes: the drawing layer, on levels the
        // data actually has, rather than a demo that opens on a bare candlestick chart.
        let drawings = json_drawings(hi, lo);
        sqlx::query(
            "INSERT INTO histviz_instrument_layouts (coord_key, layout)
             VALUES ($1, $2)
             ON CONFLICT (coord_key) DO NOTHING",
        )
        .bind(coord_key(provider, asset_type, ticker, timeframe))
        .bind(serde_json::json!({
            "type": "candlestick",
            "brick": 0,
            "instances": instances,
            "drawings": drawings,
        }))
        .execute(pool)
        .await?;

        let coords = serde_json::json!({
            "provider": provider,
            "asset_type": asset_type,
            "ticker": ticker,
            "timeframe": timeframe,
            "name": name,
        });
        panes.push(serde_json::json!({
            "id": format!("p{}", i + 1),
            "coords": coords,
            "type": "candlestick",
            "brick": 0,
            "instances": [],
            // The two crypto panes move together; the two equities keep their own span.
            "link": if *asset_type == "crypto" { "amber" } else { "" },
            "connector_id": null,
        }));
        let entry = serde_json::json!({
            "provider": provider,
            "asset_type": asset_type,
            "ticker": ticker,
            "timeframe": timeframe,
            "connector_id": null,
            "name": name,
        });
        if *asset_type == "crypto" {
            crypto.push(entry);
        } else {
            equity.push(entry);
        }
    }

    if panes.is_empty() {
        eprintln!("chart: no dataset landed — workspace, lists and alerts skipped");
        return Ok(());
    }

    // A 2x2, because that is what the module is for: four instruments at once, on one
    // screen, two of them linked.
    let rows = if panes.len() > 2 { 2 } else { 1 };
    let cols = if panes.len() > 1 { 2 } else { 1 };
    sqlx::query(
        "INSERT INTO histviz_workspaces (name, grid_rows, grid_cols, panes, settings, position)
         VALUES ($1, $2, $3, $4, $5, 0)",
    )
    .bind("Majors")
    .bind(rows)
    .bind(cols)
    .bind(serde_json::Value::Array(panes.clone()))
    .bind(serde_json::json!({
        "row_sizes": vec![1; rows as usize],
        "col_sizes": vec![1; cols as usize],
        "active": panes[0]["id"].clone(),
    }))
    .execute(pool)
    .await?;

    for (position, (name, items)) in
        [("Crypto majors", crypto), ("US tech", equity)].into_iter().enumerate()
    {
        if items.is_empty() {
            continue;
        }
        sqlx::query("INSERT INTO histviz_lists (name, items, position) VALUES ($1, $2, $3)")
            .bind(name)
            .bind(serde_json::Value::Array(items))
            .bind(position as i32)
            .execute(pool)
            .await?;
    }

    alerts(pool).await?;
    println!("chart: workspace, lists and layouts seeded");
    Ok(())
}

/// `provider|asset_type|ticker|timeframe`, the key `histviz_instrument_layouts` is filed
/// under: provider/asset/timeframe lowercased, the ticker verbatim (case matters, and only
/// to the provider).
fn coord_key(provider: &str, asset_type: &str, ticker: &str, timeframe: &str) -> String {
    format!(
        "{}|{}|{}|{}",
        provider.to_lowercase(),
        asset_type.to_lowercase(),
        ticker,
        timeframe.to_lowercase()
    )
}

/// The high and low of the last 90 days of a seeded instrument, or `None` when the download
/// was skipped. Cast to float8: the column is NUMERIC and nothing here needs its precision.
async fn range(
    pool: &PgPool,
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
) -> anyhow::Result<Option<(f64, f64)>> {
    let row: Option<(Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT max(b.high)::float8, min(b.low)::float8
         FROM histdata_bars b
         JOIN histdata_datasets d ON d.id = b.dataset_id
         WHERE d.provider = $1 AND d.asset_type = $2 AND d.ticker = $3 AND d.timeframe = $4
           AND b.ts > now() - interval '90 days'",
    )
    .bind(provider)
    .bind(asset_type)
    .bind(ticker)
    .bind(timeframe)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some((Some(hi), Some(lo))) if hi > lo => Some((hi, lo)),
        _ => None,
    })
}

/// The quarter's range as two horizontal lines, in the drawing model's own coordinates
/// (`x` an epoch-ms instant, `y` a price). An hline only reads its `y`, but the anchor is
/// stored whole so the row is the same shape the editor writes.
fn json_drawings(hi: f64, lo: f64) -> serde_json::Value {
    let now_ms = time::OffsetDateTime::now_utc().unix_timestamp() * 1000;
    serde_json::json!([
        { "id": "d-range-high", "tool": "hline", "a": { "x": now_ms, "y": round_level(hi) },
          "b": null, "style": { "color": "", "width": 0 } },
        { "id": "d-range-low", "tool": "hline", "a": { "x": now_ms, "y": round_level(lo) },
          "b": null, "style": { "color": "", "width": 0 } },
    ])
}

/// A level a human would have drawn: three significant digits, so 108_437.21 becomes
/// 108_000 and 236.83 becomes 237.
fn round_level(v: f64) -> f64 {
    if v <= 0.0 {
        return v;
    }
    let magnitude = 10f64.powf(v.abs().log10().floor() - 2.0);
    (v / magnitude).round() * magnitude
}

/// Two chart alerts, on levels outside the seeded range.
///
/// Deliberately *outside* it: an alert placed where the market already is fires on the
/// first pass, and every 15-minute reset would replay that same notification. Placed a few
/// percent beyond the quarter's extremes it demonstrates the feature — the row, the level,
/// the evaluation bookkeeping — and stays quiet unless the market really gets there.
async fn alerts(pool: &PgPool) -> anyhow::Result<()> {
    // (ticker, provider, asset_type, timeframe, name, op, level from the range)
    let wanted: &[(&str, &str, &str, &str, &str, &str, f64, bool)] = &[
        ("BTCUSDT", "binance", "crypto", "1h", "BTC breaks the quarter's high", "above", 1.05, true),
        ("AAPL", "yahoo", "equity", "1d", "AAPL loses the quarter's low", "below", 0.95, false),
    ];
    for (ticker, provider, asset_type, timeframe, name, op, factor, from_high) in wanted {
        let Some((hi, lo)) = range(pool, provider, asset_type, ticker, timeframe).await? else {
            continue;
        };
        let level = round_level(if *from_high { hi * factor } else { lo * factor });
        sqlx::query(
            "INSERT INTO histviz_alerts
                 (name, provider, asset_type, ticker, timeframe, kind, source, op, value,
                  repeat, cooldown_secs, enabled)
             VALUES ($1, $2, $3, $4, $5, 'price', 'close', $6, $7, false, 3600, true)",
        )
        .bind(name)
        .bind(provider)
        .bind(asset_type)
        .bind(ticker)
        .bind(timeframe)
        .bind(op)
        .bind(level)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// One workflow, its first revision, a schedule and two runs of history.
///
/// The graph is **validated here** rather than trusted: a block config that stops being
/// valid must fail the seed loudly, not ship a workflow the editor refuses to save. The
/// schedule is left inactive on purpose — in the sandbox the automator is read-only, so
/// nothing can run it by hand, and an active rule would fire on the demo host every night
/// for a visitor who left hours ago.
/// The seeded workflow's graph, in the shape the editor saves.
///
/// Split out so it can be validated in a test as well as at seed time: the demo must never
/// ship a workflow the module itself would refuse.
fn briefing_graph() -> serde_json::Value {
    serde_json::json!({
        "nodes": [
            { "id": "news", "kind": "api", "name": "Overnight headlines",
              "config": { "method": "GET", "path": "/api/feed-items?limit=8" },
              "pos": { "x": 40, "y": 40 } },
            { "id": "risk", "kind": "api", "name": "Open risk",
              "config": { "method": "GET", "path": "/api/journal/exposure" },
              "pos": { "x": 40, "y": 240 } },
            { "id": "worth_sending", "kind": "if", "name": "Anything to say?",
              "config": { "match": "any", "conditions": [
                  { "left": "{{steps.news.output.body.items}}", "op": "not_empty" }
              ] },
              "pos": { "x": 320, "y": 140 } },
            { "id": "brief", "kind": "agent", "name": "Write the briefing",
              "config": {
                  "input": "Write a six-line pre-market note for a discretionary trader.\n\nHeadlines: {{steps.news.output.body.items}}\n\nOpen risk right now: {{steps.risk.output.body}}\n\nSay what changed overnight, then what it means for the positions already on. No preamble.",
                  "output": "text"
              },
              "pos": { "x": 600, "y": 80 } },
            { "id": "push", "kind": "notify", "name": "Send it",
              "config": { "title": "Pre-market briefing", "body": "{{steps.brief.output.text}}" },
              "pos": { "x": 880, "y": 80 } }
        ],
        "edges": [
            { "from": "news", "to": "worth_sending", "port": "out" },
            { "from": "risk", "to": "worth_sending", "port": "out" },
            { "from": "worth_sending", "to": "brief", "port": "true" },
            { "from": "brief", "to": "push", "port": "out" }
        ]
    })
}

async fn automator(pool: &PgPool) -> anyhow::Result<()> {
    let graph = briefing_graph();
    let parsed = crate::automator::Graph::parse(&graph)
        .map_err(|e| anyhow::anyhow!("demo workflow: {e}"))?;
    crate::automator::validate(&parsed).map_err(|e| anyhow::anyhow!("demo workflow: {e}"))?;

    let workflow = Uuid::new_v4();
    let version = Uuid::new_v4();
    // The same read-only envelope the demo agent runs under: the two `api` blocks read, and
    // nothing in the catalog can be written through this workflow.
    let perms = serde_json::Value::Object(
        crate::mcp::catalog::MODULES
            .iter()
            .map(|(id, _)| ((*id).to_string(), serde_json::Value::String("r".into())))
            .collect(),
    );
    let (token, _plaintext_discarded) =
        otw_store::mcp::create_token(pool, "Demo workflow (internal, read-only)", &perms, None)
            .await?;

    sqlx::query(
        "INSERT INTO automator_workflows
             (id, name, description, graph, mcp_token_id, favorite, enabled, max_runtime_secs)
         VALUES ($1, $2, $3, $4, $5, TRUE, TRUE, 120)",
    )
    .bind(workflow)
    .bind("Pre-market briefing")
    .bind("Reads the overnight headlines and the open risk, has the agent write the note, pushes it to the notification channels.")
    .bind(&graph)
    .bind(token.id)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO automator_versions (id, workflow_id, rev, graph, note)
         VALUES ($1, $2, 1, $3, 'First version')",
    )
    .bind(version)
    .bind(workflow)
    .bind(&graph)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE automator_workflows SET version_id = $1 WHERE id = $2")
        .bind(version)
        .bind(workflow)
        .execute(pool)
        .await?;

    // Inactive, and with no `next_run_at`: the row shows how a workflow is scheduled, and
    // the scheduler never claims it.
    sqlx::query(
        "INSERT INTO automator_schedules
             (id, workflow_id, kind, timezone, at_hour, at_minute, weekdays, active)
         VALUES ($1, $2, 'weekly', 'Europe/Paris', 8, 30, 31, FALSE)",
    )
    .bind(Uuid::new_v4())
    .bind(workflow)
    .execute(pool)
    .await?;

    workflow_runs(pool, workflow, version).await?;
    println!("automator: workflow, revision, schedule and run history seeded");
    Ok(())
}

/// Two finished runs, so the history and the step trace are not an empty table.
async fn workflow_runs(pool: &PgPool, workflow: Uuid, version: Uuid) -> anyhow::Result<()> {
    // (days ago, status, the branch the condition took)
    let runs: &[(i32, &str, bool)] = &[(1, "ok", true), (8, "ok", false)];
    for (days, status, sent) in runs {
        let run = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO automator_runs
                 (id, workflow_id, version_id, trigger, mode, status, started_at, finished_at,
                  duration_ms)
             VALUES ($1, $2, $3, 'schedule', 'live', $4,
                     now() - ($5 || ' days')::interval,
                     now() - ($5 || ' days')::interval + interval '9 seconds', 9120)",
        )
        .bind(run)
        .bind(workflow)
        .bind(version)
        .bind(status)
        .bind(days.to_string())
        .execute(pool)
        .await?;

        let mut steps: Vec<(&str, &str, &str, serde_json::Value)> = vec![
            ("news", "Overnight headlines", "api",
             serde_json::json!({ "status": 200, "body": { "items": if *sent { 8 } else { 0 } } })),
            ("risk", "Open risk", "api",
             serde_json::json!({ "status": 200, "body": { "positions": 2, "risk_pct": 1.8 } })),
            ("worth_sending", "Anything to say?", "if", serde_json::json!({ "result": sent })),
        ];
        if *sent {
            steps.push((
                "brief",
                "Write the briefing",
                "agent",
                serde_json::json!({ "text": "Overnight: risk-on, majors up with the open. Two positions on, 1.8% of the book at risk; the BTC leg is the one that moves if the level breaks." }),
            ));
            steps.push((
                "push",
                "Send it",
                "notify",
                serde_json::json!({ "sent": 1 }),
            ));
        }
        for (seq, (node_id, node_name, kind, output)) in steps.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO automator_run_steps
                     (id, run_id, node_id, node_name, kind, seq, status, request, output,
                      duration_ms, started_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, '{}'::jsonb, $8, $9,
                         now() - ($10 || ' days')::interval)",
            )
            .bind(Uuid::new_v4())
            .bind(run)
            .bind(node_id)
            .bind(node_name)
            .bind(kind)
            .bind(seq as i32)
            // The condition's No branch skips nothing here: the blocks after it simply
            // never ran, which is what an absent step means in the trace.
            .bind("ok")
            .bind(output)
            .bind(400 + (seq as i32) * 900)
            .bind(days.to_string())
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// Scrape the Dataroma superinvestor cache once, at seed time.
///
/// The module is normally filled by `mportfolios_job` (twice-weekly, jittered). In the demo
/// that loop is useless — every 15-minute reset would throw its work away — so we sync here
/// instead: the rows land in the `otw_seed` template and each reset restores them intact,
/// with no runtime egress to Dataroma from the shared host.
///
/// Best-effort, exactly like `datasets`: an upstream that is down or has changed its markup
/// must not fail the whole seed.
async fn managers(pool: &PgPool) -> anyhow::Result<()> {
    let lock = crate::mportfolios_job::new_lock();
    match crate::mportfolios_job::refresh(pool, &lock).await {
        Ok(()) => {
            let n: i64 = sqlx::query_scalar("SELECT count(*) FROM manager_portfolios")
                .fetch_one(pool)
                .await?;
            println!("managers' portfolios: {n} synced");
        }
        Err(e) => eprintln!("managers' portfolios: skipped ({e:#})"),
    }
    Ok(())
}

/// Historical datasets for the backtester, downloaded from the **keyless** providers at
/// seed time (Binance for crypto, Yahoo for equities) so the seed ships no credential and
/// no multi-MB blob in git. ~58k bars total, a few MB — cheap to keep in `otw_seed` and
/// therefore restored intact by every 15-minute reset, with no runtime egress.
///
/// Best-effort per dataset: a provider that is down or rate-limiting must not fail the
/// whole seed (the demo is still worth shipping without one series), so each failure is
/// logged and skipped. A dataset that ends up empty is deleted rather than left as an
/// empty entry in the backtester's picker.
async fn datasets(pool: &PgPool) -> anyhow::Result<()> {
    // (provider, asset_type, ticker, timeframe, days back)
    let wanted: &[(&str, &str, &str, &str, i64)] = &[
        ("binance", "crypto", "BTCUSDT", "1h", 730),
        ("binance", "crypto", "ETHUSDT", "15m", 365),
        ("yahoo", "equity", "NVDA", "1h", 730),
        ("yahoo", "equity", "AAPL", "1d", 3650),
    ];
    for (provider, asset_type, ticker, timeframe, days) in wanted {
        match one_dataset(pool, provider, asset_type, ticker, timeframe, *days).await {
            Ok(n) => println!("dataset {ticker} {timeframe}: {n} bars"),
            Err(e) => eprintln!("dataset {ticker} {timeframe}: skipped ({e:#})"),
        }
    }
    Ok(())
}

async fn one_dataset(
    pool: &PgPool,
    provider: &str,
    asset_type: &str,
    ticker: &str,
    timeframe: &str,
    days: i64,
) -> anyhow::Result<i64> {
    use time::OffsetDateTime;

    let connector = crate::histdata::connector_for(provider)?;
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; OpenTraderWorld demo seed)")
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let secrets = std::collections::HashMap::new();

    let to = OffsetDateTime::now_utc();
    let from = to - time::Duration::days(days);
    let dataset = otw_store::histdata::upsert_dataset(pool, provider, asset_type, ticker, timeframe)
        .await?;

    // Connectors return at most `max_bars_per_req` per call, so page forward until the
    // window is covered. A chunk that returns nothing means the provider has no more
    // history (Yahoo caps intraday depth) — stop rather than spin.
    let mut cursor = from;
    let mut total = 0i64;
    while cursor < to {
        let chunk = match connector
            .fetch_chunk(&client, &secrets, ticker, asset_type, timeframe, cursor, to)
            .await
        {
            Ok(c) => c,
            // Yahoo answers a window containing no session (a weekend, a holiday, or a
            // range past its intraday depth) with a result that simply omits the
            // `timestamp` array, which the connector reports as "no timestamps". For a
            // paging loop that means "nothing here", not a failure — treat it as the end
            // of available history instead of throwing away the bars already fetched.
            Err(e) if e.to_string().contains("no timestamps") => break,
            Err(e) => return Err(e),
        };
        if chunk.bars.is_empty() {
            break;
        }
        let last = chunk.bars.last().map(|b| b.ts).unwrap_or(cursor);
        total += otw_store::histdata::write_bars(pool, dataset, &chunk.bars).await? as i64;
        // Strictly advance past the last bar; equal timestamps would loop forever.
        let next = last + time::Duration::seconds(1);
        if next <= cursor {
            break;
        }
        cursor = next;
        // Be a polite guest on keyless public endpoints.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    if total == 0 {
        sqlx::query("DELETE FROM histdata_datasets WHERE id = $1")
            .bind(dataset)
            .execute(pool)
            .await?;
        anyhow::bail!("provider returned no bars");
    }
    sqlx::query("UPDATE histdata_datasets SET status = 'complete' WHERE id = $1")
        .bind(dataset)
        .execute(pool)
        .await?;
    Ok(total)
}

/// Arm the AlphaVantage feed if `OTW_DEMO_AV_KEY` is set on the seeding host.
///
/// The key is sealed into `feed_secrets` (AEAD, same path as the UI) and the feed's
/// config only ever references `{{secret:api_key}}` — so nothing secret reaches the
/// repo, the seed SQL, or `otw_seed`'s public description. No env var, no key: the feed
/// simply stays disabled rather than polling with a broken credential.
async fn av_key(pool: &PgPool) -> anyhow::Result<()> {
    let Some(key) = std::env::var("OTW_DEMO_AV_KEY").ok().filter(|k| !k.trim().is_empty()) else {
        println!("OTW_DEMO_AV_KEY unset — AlphaVantage feed left disabled");
        return Ok(());
    };
    let master = std::env::var("OTW_SECRET_KEY")
        .map_err(|_| anyhow::anyhow!("OTW_SECRET_KEY is required to seal the AlphaVantage key"))?;
    let cipher = otw_store::crypto::SecretCipher::from_master(&master)?;

    let id: Uuid = sqlx::query_scalar("SELECT id FROM feeds WHERE name = $1")
        .bind("AlphaVantage — Market news")
        .fetch_one(pool)
        .await?;
    otw_store::feeds::set_secret(pool, &cipher, id, "api_key", key.trim()).await?;
    sqlx::query("UPDATE feeds SET enabled = TRUE WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    // The dashboard link is what makes the scheduler poll it (`claim_due_feeds` joins
    // through dashboard_sources and does NOT check `enabled`) — which is exactly why the
    // link is added here and not in the statement block: without a key the feed must stay
    // unlinked, or the scheduler would hammer AlphaVantage with `{{secret:api_key}}`.
    sqlx::query(
        "INSERT INTO dashboard_sources (dashboard_id, feed_id, interval_secs, position)
         SELECT d.id, $1, 3600, 99 FROM feed_dashboards d WHERE d.is_default
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .execute(pool)
    .await?;

    // `next_run_at` is a seeded column, so the 15-minute reset restores whatever value is
    // frozen here. Left at the default (now, i.e. in the past by restore time) the feed is
    // due the instant the snapshot lands and polls 96×/day — blowing the 25/day free tier
    // before breakfast. Parking it a full interval ahead means a poll only happens when a
    // demo instance stays up past the reset, capping real egress at ~24/day.
    sqlx::query("UPDATE feeds SET next_run_at = now() + interval '1 hour' WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    println!("AlphaVantage feed armed (key sealed, hourly)");
    Ok(())
}

/// Give the demo agent its tools: a read-only MCP token wired to the default agent.
/// The plaintext is minted and **discarded** — it exists nowhere, so the token can never
/// authenticate an external `/api/mcp` call (which the demo gate blocks anyway); only the
/// in-process agent dispatch, which resolves the token by id, can use it. Because the row
/// lives in the `otw_seed` template, every 15-minute reset restores the exact same token.
async fn agent_token(pool: &PgPool) -> anyhow::Result<()> {
    let perms = serde_json::Value::Object(
        crate::mcp::catalog::MODULES
            .iter()
            .map(|(id, _)| ((*id).to_string(), serde_json::Value::String("r".into())))
            .collect(),
    );
    let (row, _plaintext_discarded) =
        otw_store::mcp::create_token(pool, "Demo agent (internal, read-only)", &perms, None).await?;
    sqlx::query("UPDATE agent_agents SET mcp_token_id = $1 WHERE is_default")
        .bind(row.id)
        .execute(pool)
        .await?;
    Ok(())
}

const STATEMENTS: &[&str] = &[
    // ── Journal: categories, capital, strategies, fees ───────────────────────
    "INSERT INTO journal_categories (id, name, color, position)
     VALUES (gen_random_uuid(), 'Crypto', '#f7931a', 1)",
    "INSERT INTO journal_capital_events (id, category_id, kind, amount, currency, occurred_at)
     SELECT gen_random_uuid(), id, 'initial', 10000, 'USD', now() - interval '90 days'
     FROM journal_categories WHERE is_default",
    "INSERT INTO journal_capital_events (id, category_id, kind, amount, currency, occurred_at)
     SELECT gen_random_uuid(), id, 'initial', 5000, 'USD', now() - interval '90 days'
     FROM journal_categories WHERE name = 'Crypto'",
    "INSERT INTO journal_strategies (id, name, description, signals, position) VALUES
       (gen_random_uuid(), 'Breakout', 'Range break with volume confirmation',
        '[\"Range break\", \"Volume surge\"]'::jsonb, 0),
       (gen_random_uuid(), 'Mean reversion', 'Fade extended moves back to VWAP',
        '[\"VWAP fade\", \"RSI extreme\"]'::jsonb, 1)",
    "INSERT INTO journal_fee_schedules (id, name, amount, amount_kind, per, currency, position)
     VALUES (gen_random_uuid(), 'Broker flat', 1.5, 'fixed', 'trade', 'USD', 0)",
    // Discipline tags: a short list of things this demo trader actually does, so the
    // analytics screen has a real cost-of-mistakes figure instead of an empty panel.
    "INSERT INTO journal_tags (id, name, kind, description, position) VALUES
       (gen_random_uuid(), 'Moved my stop', 'mistake', 'Widened the stop once it was hit', 0),
       (gen_random_uuid(), 'Exited early', 'mistake', 'Closed before the plan said to', 1),
       (gen_random_uuid(), 'Overtraded', 'mistake', 'Took a trade with no edge, out of boredom', 2),
       (gen_random_uuid(), 'Followed the plan', 'rule', 'Entry, stop and exit as written', 3),
       (gen_random_uuid(), 'Breakout continuation', 'setup', 'Second push after a clean range break', 4)",
    // ── Prompt store: prompts + their version-1 history rows ─────────────────
    "WITH new_prompts AS (
         INSERT INTO prompt_store_prompts (id, name, body, tags, vote)
         VALUES
           (gen_random_uuid(), 'Trade post-mortem',
            E'Analyze this closed trade like a trading coach.\\n\\nSetup: {{setup}}\\nOutcome: {{outcome}}\\n\\nList: what was done well, what broke the plan, and one rule to add.',
            ARRAY['journal','review'], 1),
           (gen_random_uuid(), 'Earnings summary',
            E'Summarize the following earnings report in 5 bullets: revenue vs consensus, margin trend, guidance, one risk, one catalyst.\\n\\n{{report}}',
            ARRAY['research'], 1),
           (gen_random_uuid(), 'Risk check',
            E'Given account size {{size}}, risk per trade {{risk_pct}}% and stop distance {{stop}}, compute position size and validate against max exposure rules.',
            ARRAY['risk','sizing'], 0)
         RETURNING id, name, body, tags
     )
     INSERT INTO prompt_store_versions (id, prompt_id, version, name, body, tags)
     SELECT gen_random_uuid(), id, 1, name, body, tags FROM new_prompts",
    // ── Watchlist (sync off: the sandbox never polls providers on its own) ───
    "WITH wl AS (
         INSERT INTO watchlists (id, name, description, sync_enabled, position)
         VALUES (gen_random_uuid(), 'Core holdings',
                 'Demo watchlist — seeded quotes; hit refresh for live prices', FALSE, 0)
         RETURNING id
     )
     INSERT INTO watchlist_items (id, watchlist_id, asset_class, provider, provider_id, symbol, name, position)
     SELECT gen_random_uuid(), wl.id, v.class, v.provider, v.pid, v.symbol, v.name, v.pos
     FROM wl, (VALUES
         ('crypto', 'coingecko', 'bitcoin',  'BTC',  'Bitcoin',     0.0),
         ('crypto', 'coingecko', 'ethereum', 'ETH',  'Ethereum',    1.0),
         ('stock',  'yahoo',     'AAPL',     'AAPL', 'Apple',       2.0),
         ('etf',    'yahoo',     'SPY',      'SPY',  'S&P 500 ETF', 3.0)
     ) AS v(class, provider, pid, symbol, name, pos)",
    // ── Subscriptions (a trader's recurring tool spend, mixed currencies/cadences) ──
    "INSERT INTO subscriptions (id, name, platform, url, price, currency, frequency, category, started_on) VALUES
       (gen_random_uuid(), 'TradingView Premium', 'TradingView', 'https://tradingview.com',
        59.95, 'USD', 'monthly', 'Charting', current_date - 400),
       (gen_random_uuid(), 'Interactive Brokers market data', 'IBKR', NULL,
        14.00, 'USD', 'monthly', 'Market data', current_date - 300),
       (gen_random_uuid(), 'Koyfin Plus', 'Koyfin', 'https://koyfin.com',
        468.00, 'USD', 'yearly', 'Research', current_date - 210),
       (gen_random_uuid(), 'Financial Times', 'FT', 'https://ft.com',
        39.00, 'EUR', 'monthly', 'News', current_date - 150),
       (gen_random_uuid(), 'VPS — strategy runner', 'Hetzner', NULL,
        16.50, 'EUR', 'monthly', 'Infrastructure', current_date - 120),
       (gen_random_uuid(), 'Notion', 'Notion', NULL,
        10.00, 'USD', 'monthly', 'Notes', current_date - 500),
       (gen_random_uuid(), 'Substack — macro letter', NULL, NULL,
        180.00, 'USD', 'yearly', 'Research', current_date - 95),
       (gen_random_uuid(), 'Old backtesting tool', NULL, NULL,
        29.00, 'USD', 'monthly', 'Research', current_date - 620)",
    // One cancelled sub so the active/inactive split is visible.
    "UPDATE subscriptions SET active = FALSE WHERE name = 'Old backtesting tool'",
    // ── News feeds ───────────────────────────────────────────────────────────
    // Real public RSS endpoints — the scheduler polls these in the demo, so the news
    // module fills with genuine headlines instead of canned rows. Feed *management* is
    // read-only in demo (SSRF via the scheduler), so a visitor can browse and refresh
    // but never repoint one of these at an arbitrary URL.
    "INSERT INTO feeds (id, name, kind, config, enabled, interval_secs) VALUES
       (gen_random_uuid(), 'Reuters — Business', 'rss',
        '{\"url\":\"https://ir.thomsonreuters.com/rss/news-releases.xml?items=15\"}'::jsonb,
        TRUE, 3600),
       (gen_random_uuid(), 'Federal Reserve — Press releases', 'rss',
        '{\"url\":\"https://www.federalreserve.gov/feeds/press_all.xml\"}'::jsonb,
        TRUE, 3600),
       (gen_random_uuid(), 'ECB — Press releases', 'rss',
        '{\"url\":\"https://www.ecb.europa.eu/rss/press.html\"}'::jsonb,
        TRUE, 3600)",
    // AlphaVantage NEWS_SENTIMENT as an `api` feed. The key is NOT here: config
    // references `{{secret:api_key}}`, which the fetcher substitutes from the encrypted
    // `feed_secrets` row written by `seed_av_key()` from OTW_DEMO_AV_KEY (host env).
    // Disabled when that env var is absent, so a keyless deploy shows no broken feed.
    // Hourly: the free tier allows 25 requests/day, and 24 polls fits under it — but see
    // the `next_run_at` parking below, without which the reset cadence sets the real rate.
    "INSERT INTO feeds (id, name, kind, config, enabled, interval_secs) VALUES
       (gen_random_uuid(), 'AlphaVantage — Market news', 'api',
        '{\"url\":\"https://www.alphavantage.co/query\",
          \"method\":\"GET\",
          \"query\":{\"function\":\"NEWS_SENTIMENT\",\"topics\":\"financial_markets\",
                     \"sort\":\"LATEST\",\"limit\":\"25\",\"apikey\":\"{{secret:api_key}}\"},
          \"items_path\":\"feed\",
          \"title_path\":\"title\",
          \"url_path\":\"url\",
          \"date_path\":\"time_published\",
          \"summary_path\":\"summary\",
          \"source_path\":\"source\"}'::jsonb,
        FALSE, 3600)",
    // A *started* dashboard is what actually drives polling: `claim_due_feeds` joins
    // through `dashboard_sources` and ignores a feed's own `enabled` flag, so a source
    // with no started dashboard is never fetched. Migration 0030 already creates the
    // started default dashboard (and links whatever feeds existed then — none, since it
    // runs before this seed), so rename it and link ours rather than inserting a second
    // default, which the `uq_feed_dashboards_default` partial index rejects.
    // The per-link interval_secs (not the feed column) sets the cadence for an instance
    // that stays up: 3600s. What bounds total egress across resets is `next_run_at`.
    "UPDATE feed_dashboards SET name = 'Markets', started = TRUE, favorite = TRUE
     WHERE is_default",
    "INSERT INTO dashboard_sources (dashboard_id, feed_id, interval_secs, position)
     SELECT d.id, f.id, 3600,
            row_number() OVER (ORDER BY f.name)::int
     FROM feed_dashboards d, feeds f
     WHERE d.is_default AND f.enabled
     ON CONFLICT DO NOTHING",
    // Park every seeded feed one interval out. `next_run_at` is restored verbatim by the
    // 15-minute reset, so a seed-time default (already in the past) makes each feed due the
    // moment the snapshot lands — 96 fetches/day per publisher instead of 24, from a public
    // demo, which is a good way to get the demo's IP blocked by Reuters/the Fed/the ECB.
    "UPDATE feeds SET next_run_at = now() + interval '1 hour'",
    // ── Todos & goals ────────────────────────────────────────────────────────
    "INSERT INTO todos (id, name, due_date, details, done) VALUES
       (gen_random_uuid(), 'Review last week''s trades', current_date + 1, 'Tag each with a strategy and one lesson.', FALSE),
       (gen_random_uuid(), 'Update watchlist for earnings season', current_date + 3, '', FALSE),
       (gen_random_uuid(), 'Backtest the breakout tweak', current_date + 7, 'Wider stop, same target — check expectancy.', FALSE),
       (gen_random_uuid(), 'Journal the ETH loss', current_date - 2, 'Done during weekend review.', TRUE)",
    "INSERT INTO goals (id, name, deadline, details, kpis) VALUES
       (gen_random_uuid(), 'Positive expectancy quarter', current_date + 60,
        'Three consecutive months with positive expectancy across all strategies.',
        '[{\"name\":\"Win rate\",\"target\":\"55%\"},{\"name\":\"Avg R\",\"target\":\"1.8\"}]'::jsonb),
       (gen_random_uuid(), 'Journal every trade', current_date + 30,
        'No unlogged fills for 30 days straight.',
        '[{\"name\":\"Logged trades\",\"target\":\"100%\"}]'::jsonb)",
    // ── MyWealth: a small net-worth sheet across asset types ─────────────────
    // Assets are template-less (the module's reserved price/quantity live on the revision),
    // mixed currencies so the FX-converted breakdown has something to convert, and each one
    // carries several revisions so the net-worth curve is a curve and not a single point.
    "INSERT INTO wealth_assets (id, name, asset_type, currency, category) VALUES
       (gen_random_uuid(), 'Cash — main account', 'money',  'USD', 'Liquid'),
       (gen_random_uuid(), 'Cash — EUR savings',  'money',  'EUR', 'Liquid'),
       (gen_random_uuid(), 'Brokerage — ETF sleeve', 'stock',  'USD', 'Invested'),
       (gen_random_uuid(), 'Cold wallet — BTC',   'crypto', 'USD', 'Invested'),
       (gen_random_uuid(), 'Apartment',           'house',  'EUR', 'Real estate'),
       (gen_random_uuid(), 'Speedmaster',         'watch',  'EUR', 'Collectibles')",
    // Quarterly revisions over the last year. `value` is what the breakdown sums; price and
    // quantity are set only where they mean something (BTC, the ETF sleeve).
    "INSERT INTO wealth_revisions (id, asset_id, valued_at, price, quantity, value, note)
     SELECT gen_random_uuid(), a.id, current_date - v.days_ago,
            v.price, v.qty, v.value, v.note
     FROM wealth_assets a, (VALUES
         ('Cash — main account',    360, NULL::float8, NULL::float8, 18400.0, 'Opening balance'),
         ('Cash — main account',    270, NULL, NULL, 21250.0, ''),
         ('Cash — main account',    180, NULL, NULL, 19800.0, 'Paid the tax bill'),
         ('Cash — main account',     90, NULL, NULL, 24600.0, ''),
         ('Cash — main account',      5, NULL, NULL, 26150.0, ''),
         ('Cash — EUR savings',     360, NULL, NULL, 12000.0, ''),
         ('Cash — EUR savings',     180, NULL, NULL, 14500.0, ''),
         ('Cash — EUR savings',       5, NULL, NULL, 16250.0, ''),
         ('Brokerage — ETF sleeve', 360, 118.20, 85.0, 10047.0, ''),
         ('Brokerage — ETF sleeve', 180, 129.60, 85.0, 11016.0, ''),
         ('Brokerage — ETF sleeve',   5, 141.75, 85.0, 12048.75, 'Same line as the tracker'),
         ('Cold wallet — BTC',      360, 68400.0, 0.35, 23940.0, ''),
         ('Cold wallet — BTC',      180, 91200.0, 0.35, 31920.0, ''),
         ('Cold wallet — BTC',        5, 108400.0, 0.35, 37940.0, ''),
         ('Apartment',              360, NULL, NULL, 268000.0, 'Notary estimate'),
         ('Apartment',                5, NULL, NULL, 279000.0, 'Local comparables'),
         ('Speedmaster',            360, NULL, NULL, 5200.0, ''),
         ('Speedmaster',              5, NULL, NULL, 5650.0, '')
     ) AS v(asset, days_ago, price, qty, value, note)
     WHERE a.name = v.asset",
    // ── Calendar: personal events around the trading week ────────────────────
    // Anchored on current_date so the month view always opens on a populated month, with a
    // mix of past and upcoming, timed and all-day, plus one multi-day block.
    "INSERT INTO calendar_events (id, title, start_at, end_at, all_day, category, color, location, notes)
     VALUES
       (gen_random_uuid(), 'Weekly review',
        (current_date - 4 + time '17:00') AT TIME ZONE 'UTC',
        (current_date - 4 + time '18:00') AT TIME ZONE 'UTC',
        FALSE, 'Routine', '#3b82f6', '', 'Tag every fill, update the playbook.'),
       (gen_random_uuid(), 'CPI release',
        (current_date + 1 + time '13:30') AT TIME ZONE 'UTC',
        (current_date + 1 + time '14:00') AT TIME ZONE 'UTC',
        FALSE, 'Macro', '#ef4444', '', 'Flat into the print — no new risk 30 min before.'),
       (gen_random_uuid(), 'NVDA earnings',
        (current_date + 3 + time '21:00') AT TIME ZONE 'UTC',
        (current_date + 3 + time '22:00') AT TIME ZONE 'UTC',
        FALSE, 'Earnings', '#f59e0b', '', 'After the close.'),
       (gen_random_uuid(), 'FOMC decision',
        (current_date + 8 + time '18:00') AT TIME ZONE 'UTC',
        (current_date + 8 + time '19:00') AT TIME ZONE 'UTC',
        FALSE, 'Macro', '#ef4444', '', ''),
       (gen_random_uuid(), 'Monthly journal export',
        current_date + 12, NULL, TRUE, 'Routine', '#22c55e', '', 'Archive the month and back it up.'),
       (gen_random_uuid(), 'Quant workshop',
        current_date + 18, current_date + 21, TRUE, 'Learning', '#8b5cf6', 'Amsterdam',
        'Three days — no discretionary trading.'),
       (gen_random_uuid(), 'Broker statement reconciliation',
        (current_date - 11 + time '09:30') AT TIME ZONE 'UTC',
        (current_date - 11 + time '10:30') AT TIME ZONE 'UTC',
        FALSE, 'Admin', '#64748b', '', 'Matched against the journal — two fees adjusted.')",
    // ── Time tracker: projects with a week of closed entries ─────────────────
    // Budgets and hourly rates are set so the breakdown shows both progress bars and valued
    // time. One archived project keeps the active/archived split visible. No open entry: a
    // timer left running would show absurd elapsed time on a sandbox reset every 15 minutes.
    "INSERT INTO time_projects (id, name, category, color, planned_end, time_budget_hours, hourly_rate, rate_currency, position) VALUES
       (gen_random_uuid(), 'Strategy research', 'Trading', '#3b82f6', current_date + 45, 120, 90, 'USD', 0),
       (gen_random_uuid(), 'Journal & review',  'Trading', '#22c55e', NULL, 40, 90, 'USD', 1),
       (gen_random_uuid(), 'Platform tinkering', 'Ops',    '#8b5cf6', current_date + 20, 60, 0, 'USD', 2),
       (gen_random_uuid(), 'Client reporting',  'Work',    '#f59e0b', current_date + 10, 25, 140, 'EUR', 3),
       (gen_random_uuid(), 'Old data migration', 'Ops',    '#64748b', NULL, NULL, NULL, 'USD', 4)",
    "UPDATE time_projects SET archived = TRUE WHERE name = 'Old data migration'",
    "INSERT INTO time_entries (id, project_id, started_at, ended_at, note)
     SELECT gen_random_uuid(), p.id,
            (current_date - v.days_ago + v.start_h) AT TIME ZONE 'UTC',
            (current_date - v.days_ago + v.start_h + make_interval(mins => v.mins)) AT TIME ZONE 'UTC',
            v.note
     FROM time_projects p, (VALUES
         ('Strategy research',  12, time '09:00', 145, 'Post-earnings drift sample'),
         ('Strategy research',  11, time '14:00',  95, 'Liquidity floor rework'),
         ('Strategy research',   9, time '10:30', 170, 'Mid-cap re-run'),
         ('Strategy research',   6, time '09:15', 120, ''),
         ('Strategy research',   4, time '15:00',  75, 'Correlation vs the breakout book'),
         ('Strategy research',   1, time '11:00', 110, ''),
         ('Journal & review',    7, time '18:00',  60, 'Weekly review'),
         ('Journal & review',    5, time '17:45',  35, ''),
         ('Journal & review',    2, time '18:15',  50, 'Tagged the ETH loss'),
         ('Platform tinkering', 10, time '20:00',  90, 'Watchlist layout'),
         ('Platform tinkering',  8, time '21:00', 130, 'Feed dashboard'),
         ('Platform tinkering',  3, time '20:30',  65, ''),
         ('Client reporting',    6, time '08:30', 100, 'Monthly pack'),
         ('Client reporting',    2, time '09:00',  80, 'Revisions')
     ) AS v(project, days_ago, start_h, mins, note)
     WHERE p.name = v.project",
    // ── Agent: OpenRouter provider with an EMPTY key (host injects OTW_DEMO_LLM_KEY),
    //    wired as the default agent's provider. The `:free` slug below is only a
    //    PREFERENCE: OpenRouter retires free tiers, and this template is replayed by
    //    every reset for weeks, so `demo::resolve_free_model` re-checks it against the
    //    live model list at boot and repins if it went paid. ─────
    "INSERT INTO agent_providers (id, kind, label, base_url, api_key, default_model, enabled)
     VALUES (gen_random_uuid(), 'openai_compat', 'OpenRouter (free models)',
             'https://openrouter.ai/api/v1', '', 'openai/gpt-oss-20b:free', TRUE)",
    "INSERT INTO agent_agents (id, name, system_prompt, is_default)
     SELECT gen_random_uuid(), 'Assistant',
            'You are the OpenTraderWorld demo assistant. Be concise. You can read the sandbox''s data through your tools.',
            TRUE
     WHERE NOT EXISTS (SELECT 1 FROM agent_agents WHERE is_default)",
    "UPDATE agent_agents
     SET provider_id = (SELECT id FROM agent_providers WHERE label = 'OpenRouter (free models)'),
         model = 'openai/gpt-oss-20b:free'
     WHERE is_default",
    // ── Mailbox: one connected mailbox (paused), a few senders and issues ────
    // The vault row exists only so the account row is well-formed; its bytes are junk
    // and the sandbox never polls (the account is disabled and /poll is denied).
    "WITH v AS (
         INSERT INTO vaults (id, name) VALUES (gen_random_uuid(), 'Demo mailbox') RETURNING id
     ), it AS (
         INSERT INTO vault_items (id, vault_id, name, nonce, ciphertext)
         SELECT gen_random_uuid(), v.id, 'app-password', '\\x00'::bytea, '\\x00'::bytea FROM v
         RETURNING id
     )
     INSERT INTO mailbox_accounts
       (id, name, email, preset, host, port, security, username, vault_item_id, enabled,
        last_success_at, uid_validity, last_uid)
     SELECT gen_random_uuid(), 'Newsletters', 'demo@example.com', 'fastmail',
            'imap.fastmail.com', 993, 'ssl', 'demo@example.com', it.id, FALSE,
            now() - interval '2 hours', 1, 42
     FROM it",
    "INSERT INTO mailbox_senders
       (id, from_addr, domain, name, description, site_url, category, status, last_subject,
        seen_count, last_seen_at)
     VALUES
       (gen_random_uuid(), 'letter@macroweekly.example', 'macroweekly.example', 'Macro Weekly',
        'Rates, liquidity and positioning, every Sunday.', 'https://macroweekly.example',
        'newsletter', 'kept', 'Liquidity is turning', 12, now() - interval '3 hours'),
       (gen_random_uuid(), 'desk@marketwire.example', 'marketwire.example', 'Market Wire',
        'Pre-open headlines.', 'https://marketwire.example',
        'news', 'kept', 'Futures firm ahead of CPI', 40, now() - interval '6 hours'),
       (gen_random_uuid(), 'statements@broker.example', 'broker.example', 'Broker statements',
        'Monthly account statements.', '', 'broker', 'kept',
        'Your July statement is ready', 4, now() - interval '2 days'),
       (gen_random_uuid(), 'hello@someshop.example', 'someshop.example', 'Some Shop',
        '', '', 'other', 'pending', 'Your order has shipped', 1, now() - interval '1 day')",
    "INSERT INTO mailbox_messages
       (id, account_id, sender_id, uid, uid_validity, message_id, subject, snippet,
        body_html, body_text, received_at, read, has_remote_images)
     SELECT gen_random_uuid(), a.id, s.id, v.uid, 1,
            'demo-' || v.uid || '@example', v.subject, v.snippet,
            '<h2>' || v.subject || '</h2><p>' || v.snippet || '</p>', v.snippet,
            now() - make_interval(hours => v.hours), v.was_read, FALSE
     FROM mailbox_accounts a
     CROSS JOIN (VALUES
        ('letter@macroweekly.example', 101, 'Liquidity is turning',
         'Reserve balances stopped falling this week — what that changes for risk assets, and the three charts to watch.', 3, FALSE),
        ('letter@macroweekly.example', 98, 'The carry trade nobody talks about',
         'Funding spreads widened quietly. A walk through who is short what, and where it breaks.', 170, TRUE),
        ('desk@marketwire.example', 100, 'Futures firm ahead of CPI',
         'Index futures up 0.4%, crude flat, the dollar softer. Consensus sees 0.2% core.', 6, FALSE),
        ('desk@marketwire.example', 96, 'Chips lead the tape',
         'Semis outperform for a third session; breadth still narrow.', 30, TRUE),
        ('statements@broker.example', 90, 'Your July statement is ready',
         'Account summary, realised PnL and fees for the month.', 48, TRUE)
     ) AS v(from_addr, uid, subject, snippet, hours, was_read)
     JOIN mailbox_senders s ON s.from_addr = v.from_addr",
    "INSERT INTO mailbox_store_links (id, name, url, domain, description, topic, subscribed, position)
     VALUES
       (gen_random_uuid(), 'Macro Weekly', 'https://macroweekly.example', 'macroweekly.example',
        'Rates, liquidity and positioning. Sunday.', 'economics', TRUE, 0),
       (gen_random_uuid(), 'Market Wire', 'https://marketwire.example', 'marketwire.example',
        'Pre-open headlines, five minutes.', 'finance', TRUE, 1),
       (gen_random_uuid(), 'The Trading Desk Diary', 'https://deskdiary.example', 'deskdiary.example',
        'One trader''s post-mortems, warts included.', 'trading', FALSE, 2),
       (gen_random_uuid(), 'Signal & Noise', 'https://signalnoise.example', 'signalnoise.example',
        'Geopolitics for people who move money.', 'geopolitics', FALSE, 3),
       (gen_random_uuid(), 'Steady Hands', 'https://steadyhands.example', 'steadyhands.example',
        'Discipline, tilt and the psychology of drawdown.', 'mindset', TRUE, 4)",
];

/// A spread of closed trades over the last two months — enough for the breakdown,
/// calendar and equity curve to look alive.
async fn trades(pool: &PgPool) -> anyhow::Result<()> {
    // (ticker, class, side, days_ago_entry, days_held, entry, exit, qty, fees, currency,
    //  strategy, signal, feedback, planned stop, tags)
    // The stop is what makes the R-multiples real; the tags are what price the mistakes.
    let trades: &[(&str, &str, &str, i32, i32, f64, f64, f64, f64, &str, &str, &str, &str, f64, &str)] = &[
        ("AAPL", "stock", "long", 55, 3, 227.40, 234.10, 20.0, 1.5, "USD", "Breakout", "Range break", "Clean setup, took profit at resistance.", 224.00, "Followed the plan"),
        ("NVDA", "stock", "long", 48, 5, 168.20, 176.90, 15.0, 1.5, "USD", "Breakout", "Volume surge", "Strong momentum; exited a bit early.", 164.50, "Exited early"),
        ("TSLA", "stock", "short", 41, 2, 322.50, 314.80, 10.0, 1.5, "USD", "Mean reversion", "RSI extreme", "Faded the gap-up; worked as planned.", 328.00, "Followed the plan"),
        ("MSFT", "stock", "long", 34, 6, 502.10, 497.30, 8.0, 1.5, "USD", "Breakout", "Range break", "False break, cut it at the stop.", 497.00, "Followed the plan"),
        ("BTC-USD", "crypto", "long", 28, 4, 104200.0, 109800.0, 0.15, 12.0, "USD", "Breakout", "Volume surge", "Held through the chop, good exit.", 101500.0, "Followed the plan,Breakout continuation"),
        ("ETH-USD", "crypto", "long", 21, 3, 3320.0, 3145.0, 2.0, 8.0, "USD", "Mean reversion", "VWAP fade", "Fought the trend, lesson logged.", 3250.0, "Moved my stop"),
        ("AIR.PA", "stock", "long", 14, 7, 172.60, 181.20, 25.0, 4.0, "EUR", "Breakout", "Range break", "Patience paid; textbook continuation.", 168.90, "Followed the plan,Breakout continuation"),
        ("SPY", "etf", "short", 7, 1, 623.40, 619.90, 12.0, 1.5, "USD", "Mean reversion", "RSI extreme", "Quick scalp into the close.", 626.00, "Overtraded"),
    ];
    for t in trades {
        sqlx::query(
            "INSERT INTO journal_trades
               (id, category_id, strategy_id, ticker, asset_class, side,
                entry_at, exit_at, entry_price, exit_price, quantity, fees, currency,
                signal_name, feedback, stop_price)
             SELECT gen_random_uuid(),
                    CASE WHEN $2 = 'crypto' THEN (SELECT id FROM journal_categories WHERE name = 'Crypto')
                         ELSE (SELECT id FROM journal_categories WHERE is_default) END,
                    (SELECT id FROM journal_strategies WHERE name = $10),
                    $1, $2, $3,
                    now() - make_interval(days => $4),
                    now() - make_interval(days => $4 - $5),
                    $6, $7, $8, $9, $11, $12, $13, $14",
        )
        .bind(t.0)  // $1 ticker
        .bind(t.1)  // $2 asset_class
        .bind(t.2)  // $3 side
        .bind(t.3)  // $4 days_ago_entry
        .bind(t.4)  // $5 days_held
        .bind(t.5)  // $6 entry_price
        .bind(t.6)  // $7 exit_price
        .bind(t.7)  // $8 quantity
        .bind(t.8)  // $9 fees
        .bind(t.10) // $10 strategy name
        .bind(t.9)  // $11 currency
        .bind(t.11) // $12 signal_name
        .bind(t.12) // $13 feedback
        .bind(t.13) // $14 stop_price
        .execute(pool)
        .await?;
        if !t.14.is_empty() {
            // Tickers are unique inside the seed, so matching on one is enough to
            // attach the trade's tags without threading the generated id back out.
            sqlx::query(
                "INSERT INTO journal_trade_tags (trade_id, tag_id)
                 SELECT tr.id, tg.id FROM journal_trades tr
                 JOIN journal_tags tg ON tg.name = ANY(string_to_array($2, ','))
                 WHERE tr.ticker = $1
                 ON CONFLICT DO NOTHING",
            )
            .bind(t.0)
            .bind(t.14)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// A funded portfolio with a buy/sell ledger and ~3 months of daily valuations, so the
/// tracker's value chart and cost-basis figures have something to draw. `auto_refresh`
/// stays FALSE: the daily job must not spend the shared demo host's egress on its own
/// (a visitor can still hit Refresh, which prices against live CoinGecko/Yahoo).
async fn portfolio(pool: &PgPool) -> anyhow::Result<()> {
    let pf: Uuid = sqlx::query_scalar(
        "INSERT INTO portfolios (id, name, description, currency, auto_refresh, position)
         VALUES (gen_random_uuid(), 'Long-term core',
                 'Buy-and-hold sleeve — demo data, hit Refresh for live prices', 'USD', FALSE, 0)
         RETURNING id",
    )
    .fetch_one(pool)
    .await?;

    // (class, provider, provider_id, symbol, name, [(side, days_ago, qty, price)])
    type Op = (&'static str, i32, f64, f64);
    let assets: &[(&str, &str, &str, &str, &str, &[Op])] = &[
        ("crypto", "coingecko", "bitcoin", "BTC", "Bitcoin",
         &[("buy", 88, 0.25, 96500.0), ("buy", 40, 0.10, 103200.0)]),
        ("crypto", "coingecko", "ethereum", "ETH", "Ethereum",
         &[("buy", 75, 3.0, 3050.0), ("sell", 20, 1.0, 3480.0)]),
        ("stock", "yahoo", "AAPL", "AAPL", "Apple",
         &[("buy", 82, 40.0, 221.30)]),
        ("etf", "yahoo", "VWCE.DE", "VWCE", "Vanguard FTSE All-World",
         &[("buy", 82, 60.0, 128.40), ("buy", 51, 25.0, 133.10)]),
    ];
    for (class, provider, pid, symbol, name, ops) in assets {
        let asset: Uuid = sqlx::query_scalar(
            "INSERT INTO portfolio_assets
                 (id, portfolio_id, asset_class, provider, provider_id, symbol, name)
             VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(pf).bind(class).bind(provider).bind(pid).bind(symbol).bind(name)
        .fetch_one(pool)
        .await?;
        for (side, days_ago, qty, price) in *ops {
            sqlx::query(
                "INSERT INTO portfolio_operations (id, asset_id, side, op_date, quantity, price, fee)
                 VALUES (gen_random_uuid(), $1, $2, current_date - $3, $4, $5, 0.9)",
            )
            .bind(asset).bind(side).bind(*days_ago).bind(*qty).bind(*price)
            .execute(pool)
            .await?;
        }
    }

    // Seed a first spot per asset so the tracker opens with a live-looking market value and
    // unrealized PnL instead of blanks. Same figures as the watchlist's seeded quotes where
    // the symbols overlap, so the two modules don't contradict each other on the same screen.
    // `refreshed_at` is set to match: the UI dates the prices from it.
    for (pid, price) in [
        ("bitcoin", 108_400.0),
        ("ethereum", 3_285.0),
        ("AAPL", 236.80),
        ("VWCE.DE", 141.75),
    ] {
        sqlx::query(
            "UPDATE portfolio_assets SET last_price_usd = $1, last_price_at = now(),
                    recon_status = 'ok', recon_checked_at = now()
             WHERE portfolio_id = $2 AND provider_id = $3",
        )
        .bind(price)
        .bind(pf)
        .bind(pid)
        .execute(pool)
        .await?;
    }
    sqlx::query("UPDATE portfolios SET refreshed_at = now() WHERE id = $1")
        .bind(pf)
        .execute(pool)
        .await?;

    // Daily snapshots: a gently drifting curve (deterministic, no RNG) so the chart shows
    // a plausible shape instead of a flat line. Cost basis is flat after the last buy.
    sqlx::query(
        "INSERT INTO portfolio_snapshots (portfolio_id, snap_date, currency, market_value, cost_basis)
         SELECT $1, d::date, 'USD',
                58000 + 5200 * sin(extract(epoch FROM d) / 950000.0)
                      + 90 * (current_date - d::date),
                54900
         FROM generate_series(current_date - 89, current_date, interval '1 day') AS d",
    )
    .bind(pf)
    .execute(pool)
    .await?;
    Ok(())
}

/// Prime each watchlist item's cached quote so the module opens on populated cards
/// (price, 24h/3d/7d/30d changes, sparkline) instead of four dashes.
///
/// The sandbox never polls providers on its own, so these are *seeded* quotes, not fetched
/// ones — a deterministic 31-day series per symbol, no RNG and no network. The JSON matches
/// `watchlists::quotes::compute_quote` field for field, so a visitor who hits Refresh simply
/// overwrites it with the real thing.
///
/// `history_at` is backdated past `HISTORY_TTL` on purpose: a refresh then refetches the real
/// series rather than deriving changes from this synthetic one.
async fn watchlist_quotes(pool: &PgPool) -> anyhow::Result<()> {
    use time::OffsetDateTime;

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let day = 86_400i64;
    // Anchor the series on the last completed UTC day: `compute_quote` drops the current
    // day's partial close, and the seeded data must land the same way.
    let last_close = (now / day - 1) * day;

    // (provider_id, spot USD, drift %/day, wobble amplitude %)
    let series: &[(&str, f64, f64, f64)] = &[
        ("bitcoin", 108_400.0, 0.18, 2.6),
        ("ethereum", 3_285.0, -0.09, 3.4),
        ("AAPL", 236.80, 0.11, 1.2),
        ("SPY", 627.40, 0.06, 0.7),
    ];

    for (pid, spot, drift, wobble) in series {
        // Walk backwards from the spot: close(t) = spot / (1+drift)^n, plus a smooth
        // wobble so the sparkline has shape. 30 closes + the live spot = the 31-point
        // window `compute_quote` caps `spark` at.
        let mut history: Vec<serde_json::Value> = Vec::with_capacity(30);
        for n in (1..=30).rev() {
            let t = last_close - (n as i64 - 1) * day;
            let base = spot / (1.0 + drift / 100.0).powi(n as i32);
            let close = base * (1.0 + wobble / 100.0 * ((n as f64) * 0.7).sin());
            history.push(serde_json::json!([t, (close * 1e6).round() / 1e6]));
        }
        // Changes are computed against the seeded closes, so the card's percentages agree
        // with its own sparkline.
        let close_at = |days: i64| -> Option<f64> {
            let cutoff = now - days * day;
            history
                .iter()
                .rev()
                .find(|p| p[0].as_i64().is_some_and(|t| t <= cutoff))
                .and_then(|p| p[1].as_f64())
        };
        let pct = |r: f64| (((spot - r) / r * 100.0) * 1e4).round() / 1e4;
        let spark: Vec<f64> = history
            .iter()
            .filter_map(|p| p[1].as_f64())
            .chain(std::iter::once(*spot))
            .collect();

        let quote = serde_json::json!({
            "price_usd": spot,
            "change_24h": close_at(1).map(pct),
            "change_3d": close_at(3).map(pct),
            "change_7d": close_at(7).map(pct),
            "change_30d": close_at(30).map(pct),
            "spark": spark,
            "history": history,
            // Older than HISTORY_TTL → the first real refresh refetches the true series.
            "history_at": now - 7 * day,
        });
        sqlx::query(
            "UPDATE watchlist_items SET quote = $1, quoted_at = now() WHERE provider_id = $2",
        )
        .bind(&quote)
        .bind(pid)
        .execute(pool)
        .await?;
    }
    // The list header reads this to label how fresh the cards are.
    sqlx::query("UPDATE watchlists SET refreshed_at = now()").execute(pool).await?;
    Ok(())
}

/// Editor content: a folder with two research pages, so the editor opens on something
/// real (tree + rich text) instead of an empty state. TipTap/ProseMirror JSON.
async fn documents(pool: &PgPool) -> anyhow::Result<()> {
    let folder: Uuid = sqlx::query_scalar(
        "INSERT INTO documents (id, parent_id, kind, title, position, icon)
         VALUES (gen_random_uuid(), NULL, 'folder', 'Research', 0, '📁') RETURNING id",
    )
    .fetch_one(pool)
    .await?;

    let inefficiency = serde_json::json!({
        "type": "doc",
        "content": [
            { "type": "heading", "attrs": { "level": 1 },
              "content": [{ "type": "text", "text": "Post-earnings drift — does it still pay?" }] },
            { "type": "paragraph", "content": [{ "type": "text",
              "text": "Working note on whether post-earnings announcement drift survives in large caps. Numbers below are illustrative demo data, not a real study." }] },
            { "type": "heading", "attrs": { "level": 2 },
              "content": [{ "type": "text", "text": "Hypothesis" }] },
            { "type": "paragraph", "content": [{ "type": "text",
              "text": "Stocks that beat consensus by more than one standard deviation keep drifting up for roughly 20 sessions, because analyst revisions lag the print." }] },
            { "type": "heading", "attrs": { "level": 2 },
              "content": [{ "type": "text", "text": "What the sample showed" }] },
            { "type": "bulletList", "content": [
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Mean 20-day excess return after a large beat: +1.8% (n=214)." }] }] },
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Most of the edge lands in the first 5 sessions; the tail is noise." }] }] },
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Effect roughly halves once you subtract a 12 bps round-trip cost." }] }] },
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Nothing left in mega caps — coverage is too dense for the lag to persist." }] }] }
            ]},
            { "type": "heading", "attrs": { "level": 2 },
              "content": [{ "type": "text", "text": "Next step" }] },
            { "type": "paragraph", "content": [{ "type": "text",
              "text": "Re-run restricted to mid caps with a liquidity floor, then size it against the breakout book to check the correlation is low enough to be worth a sleeve." }] }
        ]
    });

    let playbook = serde_json::json!({
        "type": "doc",
        "content": [
            { "type": "heading", "attrs": { "level": 1 },
              "content": [{ "type": "text", "text": "Breakout playbook" }] },
            { "type": "paragraph", "content": [{ "type": "text",
              "text": "The rules the journal's Breakout strategy is graded against." }] },
            { "type": "orderedList", "content": [
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Range must be at least 10 sessions wide; ignore anything tighter." }] }] },
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Breakout bar needs volume above 1.5× the 20-day average." }] }] },
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Stop goes under the last higher low, never at a round number." }] }] },
                { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text",
                  "text": "Take half off at 2R, trail the rest behind the 10-day low." }] }] }
            ]},
            { "type": "blockquote", "content": [{ "type": "paragraph", "content": [{ "type": "text",
              "text": "Recurring mistake: entering before the close confirms the break. Two of the losing trades in the journal are exactly this." }] }] }
        ]
    });

    for (pos, title, icon, content) in [
        (0.0, "Post-earnings drift — does it still pay?", "🔬", inefficiency),
        (1.0, "Breakout playbook", "📈", playbook),
    ] {
        sqlx::query(
            "INSERT INTO documents (id, parent_id, kind, title, content, position, icon)
             VALUES (gen_random_uuid(), $1, 'page', $2, $3, $4, $5)",
        )
        .bind(folder).bind(title).bind(content).bind(pos).bind(icon)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seeded_workflow_is_one_the_module_would_accept() {
        // The seed writes this graph straight into `automator_workflows.graph`, which the
        // engine runs and the editor re-saves: a block config that drifts out of the schema
        // must fail here, not at 3 a.m. on the demo host.
        let graph = crate::automator::Graph::parse(&briefing_graph()).expect("parses");
        crate::automator::validate(&graph).expect("validates");
    }

    #[test]
    fn every_chart_instrument_carries_usable_studies() {
        for (provider, asset_type, ticker, timeframe, name, studies) in CHART_INSTRUMENTS {
            let parsed: serde_json::Value =
                serde_json::from_str(studies).unwrap_or_else(|e| panic!("{ticker}: {e}"));
            let list = parsed.as_array().expect("an instances array");
            assert!(!list.is_empty(), "{ticker} has no studies");
            for i in list {
                assert!(i["type"].is_string(), "{ticker}: a study needs a type");
                assert!(i["params"].is_object(), "{ticker}: a study needs params");
            }
            for field in [provider, asset_type, ticker, timeframe, name] {
                assert!(!field.trim().is_empty());
            }
        }
    }

    #[test]
    fn a_layout_key_matches_what_the_chart_files_it_under() {
        // Provider/asset/timeframe lowercased, the ticker verbatim.
        assert_eq!(
            coord_key("Binance", "Crypto", "BTCUSDT", "1H"),
            "binance|crypto|BTCUSDT|1h"
        );
    }

    #[test]
    fn levels_are_rounded_the_way_a_hand_would_draw_them() {
        assert_eq!(round_level(108_437.21), 108_000.0);
        assert_eq!(round_level(236.83), 237.0);
        assert_eq!(round_level(3.2874), 3.29);
        assert_eq!(round_level(0.0), 0.0);
    }
}
