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
    managers(pool).await?;
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
        otw_store::mcp::create_token(pool, "Demo agent (internal, read-only)", &perms).await?;
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
];

/// A spread of closed trades over the last two months — enough for the breakdown,
/// calendar and equity curve to look alive.
async fn trades(pool: &PgPool) -> anyhow::Result<()> {
    // (ticker, class, side, days_ago_entry, days_held, entry, exit, qty, fees, currency, strategy, signal, feedback)
    let trades: &[(&str, &str, &str, i32, i32, f64, f64, f64, f64, &str, &str, &str, &str)] = &[
        ("AAPL", "stock", "long", 55, 3, 227.40, 234.10, 20.0, 1.5, "USD", "Breakout", "Range break", "Clean setup, took profit at resistance."),
        ("NVDA", "stock", "long", 48, 5, 168.20, 176.90, 15.0, 1.5, "USD", "Breakout", "Volume surge", "Strong momentum; exited a bit early."),
        ("TSLA", "stock", "short", 41, 2, 322.50, 314.80, 10.0, 1.5, "USD", "Mean reversion", "RSI extreme", "Faded the gap-up; worked as planned."),
        ("MSFT", "stock", "long", 34, 6, 502.10, 497.30, 8.0, 1.5, "USD", "Breakout", "Range break", "False break — cut it at the stop."),
        ("BTC-USD", "crypto", "long", 28, 4, 104200.0, 109800.0, 0.15, 12.0, "USD", "Breakout", "Volume surge", "Held through the chop, good exit."),
        ("ETH-USD", "crypto", "long", 21, 3, 3320.0, 3145.0, 2.0, 8.0, "USD", "Mean reversion", "VWAP fade", "Fought the trend — lesson logged."),
        ("AIR.PA", "stock", "long", 14, 7, 172.60, 181.20, 25.0, 4.0, "EUR", "Breakout", "Range break", "Patience paid; textbook continuation."),
        ("SPY", "etf", "short", 7, 1, 623.40, 619.90, 12.0, 1.5, "USD", "Mean reversion", "RSI extreme", "Quick scalp into the close."),
    ];
    for t in trades {
        sqlx::query(
            "INSERT INTO journal_trades
               (id, category_id, strategy_id, ticker, asset_class, side,
                entry_at, exit_at, entry_price, exit_price, quantity, fees, currency,
                signal_name, feedback)
             SELECT gen_random_uuid(),
                    CASE WHEN $2 = 'crypto' THEN (SELECT id FROM journal_categories WHERE name = 'Crypto')
                         ELSE (SELECT id FROM journal_categories WHERE is_default) END,
                    (SELECT id FROM journal_strategies WHERE name = $10),
                    $1, $2, $3,
                    now() - make_interval(days => $4),
                    now() - make_interval(days => $4 - $5),
                    $6, $7, $8, $9, $11, $12, $13",
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
        .execute(pool)
        .await?;
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
