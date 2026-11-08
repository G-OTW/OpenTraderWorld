//! Demo fixtures for Subscriptions, the Journal's longer history, the Scanner and Options,
//! loaded by `--seed-demo` (see [`crate::demo_seed`]) after the datasets landed.
//!
//! - Subscriptions: price history, usage ratings, a free trial, a cancelled service, a shared
//!   plan, payment cards, reminders and category budgets, so every panel has a reading.
//! - Journal: four more months of closed trades before the ones `demo_seed` writes, and a few
//!   advanced (scaled in and out, bracketed) positions, saved through the API's own path.
//! - Scanner: a universe over the stored datasets only (the demo scans nothing it would have
//!   to download), hives, signals, alerts, two boards. The board cache and the past hits are
//!   computed from the stored bars with the scanner's own evaluator, never written down.
//! - Options: saved strategies whose strikes and premiums come from the last stored close,
//!   priced with the builder's own model. An underlying without bars is left out.
//!
//! Everything here ships in the public repo: no secrets. Dates are relative to the seed run.

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use time::{Date, Duration, Month, OffsetDateTime, Weekday};
use uuid::Uuid;

use otw_store::histdata::{self as hist_store, Bar};
use otw_store::journal::{self, TradeInput};
use otw_store::scanner::{self as scan_store, AlertInput, ItemInput, ViewInput};

use crate::options::pricing::{Model, Style};
use crate::scanner::eval::{self, Compiled, Library};
use crate::scanner::Ohlcv;

pub async fn seed(pool: &PgPool) -> Result<()> {
    subscriptions(pool).await?;
    journal_history(pool).await?;
    advanced_trades(pool).await?;
    scanner(pool).await?;
    options(pool).await?;
    println!("subscriptions, journal, scanner, options: demo data seeded");
    Ok(())
}

// ── Subscriptions ────────────────────────────────────────────────────────────

/// One statement per query (sqlx prepared statements are single-statement).
const SUBSCRIPTIONS: &[&str] = &[
    // A few more services: a weekly and a quarterly cadence, a plan split with a desk
    // partner, and a free trial that turns into a bill in four days.
    "INSERT INTO subscriptions (id, name, platform, url, price, currency, frequency, category, started_on) VALUES
       (gen_random_uuid(), 'Benzinga Pro', 'Benzinga', 'https://benzinga.com',
        147.00, 'USD', 'monthly', 'News', current_date - 75),
       (gen_random_uuid(), 'Barron''s Digital', 'Barron''s', NULL,
        4.99, 'USD', 'weekly', 'News', current_date - 120),
       (gen_random_uuid(), 'Quiver Quant', 'Quiver', NULL,
        75.00, 'USD', 'quarterly', 'Alternative data', current_date - 200),
       (gen_random_uuid(), 'Finviz Elite', 'Finviz', 'https://finviz.com',
        39.50, 'USD', 'monthly', 'Research', current_date - 10)",
    "UPDATE subscriptions SET trial_end = current_date + 4, remind_days = 2 WHERE name = 'Finviz Elite'",
    "UPDATE subscriptions SET my_share_pct = 50, payment_method = 'Amex 1009',
            card_expiry = to_char(current_date + 30, 'YYYY-MM')
     WHERE name = 'Benzinga Pro'",
    // The cancelled tool keeps its history up to the day it ended.
    "UPDATE subscriptions SET ended_at = current_date - 75 WHERE name = 'Old backtesting tool'",
    // Payment methods, reminders and the yearly price a monthly plan could switch to.
    "UPDATE subscriptions SET payment_method = 'Visa 4242', card_expiry = to_char(current_date + 30, 'YYYY-MM'),
            remind_days = 3, yearly_price = 599.40
     WHERE name = 'TradingView Premium'",
    "UPDATE subscriptions SET payment_method = 'IBKR account' WHERE name = 'Interactive Brokers market data'",
    "UPDATE subscriptions SET payment_method = 'Visa 4242', card_expiry = to_char(current_date + 30, 'YYYY-MM'),
            remind_days = 7
     WHERE name = 'Koyfin Plus'",
    "UPDATE subscriptions SET payment_method = 'Mastercard 5100', card_expiry = '2028-09', yearly_price = 390.00
     WHERE name = 'Financial Times'",
    "UPDATE subscriptions SET payment_method = 'SEPA direct debit', remind_days = 2 WHERE name = 'VPS — strategy runner'",
    "UPDATE subscriptions SET payment_method = 'Visa 4242', card_expiry = to_char(current_date + 30, 'YYYY-MM'),
            yearly_price = 96.00
     WHERE name = 'Notion'",
    "UPDATE subscriptions SET payment_method = 'PayPal' WHERE name IN ('Substack — macro letter', 'Barron''s Digital', 'Quiver Quant')",
    // Price history: four services were repriced. The rest start with the price they hold.
    "INSERT INTO subscription_prices (subscription_id, price, currency, effective_from)
     SELECT s.id, p.price, s.currency, s.started_on + p.after_days
     FROM subscriptions s
     JOIN (VALUES ('TradingView Premium', 49.95, 0), ('TradingView Premium', 59.95, 240),
                  ('Koyfin Plus', 396.00, 0), ('Koyfin Plus', 468.00, 190),
                  ('Notion', 8.00, 0), ('Notion', 10.00, 260),
                  ('Financial Times', 35.00, 0), ('Financial Times', 39.00, 90))
       AS p(name, price, after_days) ON p.name = s.name",
    "INSERT INTO subscription_prices (subscription_id, price, currency, effective_from)
     SELECT s.id, s.price, s.currency, COALESCE(s.started_on, current_date)
     FROM subscriptions s
     WHERE NOT EXISTS (SELECT 1 FROM subscription_prices p WHERE p.subscription_id = s.id)",
    // Quarterly usage ratings: the charting tool earns its price, the research stack is
    // sliding, the newsletter is the one to cut.
    "INSERT INTO subscription_ratings (subscription_id, rated_at, score)
     SELECT s.id, current_date - r.days_ago, r.score
     FROM subscriptions s
     JOIN (VALUES ('TradingView Premium', 185, 5), ('TradingView Premium', 95, 5), ('TradingView Premium', 6, 5),
                  ('Interactive Brokers market data', 95, 4), ('Interactive Brokers market data', 6, 4),
                  ('Koyfin Plus', 185, 4), ('Koyfin Plus', 95, 3), ('Koyfin Plus', 6, 2),
                  ('Financial Times', 95, 3), ('Financial Times', 6, 2),
                  ('VPS — strategy runner', 95, 5), ('VPS — strategy runner', 6, 5),
                  ('Notion', 185, 3), ('Notion', 6, 3),
                  ('Substack — macro letter', 60, 2), ('Substack — macro letter', 6, 1),
                  ('Benzinga Pro', 6, 4),
                  ('Barron''s Digital', 30, 3),
                  ('Quiver Quant', 95, 4), ('Quiver Quant', 6, 3),
                  ('Old backtesting tool', 200, 2))
       AS r(name, days_ago, score) ON r.name = s.name",
    // Monthly caps per category: Research is over, the rest have room.
    "UPDATE subscription_settings SET category_budgets =
       '{\"Research\": 80, \"News\": 150, \"Charting\": 70, \"Market data\": 25, \"Infrastructure\": 25}'::jsonb
     WHERE id = TRUE",
];

async fn subscriptions(pool: &PgPool) -> Result<()> {
    for sql in SUBSCRIPTIONS {
        sqlx::query(*sql).execute(pool).await.with_context(|| format!("subscriptions: {sql}"))?;
    }
    Ok(())
}

// ── Journal ──────────────────────────────────────────────────────────────────

/// Four more months of closed trades, before the two `demo_seed` writes: the calendar, the
/// monthly breakdown and the equity curve then span half a year, with one bad month in it.
async fn journal_history(pool: &PgPool) -> Result<()> {
    // The account opens before the first trade.
    sqlx::query("UPDATE journal_capital_events SET occurred_at = now() - interval '200 days' WHERE kind = 'initial'")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO journal_capital_events (id, category_id, kind, amount, note, occurred_at)
         SELECT gen_random_uuid(), id, 'refill', 2500, 'Quarterly top-up', now() - interval '105 days'
         FROM journal_categories WHERE is_default",
    )
    .execute(pool)
    .await?;

    // (ticker, class, side, days_ago_entry, days_held, entry, exit, qty, fees, currency,
    //  strategy, signal, feedback, planned stop, tags)
    let trades: &[(&str, &str, &str, i32, i32, f64, f64, f64, f64, &str, &str, &str, &str, f64, &str)] = &[
        ("MSFT", "stock", "long", 182, 6, 468.20, 481.90, 10.0, 1.5, "USD", "Breakout", "Range break", "First trade of the half: small size, clean break.", 461.00, "Followed the plan"),
        ("BTC-USD", "crypto", "long", 176, 5, 96_400.0, 101_200.0, 0.12, 12.0, "USD", "Breakout", "Volume surge", "Rode the weekend squeeze, trailed the stop under each higher low.", 93_800.0, "Followed the plan,Breakout continuation"),
        ("TSLA", "stock", "long", 170, 2, 341.60, 329.40, 12.0, 1.5, "USD", "Breakout", "Range break", "Chased the open; the break never held.", 333.00, "Moved my stop"),
        ("SAP.DE", "stock", "long", 165, 9, 241.80, 252.30, 30.0, 4.0, "EUR", "Breakout", "Range break", "Patient hold through earnings drift.", 236.50, "Followed the plan"),
        ("EURUSD", "forex", "long", 160, 3, 1.0712, 1.0786, 50_000.0, 4.0, "USD", "Mean reversion", "RSI extreme", "Bought the washout after the ECB minutes.", 1.0668, "Followed the plan"),
        ("AAPL", "stock", "short", 154, 2, 238.10, 233.40, 15.0, 1.5, "USD", "Mean reversion", "RSI extreme", "Faded the event pop, covered at the gap fill.", 241.50, "Followed the plan"),
        ("ETH-USD", "crypto", "long", 149, 4, 3_480.0, 3_655.0, 1.5, 8.0, "USD", "Breakout", "Volume surge", "Followed BTC strength, waited for the retest.", 3_390.0, "Followed the plan,Breakout continuation"),
        ("NVDA", "stock", "long", 143, 3, 158.40, 154.10, 25.0, 1.5, "USD", "Mean reversion", "VWAP fade", "Bought a dip that kept dipping; out at the stop.", 154.20, "Followed the plan"),
        ("SPY", "etf", "long", 138, 1, 602.30, 606.80, 15.0, 1.5, "USD", "Mean reversion", "VWAP fade", "Morning flush into VWAP reclaim.", 599.50, "Followed the plan"),
        // The bad month: revenge trading after a stop-out, then a forced break.
        ("META", "stock", "long", 131, 1, 712.40, 698.60, 8.0, 1.5, "USD", "Breakout", "Range break", "Bought the high of the day, no confirmation.", 704.00, "Moved my stop"),
        ("META", "stock", "short", 130, 1, 699.10, 708.80, 8.0, 1.5, "USD", "Mean reversion", "RSI extreme", "Flipped short out of frustration. Pure revenge.", 705.00, "Overtraded,Moved my stop"),
        ("AMD", "stock", "long", 129, 1, 166.30, 161.90, 30.0, 1.5, "USD", "Breakout", "Volume surge", "Third trade of the day with no setup.", 163.00, "Overtraded"),
        ("SOL-USD", "crypto", "long", 127, 2, 189.40, 178.20, 10.0, 6.0, "USD", "Breakout", "Volume surge", "Size too big after two losers. Took a day off after this.", 183.00, "Overtraded,Moved my stop"),
        ("QQQ", "etf", "short", 120, 2, 548.90, 551.30, 10.0, 1.5, "USD", "Mean reversion", "RSI extreme", "Back from the break, half size; small scratch.", 553.00, "Followed the plan"),
        ("AIR.PA", "stock", "long", 114, 6, 161.40, 169.80, 25.0, 4.0, "EUR", "Breakout", "Range break", "Weekly range break, sector tailwind from the order book.", 157.80, "Followed the plan,Breakout continuation"),
        ("GOOGL", "stock", "long", 108, 4, 176.30, 182.60, 25.0, 1.5, "USD", "Breakout", "Volume surge", "Volume came in on the second push, held to target.", 173.10, "Followed the plan"),
        ("GBPUSD", "forex", "short", 101, 2, 1.2788, 1.2731, 40_000.0, 3.5, "USD", "Mean reversion", "RSI extreme", "Faded the London high; target at prior close.", 1.2825, "Followed the plan"),
        ("AMZN", "stock", "long", 96, 3, 204.70, 202.10, 20.0, 1.5, "USD", "Breakout", "Range break", "Took profit too soon, then it ran another 4%.", 200.90, "Exited early"),
        ("BTC-USD", "crypto", "short", 90, 2, 108_900.0, 105_300.0, 0.08, 12.0, "USD", "Mean reversion", "RSI extreme", "Funding was hot; faded the spike into the weekly level.", 111_200.0, "Followed the plan"),
        ("ASML.AS", "stock", "long", 84, 5, 872.40, 903.60, 8.0, 3.0, "EUR", "Breakout", "Volume surge", "Semis led the tape, clean continuation.", 858.00, "Followed the plan,Breakout continuation"),
        ("NFLX", "stock", "short", 78, 1, 1_212.40, 1_196.80, 5.0, 1.5, "USD", "Mean reversion", "VWAP fade", "Sold the overextended gap, covered at VWAP.", 1_226.00, "Followed the plan"),
        ("XAUUSD", "forex", "long", 72, 4, 3_318.0, 3_362.0, 5.0, 4.0, "USD", "Breakout", "Range break", "Gold broke the month range on the dollar slide.", 3_296.0, "Followed the plan"),
    ];
    for t in trades {
        let (id,): (Uuid,) = sqlx::query_as(
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
                    $6, $7, $8, $9, $11, $12, $13, $14
             RETURNING id",
        )
        .bind(t.0)
        .bind(t.1)
        .bind(t.2)
        .bind(t.3)
        .bind(t.4)
        .bind(t.5)
        .bind(t.6)
        .bind(t.7)
        .bind(t.8)
        .bind(t.10)
        .bind(t.9)
        .bind(t.11)
        .bind(t.12)
        .bind(t.13)
        .fetch_one(pool)
        .await
        .with_context(|| format!("journal trade {}", t.0))?;
        if !t.14.is_empty() {
            sqlx::query(
                "INSERT INTO journal_trade_tags (trade_id, tag_id)
                 SELECT $1, id FROM journal_tags WHERE name = ANY(string_to_array($2, ','))
                 ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(t.14)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// Positions built in several fills: a pyramided long, a short scaled out by a take-profit
/// bracket, and one still open with a partial exit. Saved like the trade form saves them,
/// so the triggered bracket folds into an exit leg exactly as it would on the API.
async fn advanced_trades(pool: &PgPool) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    let ago = |d: i64, h: i64| rfc3339(now - Duration::days(d) + Duration::hours(h));
    let default_cat: (Uuid,) = sqlx::query_as("SELECT id FROM journal_categories WHERE is_default").fetch_one(pool).await?;
    let crypto_cat: (Uuid,) = sqlx::query_as("SELECT id FROM journal_categories WHERE name = 'Crypto'").fetch_one(pool).await?;
    let strategy = |name: &'static str| async move {
        sqlx::query_as::<_, (Uuid,)>("SELECT id FROM journal_strategies WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map(|r| r.map(|r| r.0))
    };
    let tags = |names: &'static [&'static str]| async move {
        sqlx::query_as::<_, (Uuid,)>("SELECT id FROM journal_tags WHERE name = ANY($1)")
            .bind(names)
            .fetch_all(pool)
            .await
            .map(|v| v.into_iter().map(|r| r.0).collect::<Vec<_>>())
    };

    let rows = vec![
        json!({
            "category_id": default_cat.0,
            "strategy_id": strategy("Breakout").await?,
            "ticker": "NVDA", "asset_class": "stock", "side": "long", "currency": "USD",
            "entry_at": ago(66, 0), "exit_at": ago(58, 0),
            "signal_name": "Range break",
            "feedback": "Added on the retest, scaled out into strength. Second half ran to the measured move.",
            "advanced": true, "cost_basis_method": "fifo",
            "entries": [
                { "id": "e1", "price": 171.20, "qty": 20, "at": ago(66, 0), "fees": 1.5, "signal": "Range break" },
                { "id": "e2", "price": 174.60, "qty": 15, "at": ago(64, 0), "fees": 1.5, "signal": "Volume surge" }
            ],
            "exits": [
                { "id": "x1", "price": 181.30, "qty": 20, "at": ago(61, 0), "fees": 1.5 },
                { "id": "x2", "price": 186.90, "qty": 15, "at": ago(58, 0), "fees": 1.5 }
            ],
            "brackets": [
                { "id": "b1", "kind": "sl", "price": 166.80, "triggered": false, "note": "Under the range" }
            ],
            "tags": tags(&["Followed the plan", "Breakout continuation"]).await?,
        }),
        json!({
            "category_id": crypto_cat.0,
            "strategy_id": strategy("Mean reversion").await?,
            "ticker": "BTC-USD", "asset_class": "crypto", "side": "short", "currency": "USD",
            "entry_at": ago(46, 0), "exit_at": ago(43, 0),
            "signal_name": "RSI extreme",
            "feedback": "Half off at the first target by bracket, the rest covered by hand at the weekly level.",
            "advanced": true, "cost_basis_method": "avg",
            "entries": [
                { "id": "e1", "price": 112_400.0, "qty": 0.2, "at": ago(46, 0), "fees": 12.0, "signal": "RSI extreme" }
            ],
            "exits": [
                { "id": "x1", "price": 107_900.0, "qty": 0.1, "at": ago(43, 0), "fees": 12.0 }
            ],
            "brackets": [
                { "id": "b1", "kind": "tp", "price": 109_500.0, "qty": 0.1, "at": ago(45, 0), "triggered": true },
                { "id": "b2", "kind": "sl", "price": 115_600.0, "triggered": false }
            ],
            "tags": tags(&["Followed the plan"]).await?,
        }),
        // Still open: two entries, a third taken off, the stop under the second entry.
        json!({
            "category_id": default_cat.0,
            "strategy_id": strategy("Breakout").await?,
            "ticker": "MSFT", "asset_class": "stock", "side": "long", "currency": "USD",
            "entry_at": ago(9, 0),
            "signal_name": "Range break",
            "advanced": true, "cost_basis_method": "avg",
            "entries": [
                { "id": "e1", "price": 506.40, "qty": 6, "at": ago(9, 0), "fees": 1.5, "signal": "Range break" },
                { "id": "e2", "price": 512.10, "qty": 6, "at": ago(7, 0), "fees": 1.5, "signal": "Volume surge" }
            ],
            "exits": [
                { "id": "x1", "price": 521.80, "qty": 4, "at": ago(3, 0), "fees": 1.5 }
            ],
            "brackets": [
                { "id": "b1", "kind": "sl", "price": 503.00, "triggered": false },
                { "id": "b2", "kind": "tp", "price": 538.00, "qty": 8, "triggered": false }
            ],
        }),
    ];
    for row in rows {
        let mut input: TradeInput = serde_json::from_value(row).context("demo advanced trade")?;
        input.normalize(None);
        journal::add_trade(pool, &input).await?;
    }
    Ok(())
}

// ── Scanner ──────────────────────────────────────────────────────────────────

/// Extra instruments for the scanner universe (and the options spots): (provider,
/// asset_type, ticker, timeframe, days back). Downloaded like the base datasets.
const SCANNER_DATASETS: &[(&str, &str, &str, &str, i64)] = &[
    ("binance", "crypto", "SOLUSDT", "1h", 120),
    ("binance", "crypto", "XRPUSDT", "1h", 120),
    ("binance", "crypto", "BNBUSDT", "1h", 120),
    ("binance", "crypto", "DOGEUSDT", "1h", 120),
    ("yahoo", "equity", "MSFT", "1d", 730),
    ("yahoo", "equity", "TSLA", "1d", 730),
    ("yahoo", "equity", "AMZN", "1d", 730),
    ("yahoo", "equity", "META", "1d", 730),
    ("yahoo", "equity", "GOOGL", "1d", 730),
    ("yahoo", "equity", "SPY", "1d", 730),
    ("yahoo", "equity", "QQQ", "1d", 730),
];

/// (hive, color, [(provider, asset_type, ticker, timeframe, label)])
const HIVES: &[(&str, &str, &[(&str, &str, &str, &str, &str)])] = &[
    (
        "Crypto majors",
        "chart-3",
        &[
            ("binance", "crypto", "BTCUSDT", "1h", "Bitcoin"),
            ("binance", "crypto", "ETHUSDT", "15m", "Ether"),
            ("binance", "crypto", "SOLUSDT", "1h", "Solana"),
            ("binance", "crypto", "XRPUSDT", "1h", "XRP"),
            ("binance", "crypto", "BNBUSDT", "1h", "BNB"),
            ("binance", "crypto", "DOGEUSDT", "1h", "Dogecoin"),
        ],
    ),
    (
        "US mega caps",
        "chart-1",
        &[
            ("yahoo", "equity", "NVDA", "1h", "NVIDIA"),
            ("yahoo", "equity", "AAPL", "1d", "Apple"),
            ("yahoo", "equity", "MSFT", "1d", "Microsoft"),
            ("yahoo", "equity", "TSLA", "1d", "Tesla"),
            ("yahoo", "equity", "AMZN", "1d", "Amazon"),
            ("yahoo", "equity", "META", "1d", "Meta"),
            ("yahoo", "equity", "GOOGL", "1d", "Alphabet"),
        ],
    ),
    (
        "Index ETFs",
        "chart-5",
        &[("yahoo", "equity", "SPY", "1d", "S&P 500"), ("yahoo", "equity", "QQQ", "1d", "Nasdaq 100")],
    ),
];

/// (name, builder, config, color)
const SIGNALS: &[(&str, &str, &str, &str)] = &[
    ("Volume spike x2", "event", r#"{"kind":"volume_spike","factor":2.0,"period":20}"#, "chart-2"),
    ("Gap 2%", "event", r#"{"kind":"gap","pct":2.0,"direction":"either"}"#, "chart-4"),
    ("New 50-bar high", "event", r#"{"kind":"new_high","bars":50}"#, "chart-1"),
    ("Move 3% in 5 bars", "event", r#"{"kind":"move","pct":3.0,"bars":5,"direction":"either"}"#, "chart-6"),
    ("Range expansion", "event", r#"{"kind":"range_expansion","factor":1.8,"period":14}"#, "chart-7"),
    ("20-bar breakout up", "pattern", r#"{"kind":"breakout","bars":20,"direction":"up"}"#, "chart-3"),
    ("Bullish engulfing", "pattern", r#"{"kind":"engulfing","direction":"up"}"#, "chart-5"),
    ("Inside bar", "pattern", r#"{"kind":"inside_bar"}"#, "chart-8"),
    ("Tight 10-bar range", "pattern", r#"{"kind":"consolidation","bars":10,"max_range_pct":4.0}"#, "chart-2"),
    (
        "RSI oversold",
        "signal",
        r#"{"logic":"all","conditions":[{"left":{"kind":"indicator","indicator":"rsi","period":14},"op":"below","right":{"kind":"const","value":30}}]}"#,
        "chart-4",
    ),
    (
        "Above 50 SMA",
        "signal",
        r#"{"logic":"all","conditions":[{"left":{"kind":"price","field":"close"},"op":"above","right":{"kind":"indicator","indicator":"sma","period":50}}]}"#,
        "chart-1",
    ),
];

/// (name, enabled, target hive or "item:<ticker>", signals, notify, cooldown seconds)
const ALERTS: &[(&str, bool, &str, &[&str], &str, i32)] = &[
    ("Crypto volume spike", true, "Crypto majors", &["Volume spike x2"], "digest", 4 * 3600),
    ("Breakout on volume", true, "US mega caps", &["20-bar breakout up", "Volume spike x2"], "digest", 0),
    ("Oversold, buyers stepping in", true, "US mega caps", &["RSI oversold", "Bullish engulfing"], "per_hit", 0),
    ("Index gap", true, "Index ETFs", &["Gap 2%"], "per_hit", 0),
    ("Bitcoin new high", true, "item:BTCUSDT", &["New 50-bar high"], "per_hit", 6 * 3600),
    ("Trend + range expansion", true, "US mega caps", &["Above 50 SMA", "Range expansion"], "digest", 0),
    ("Squeeze watch", false, "Crypto majors", &["Inside bar", "Tight 10-bar range"], "digest", 0),
];

/// How far back the seeded hits go, and the most kept per (alert, item).
const HIT_DAYS: i64 = 30;
const HITS_PER_PAIR: usize = 12;
const BARS_READ: i64 = 1500;

async fn scanner(pool: &PgPool) -> Result<()> {
    // The scan reads through the broker like the chart: the same two keyless providers.
    sqlx::query(
        "INSERT INTO connector_modules (connector_id, module)
         SELECT c.id, 'scanner' FROM histdata_connectors c
         WHERE c.provider IN ('binance', 'yahoo')
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await?;

    for (provider, asset_type, ticker, timeframe, days) in SCANNER_DATASETS {
        match crate::demo_seed::one_dataset(pool, provider, asset_type, ticker, timeframe, *days).await {
            Ok(n) => println!("dataset {ticker} {timeframe}: {n} bars"),
            Err(e) => eprintln!("dataset {ticker} {timeframe}: skipped ({e:#})"),
        }
    }

    // Signals first: the cache's signal state and the hits are judged with them.
    let lib = Library::default();
    let mut signals: Vec<(String, Compiled)> = Vec::new();
    for (name, builder, config, color) in SIGNALS {
        let config: Value = serde_json::from_str(config).with_context(|| format!("signal {name}"))?;
        let spec = eval::parse(builder, &config).map_err(|e| anyhow::anyhow!("signal {name}: {e}"))?;
        spec.validate().map_err(|e| anyhow::anyhow!("signal {name}: {e}"))?;
        let row = scan_store::create_signal(pool, name, builder, &config, color).await?;
        let compiled = Compiled::new(row.id, name, spec, &lib).map_err(|e| anyhow::anyhow!("signal {name}: {e}"))?;
        signals.push((name.to_string(), compiled));
    }
    let signal = |name: &str| signals.iter().find(|(n, _)| n == name).map(|(_, c)| c);

    // Items and hives. An instrument whose download was skipped is left out.
    struct Seeded {
        id: Uuid,
        ticker: String,
        tf: i64,
        bars: Ohlcv,
    }
    let now = OffsetDateTime::now_utc();
    let mut items: Vec<Seeded> = Vec::new();
    let mut hives: Vec<(String, Uuid, Vec<usize>)> = Vec::new();
    for (hive_name, color, members) in HIVES {
        let hive = scan_store::create_hive(pool, hive_name, color).await?;
        let mut ids = Vec::new();
        let mut idx = Vec::new();
        for (provider, asset_type, ticker, timeframe, label) in *members {
            let Some(d) = hist_store::find_dataset(pool, provider, asset_type, ticker, timeframe).await? else {
                eprintln!("scanner item {ticker} {timeframe}: no stored bars, left out");
                continue;
            };
            let tf = crate::histdata::timeframe_secs(timeframe)?;
            let bars: Vec<Bar> = hist_store::read_bars(pool, d.id, None, None, BARS_READ).await?;
            let closed = crate::histviz_alerts::closed_bars(&bars, tf, now);
            let s = Ohlcv::of(&closed);
            let item = scan_store::create_item(
                pool,
                ItemInput {
                    provider,
                    asset_type,
                    ticker,
                    timeframe,
                    connector_id: None,
                    label,
                    color: "",
                },
            )
            .await?;
            scan_store::save_item_cache(pool, item.id, &eval::cache_of(&s, tf)).await?;
            let mut state = Map::new();
            for (_, c) in &signals {
                let held = c.judge(&s, tf, None).last().copied().unwrap_or(false);
                state.insert(c.id.to_string(), json!(held));
            }
            scan_store::merge_signal_state(pool, item.id, &Value::Object(state)).await?;
            ids.push(item.id);
            idx.push(items.len());
            items.push(Seeded { id: item.id, ticker: ticker.to_string(), tf, bars: s });
        }
        scan_store::set_hive_items(pool, hive.id, &ids).await?;
        hives.push((hive_name.to_string(), hive.id, idx));
    }
    let hive = |name: &str| hives.iter().find(|(n, _, _)| n == name);

    // Alerts, and the hits they would have recorded over the last month of stored bars.
    let since = now - Duration::days(HIT_DAYS);
    let mut hit_count = 0usize;
    for (name, enabled, target, signal_names, notify, cooldown) in ALERTS {
        let compiled: Vec<&Compiled> = signal_names.iter().filter_map(|n| signal(n)).collect();
        if compiled.len() != signal_names.len() {
            anyhow::bail!("scanner alert {name}: names a signal the seed does not define");
        }
        let (kind, target_id, members): (&str, Uuid, Vec<usize>) = match target.strip_prefix("item:") {
            Some(ticker) => match items.iter().position(|i| i.ticker == ticker) {
                Some(p) => ("item", items[p].id, vec![p]),
                None => continue,
            },
            None => match hive(target) {
                Some((_, id, idx)) => ("hive", *id, idx.clone()),
                None => continue,
            },
        };
        let signal_ids: Vec<Uuid> = compiled.iter().map(|c| c.id).collect();
        let alert = scan_store::create_alert(
            pool,
            AlertInput {
                name,
                enabled: *enabled,
                target_kind: kind,
                target_id,
                signal_ids: &signal_ids,
                notify,
                channels: &json!([]),
                cooldown_secs: *cooldown,
            },
        )
        .await?;
        if !enabled {
            continue;
        }
        sqlx::query("UPDATE scanner_alerts SET last_run_at = now() WHERE id = $1")
            .bind(alert.id)
            .execute(pool)
            .await?;
        for &p in &members {
            let it = &items[p];
            let held = eval::held_bars(&compiled, &it.bars, it.tf, None);
            let mut last_hit: Option<OffsetDateTime> = None;
            let mut picked = Vec::new();
            for i in held {
                let ts = it.bars.ts[i];
                if ts < since || !eval::cooldown_allows(last_hit, ts, *cooldown) {
                    continue;
                }
                last_hit = Some(ts);
                picked.push(i);
            }
            for &i in picked.iter().rev().take(HITS_PER_PAIR) {
                let prefix = prefix(&it.bars, i);
                let Some(v) = eval::verdict(&compiled, &prefix, it.tf, None) else { continue };
                let at = v.bar_ts + Duration::seconds(it.tf);
                sqlx::query(
                    "INSERT INTO scanner_hits (alert_id, item_id, bar_ts, \"values\", notified_at, created_at)
                     VALUES ($1, $2, $3, $4, $5, $5)
                     ON CONFLICT ON CONSTRAINT scanner_hits_once DO NOTHING",
                )
                .bind(alert.id)
                .bind(it.id)
                .bind(v.bar_ts)
                .bind(&v.values)
                .bind(at)
                .execute(pool)
                .await?;
                hit_count += 1;
            }
        }
    }

    // Two boards: the overview, and a momentum board colored by volume and by a signal.
    let main = scan_store::create_board(pool, "Overview", Some(0)).await?;
    let momentum = scan_store::create_board(pool, "Momentum", Some(1)).await?;
    let above_sma = signal("Above 50 SMA").map(|c| format!("signal:{}", c.id));
    let views: Vec<(Uuid, &str, &str, &str, Value, Value)> = vec![
        (main.id, "Crypto heat", "honeycomb", "Crypto majors", json!({"metric": "change_pct"}), json!({"size": "m", "sort": "change_pct"})),
        (main.id, "Mega caps", "grid", "US mega caps", json!({"metric": "change_pct"}), json!({"size": "l", "sort": "ticker"})),
        (main.id, "Index ETFs", "lines", "Index ETFs", json!({"metric": "change_pct"}), json!({"size": "m", "sort": "ticker"})),
        (momentum.id, "Crypto lines", "lines", "Crypto majors", json!({"metric": "change_pct"}), json!({"size": "s", "sort": "ticker"})),
        (momentum.id, "Volume vs average", "honeycomb", "US mega caps", json!({"metric": "volume_ratio"}), json!({"size": "m", "sort": "ticker"})),
        (
            momentum.id,
            "Trend filter",
            "honeycomb",
            "Crypto majors",
            json!({"metric": above_sma.unwrap_or_else(|| "change_pct".into())}),
            json!({"size": "s", "sort": "ticker"}),
        ),
    ];
    for (board_id, name, kind, hive_name, color_by, layout) in views {
        let Some((_, hive_id, _)) = hive(hive_name) else { continue };
        scan_store::create_view(
            pool,
            ViewInput { board_id, name, kind, hive_id: Some(*hive_id), color_by: &color_by, layout: &layout, sort: None },
        )
        .await?;
    }
    println!("scanner: {} items, {} hits", items.len(), hit_count);
    Ok(())
}

/// The bars up to and including `i`, so a verdict reads that bar as the newest closed one.
fn prefix(s: &Ohlcv, i: usize) -> Ohlcv {
    let n = i + 1;
    Ohlcv {
        ts: s.ts[..n].to_vec(),
        open: s.open[..n].to_vec(),
        high: s.high[..n].to_vec(),
        low: s.low[..n].to_vec(),
        close: s.close[..n].to_vec(),
        volume: s.volume[..n].to_vec(),
    }
}

// ── Options ──────────────────────────────────────────────────────────────────

/// The third Friday of the first month whose one is at least `days` away: a standard
/// monthly expiry.
fn monthly_expiry(today: Date, days: i64) -> Date {
    let floor = today + Duration::days(days);
    let (mut y, mut m) = (floor.year(), floor.month());
    loop {
        let first = Date::from_calendar_date(y, m, 1).expect("valid month start");
        let offset = (Weekday::Friday.number_days_from_monday() as i64 - first.weekday().number_days_from_monday() as i64)
            .rem_euclid(7);
        let third = first + Duration::days(offset + 14);
        if third >= floor {
            return third;
        }
        if m == Month::December {
            y += 1;
        }
        m = m.next();
    }
}

fn strike_step(spot: f64) -> f64 {
    if spot >= 100.0 { 5.0 } else if spot >= 25.0 { 1.0 } else { 0.5 }
}

/// The listed strike nearest `spot * factor`.
fn strike(spot: f64, factor: f64) -> f64 {
    let step = strike_step(spot);
    ((spot * factor) / step).round() * step
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// One leg of a saved strategy: (call?, strike factor, expiry DTE floor, signed contracts, iv).
type LegSpec = (bool, f64, i64, f64, f64);

/// (name, underlying, asset_type, stock shares, legs, notes)
const STRATEGIES: &[(&str, &str, &str, f64, &[LegSpec], &str)] = &[
    (
        "AAPL covered call",
        "AAPL",
        "equity",
        100.0,
        &[(true, 1.05, 40, -1.0, 0.24)],
        "Own 100 shares, sell the 5% out-of-the-money monthly against them. Roll at 21 days left.",
    ),
    (
        "AAPL cash-secured put",
        "AAPL",
        "equity",
        0.0,
        &[(false, 0.95, 30, -1.0, 0.27)],
        "Paid to wait for a lower entry. Cash for 100 shares set aside at the strike.",
    ),
    (
        "NVDA bull call spread",
        "NVDA",
        "equity",
        0.0,
        &[(true, 1.0, 35, 1.0, 0.46), (true, 1.10, 35, -1.0, 0.43)],
        "Defined-risk upside into the next earnings run-up.",
    ),
    (
        "SPY iron condor",
        "SPY",
        "etf",
        0.0,
        &[
            (false, 0.92, 45, 1.0, 0.22),
            (false, 0.95, 45, -1.0, 0.19),
            (true, 1.05, 45, -1.0, 0.14),
            (true, 1.08, 45, 1.0, 0.13),
        ],
        "Range view for the month. Close at half the credit or if a short strike is touched.",
    ),
    (
        "TSLA long straddle",
        "TSLA",
        "equity",
        0.0,
        &[(true, 1.0, 30, 1.0, 0.62), (false, 1.0, 30, 1.0, 0.63)],
        "Long volatility into the delivery numbers. Needs a move bigger than the debit.",
    ),
    (
        "QQQ collar",
        "QQQ",
        "etf",
        100.0,
        &[(false, 0.95, 60, 1.0, 0.21), (true, 1.05, 60, -1.0, 0.16)],
        "Protects a core holding: the call sold pays most of the put.",
    ),
    (
        "MSFT call calendar",
        "MSFT",
        "equity",
        0.0,
        &[(true, 1.0, 25, -1.0, 0.23), (true, 1.0, 85, 1.0, 0.25)],
        "Sells the front month's decay, keeps the back month's vega. Best if price pins the strike.",
    ),
];

/// The last stored close of an underlying, from whichever dataset holds it.
async fn last_close(pool: &PgPool, ticker: &str) -> Result<Option<f64>> {
    let row: Option<(f64,)> = sqlx::query_as(
        "SELECT b.close::float8 FROM histdata_bars b
         JOIN histdata_datasets d ON d.id = b.dataset_id
         WHERE d.provider = 'yahoo' AND d.ticker = $1
         ORDER BY b.ts DESC LIMIT 1",
    )
    .bind(ticker)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

async fn options(pool: &PgPool) -> Result<()> {
    let today = OffsetDateTime::now_utc().date();
    let model = Model { style: Style::American, rate: 0.04, dividend: 0.0, dividends: &[] };
    let mut saved = 0;
    for (name, underlying, asset_type, shares, legs, notes) in STRATEGIES {
        let Some(spot) = last_close(pool, underlying).await? else {
            eprintln!("options strategy {name}: no stored close for {underlying}, left out");
            continue;
        };
        let mut out: Vec<Value> = Vec::new();
        if *shares != 0.0 {
            out.push(json!({ "kind": "stock", "strike": null, "expiry": null, "qty": shares, "price": round2(spot), "iv": null }));
        }
        for (call, factor, dte, qty, iv) in *legs {
            let k = strike(spot, *factor);
            let expiry = monthly_expiry(today, *dte);
            let premium = model
                .value(*call, spot, k, today, expiry, *iv)
                .with_context(|| format!("pricing {name}"))?;
            out.push(json!({
                "kind": if *call { "call" } else { "put" },
                "strike": k,
                "expiry": format!("{:04}-{:02}-{:02}", expiry.year(), expiry.month() as u8, expiry.day()),
                "qty": qty,
                "price": round2(premium.max(0.01)),
                "iv": iv,
            }));
        }
        sqlx::query(
            "INSERT INTO options_strategies (name, underlying, asset_type, provider, legs, notes)
             VALUES ($1, $2, $3, NULL, $4, $5)",
        )
        .bind(name)
        .bind(underlying)
        .bind(asset_type)
        .bind(Value::Array(out))
        .bind(notes)
        .execute(pool)
        .await?;
        saved += 1;
    }
    println!("options: {saved} strategies");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_expiry_is_a_third_friday_past_the_floor() {
        let today = Date::from_calendar_date(2026, Month::November, 7).unwrap();
        let e = monthly_expiry(today, 30);
        assert_eq!(e, Date::from_calendar_date(2026, Month::December, 18).unwrap());
        assert_eq!(e.weekday(), Weekday::Friday);
        let near = monthly_expiry(today, 5);
        assert_eq!(near, Date::from_calendar_date(2026, Month::November, 20).unwrap());
        let wrap = monthly_expiry(Date::from_calendar_date(2026, Month::December, 20).unwrap(), 1);
        assert_eq!(wrap, Date::from_calendar_date(2027, Month::January, 15).unwrap());
    }

    #[test]
    fn strikes_land_on_listed_steps() {
        assert_eq!(strike(231.4, 1.05), 245.0);
        assert_eq!(strike(62.3, 0.95), 59.0);
        assert_eq!(strike(18.2, 1.0), 18.0);
    }

    #[test]
    fn seeded_signal_configs_parse_and_validate() {
        for (name, builder, config, _) in SIGNALS {
            let v: Value = serde_json::from_str(config).unwrap();
            let spec = eval::parse(builder, &v).unwrap_or_else(|e| panic!("{name}: {e}"));
            spec.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn alerts_name_seeded_signals_and_hives() {
        for (name, _, target, sigs, notify, _) in ALERTS {
            for s in *sigs {
                assert!(SIGNALS.iter().any(|(n, ..)| n == s), "{name}: unknown signal {s}");
            }
            assert!(["digest", "per_hit"].contains(notify));
            match target.strip_prefix("item:") {
                Some(t) => assert!(HIVES.iter().any(|(_, _, m)| m.iter().any(|i| i.2 == t)), "{name}: {t}"),
                None => assert!(HIVES.iter().any(|(h, ..)| h == target), "{name}: {target}"),
            }
        }
    }
}
