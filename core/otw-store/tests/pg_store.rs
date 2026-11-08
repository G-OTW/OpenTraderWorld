//! Integration tests over a real PostgreSQL, one throwaway database per test.
//!
//! Each test gets its own database from `#[sqlx::test]`, migrated from `./migrations`, and
//! dropped once the test passes. They need a server: set `DATABASE_URL` to a superuser URL
//! of a scratch instance (never the app's own database) and enable the `pg-tests` feature:
//!
//! ```text
//! docker run -d --rm --name otw-test-pg -e POSTGRES_PASSWORD=test -p 55432:5432 postgres:18-alpine
//! DATABASE_URL=postgres://postgres:test@127.0.0.1:55432/postgres \
//!     cargo test -p otw-store --features pg-tests --test pg_store
//! ```
//!
//! Without the feature every test here is reported as ignored, so a plain `cargo test` on a
//! machine without Postgres stays green and says why.

use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use time::macros::datetime;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::histdata::{self, Bar};
use otw_store::{journal, mailbox, paper_broker};

fn bar(ts: OffsetDateTime, close: f64) -> Bar {
    Bar {
        ts,
        open: close,
        high: close,
        low: close,
        close,
        volume: 1.0,
        adj_open: None,
        adj_high: None,
        adj_low: None,
        adj_close: None,
    }
}

async fn dataset(pool: &PgPool) -> Uuid {
    histdata::upsert_dataset(pool, "binance", "crypto", "BTCUSDT", "1m").await.unwrap()
}

async fn paper_account(pool: &PgPool) -> Uuid {
    otw_store::brokers::create(pool, "paper", "paper test", &["*".to_string()], &json!({}))
        .await
        .unwrap()
        .id
}

fn order(id: Uuid, key: &str, qty: Decimal) -> paper_broker::NewOrder {
    paper_broker::NewOrder {
        id,
        key: key.into(),
        connector_id: Uuid::nil(),
        provider: "binance".into(),
        asset_type: "crypto".into(),
        ticker: "BTCUSDT".into(),
        currency: "USDT".into(),
        multiplier: Decimal::ONE,
        tick: Some(Decimal::new(1, 2)),
        side: "buy".into(),
        order_type: "limit".into(),
        qty,
        limit_price: Some(Decimal::new(6_543_210, 2)),
        stop_price: None,
        tif: "gtc".into(),
        expires_at: None,
        parent_id: None,
        oca_group: None,
        reduce_only: false,
        post_only: false,
        trail_amount: None,
        trail_pct: None,
    }
}

/// The text of a database error, whatever context the store wrapped it in.
fn db_error(e: &anyhow::Error) -> String {
    format!("{e:#}")
}

// ── Migrations ──────────────────────────────────────────────────────────────

/// An empty database takes every migration, each recorded as applied, and ends with the
/// tables the app cannot boot without.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn every_migration_applies_to_an_empty_database(pool: PgPool) {
    let files = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "sql"))
        .count() as i64;
    let (applied, failed): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE NOT success) FROM _sqlx_migrations",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(applied, files, "one row per migration file");
    assert_eq!(failed, 0);

    let key_tables = [
        "users",
        "sessions",
        "app_settings",
        "mcp_tokens",
        "mcp_calls",
        "journal_categories",
        "journal_trades",
        "histdata_datasets",
        "histdata_bars",
        "histdata_jobs",
        "broker_accounts",
        "paper_orders",
        "paper_fills",
        "paper_cash",
        "mailbox_messages",
    ];
    let present: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = ANY($1)",
    )
    .bind(&key_tables[..])
    .fetch_all(&pool)
    .await
    .unwrap();
    let missing: Vec<&&str> = key_tables.iter().filter(|t| !present.iter().any(|p| p == **t)).collect();
    assert!(missing.is_empty(), "missing tables after migration: {missing:?}");
}

// ── Users and sessions ──────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_username_is_unique(pool: PgPool) {
    otw_store::create_admin(&pool, "owner", "hash", false).await.unwrap();
    let err = otw_store::create_admin(&pool, "owner", "other", false).await.unwrap_err();
    assert!(db_error(&err).contains("duplicate key"), "{err:#}");
    assert_eq!(otw_store::list_usernames(&pool).await.unwrap(), vec!["owner".to_string()]);
}

/// A session resolves while it is live, and its token is stored only as a digest.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_session_resolves_until_it_expires(pool: PgPool) {
    let user = otw_store::create_admin(&pool, "owner", "hash", false).await.unwrap();
    otw_store::create_session(&pool, "live-token", user.id, 1, "127.0.0.1", "test").await.unwrap();
    let found = otw_store::user_for_session(&pool, "live-token", None).await.unwrap();
    assert_eq!(found.map(|u| u.id), Some(user.id));
    assert!(otw_store::user_for_session(&pool, "other-token", None).await.unwrap().is_none());

    let stored: String = sqlx::query_scalar("SELECT token_hash FROM sessions").fetch_one(&pool).await.unwrap();
    assert_ne!(stored, "live-token", "the plaintext token is never stored");
    assert_eq!(stored, otw_store::mcp::hash_token("live-token"));

    // `expires_at > now()` is a strict bound: a session expiring this instant is dead.
    sqlx::query("UPDATE sessions SET expires_at = now()").execute(&pool).await.unwrap();
    assert!(otw_store::user_for_session(&pool, "live-token", None).await.unwrap().is_none());
}

/// The sliding idle limit kills a session untouched for longer than it, even when its
/// absolute expiry is days away, and leaves one used inside the window alone.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn an_idle_session_dies_before_its_absolute_expiry(pool: PgPool) {
    let user = otw_store::create_admin(&pool, "owner", "hash", false).await.unwrap();
    otw_store::create_session(&pool, "t", user.id, 24 * 7, "127.0.0.1", "test").await.unwrap();
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '23 hours'").execute(&pool).await.unwrap();
    assert!(otw_store::user_for_session(&pool, "t", Some(24)).await.unwrap().is_some(), "inside the window");
    // The resolution above touched the row: push it back past the window again.
    sqlx::query("UPDATE sessions SET last_seen_at = now() - interval '25 hours'").execute(&pool).await.unwrap();
    assert!(otw_store::user_for_session(&pool, "t", Some(24)).await.unwrap().is_none(), "idle too long");
    assert!(
        otw_store::user_for_session(&pool, "t", None).await.unwrap().is_some(),
        "without an idle limit only the absolute expiry applies"
    );
}

// ── MCP tokens ──────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn an_mcp_token_is_found_by_its_plaintext_and_reports_first_use_once(pool: PgPool) {
    let (row, plain) = otw_store::mcp::create_token(&pool, "agent", &json!({ "journal": "r" }), None, false)
        .await
        .unwrap();
    assert!(plain.starts_with("otw_mcp_") && plain.len() == 72, "{plain}");
    let (t, first) = otw_store::mcp::find_by_token(&pool, &plain).await.unwrap().unwrap();
    assert_eq!((t.id, first), (row.id, true));
    let (_, first) = otw_store::mcp::find_by_token(&pool, &plain).await.unwrap().unwrap();
    assert!(!first, "the second use is not a first use");
    assert!(otw_store::mcp::find_by_token(&pool, &row.prefix).await.unwrap().is_none(), "a prefix is not a token");
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM mcp_tokens WHERE token_hash = $1")
        .bind(&plain)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0, "the plaintext is never a stored value");
}

// ── Historical bars ─────────────────────────────────────────────────────────

/// A bar for a timestamp already stored replaces it: one row per period, last write wins,
/// and the dataset summary follows.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn writing_a_bar_twice_keeps_one_row_with_the_last_value(pool: PgPool) {
    let id = dataset(&pool).await;
    let t0 = datetime!(2026-03-02 10:00:00 UTC);
    let t1 = datetime!(2026-03-02 10:01:00 UTC);
    histdata::write_bars(&pool, id, &[bar(t0, 1.0), bar(t1, 2.0)]).await.unwrap();
    // The same batch carrying a repeated timestamp collapses it before the insert.
    histdata::write_bars(&pool, id, &[bar(t1, 3.0), bar(t1, 4.0)]).await.unwrap();
    let bars = histdata::read_bars(&pool, id, None, None, 100).await.unwrap();
    assert_eq!(bars.iter().map(|b| (b.ts, b.close)).collect::<Vec<_>>(), vec![(t0, 1.0), (t1, 4.0)]);
    let ds = histdata::get_dataset(&pool, id).await.unwrap().unwrap();
    assert_eq!((ds.bar_count, ds.range_from, ds.range_to), (2, Some(t0), Some(t1)));
}

/// `read_bars` bounds are inclusive on both ends, and a limit keeps the most recent bars,
/// still in ascending order.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn reading_bars_honours_inclusive_bounds_and_keeps_the_newest_on_a_limit(pool: PgPool) {
    let id = dataset(&pool).await;
    let start = datetime!(2026-03-02 10:00:00 UTC);
    let bars: Vec<Bar> = (0..10).map(|i| bar(start + time::Duration::minutes(i), i as f64)).collect();
    histdata::write_bars(&pool, id, &bars).await.unwrap();
    let at = |m: i64| start + time::Duration::minutes(m);

    let got = histdata::read_bars(&pool, id, Some(at(2)), Some(at(5)), 100).await.unwrap();
    assert_eq!(got.iter().map(|b| b.close).collect::<Vec<_>>(), vec![2.0, 3.0, 4.0, 5.0], "both bounds included");

    let got = histdata::read_bars(&pool, id, Some(at(2) + time::Duration::seconds(1)), Some(at(5) - time::Duration::seconds(1)), 100)
        .await
        .unwrap();
    assert_eq!(got.iter().map(|b| b.close).collect::<Vec<_>>(), vec![3.0, 4.0], "a bound between bars excludes the bar outside it");

    let got = histdata::read_bars(&pool, id, None, None, 3).await.unwrap();
    assert_eq!(got.iter().map(|b| b.close).collect::<Vec<_>>(), vec![7.0, 8.0, 9.0], "the newest three, ascending");

    assert_eq!(histdata::latest_bar_ts(&pool, id).await.unwrap(), Some(at(9)));
}

/// A dataset with no bar answers `None` for its newest bar (one NULL row, not an error).
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn an_empty_dataset_has_no_latest_bar(pool: PgPool) {
    let id = dataset(&pool).await;
    assert_eq!(histdata::latest_bar_ts(&pool, id).await.unwrap(), None);
}

/// Bars belong to their dataset: writing into one that does not exist fails as a whole
/// (nothing half written), and deleting a dataset takes its bars with it.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn bars_need_their_dataset_and_go_with_it(pool: PgPool) {
    let ghost = Uuid::new_v4();
    let err = histdata::write_bars(&pool, ghost, &[bar(datetime!(2026-03-02 10:00:00 UTC), 1.0)]).await.unwrap_err();
    assert!(db_error(&err).contains("foreign key"), "{err:#}");

    let id = dataset(&pool).await;
    histdata::write_bars(&pool, id, &[bar(datetime!(2026-03-02 10:00:00 UTC), 1.0)]).await.unwrap();
    assert!(histdata::delete_dataset(&pool, id).await.unwrap());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM histdata_bars").fetch_one(&pool).await.unwrap();
    assert_eq!(left, 0, "bars cascade with their dataset");
}

/// The instrument key is unique: asking for the same dataset twice returns the same row.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn one_instrument_is_one_dataset(pool: PgPool) {
    let a = dataset(&pool).await;
    let b = dataset(&pool).await;
    assert_eq!(a, b);
    let other = histdata::upsert_dataset(&pool, "binance", "crypto", "BTCUSDT", "1h").await.unwrap();
    assert_ne!(a, other, "another timeframe is another dataset");
    assert_eq!(histdata::list_datasets(&pool).await.unwrap().len(), 2);
}

// ── Paper broker: exact decimals and all-or-nothing tickets ─────────────────

/// NUMERIC keeps what a float cannot: 0.1 + 0.2 sums to exactly 0.3, and a value with many
/// decimals comes back digit for digit.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn paper_cash_round_trips_exact_decimals(pool: PgPool) {
    let acct = paper_account(&pool).await;
    let tenth = Decimal::new(1, 1);
    let fifth = Decimal::new(2, 1);
    let fine = "123456789.123456789012".parse::<Decimal>().unwrap();
    for (kind, amount) in [("deposit", tenth), ("deposit", fifth), ("withdraw", fine)] {
        paper_broker::book_cash(&pool, acct, kind, amount).await.unwrap();
    }
    let moves = paper_broker::cash_moves(&pool, acct).await.unwrap();
    assert_eq!(moves.iter().map(|m| m.amount).collect::<Vec<_>>(), vec![tenth, fifth, fine], "in booking order, exact");

    let sum: Decimal = sqlx::query_scalar("SELECT SUM(amount) FROM paper_cash WHERE kind = 'deposit'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sum, Decimal::new(3, 1));
}

/// The cash ledger refuses what makes no sense: a zero amount, an unknown kind, an account
/// that does not exist.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn paper_cash_refuses_zero_unknown_kinds_and_unknown_accounts(pool: PgPool) {
    let acct = paper_account(&pool).await;
    let err = paper_broker::book_cash(&pool, acct, "deposit", Decimal::ZERO).await.unwrap_err();
    assert!(db_error(&err).contains("check constraint"), "{err:#}");
    let err = paper_broker::book_cash(&pool, acct, "gift", Decimal::ONE).await.unwrap_err();
    assert!(db_error(&err).contains("check constraint"), "{err:#}");
    let err = paper_broker::book_cash(&pool, Uuid::new_v4(), "deposit", Decimal::ONE).await.unwrap_err();
    assert!(db_error(&err).contains("foreign key"), "{err:#}");
    assert!(paper_broker::cash_moves(&pool, acct).await.unwrap().is_empty());
}

/// A ticket is written whole or not at all: a bracket whose second order is refused leaves
/// no first order behind.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_ticket_with_one_bad_order_writes_none(pool: PgPool) {
    let acct = paper_account(&pool).await;
    let first = Uuid::new_v4();
    let tickets = [order(first, "entry", Decimal::ONE), order(Uuid::new_v4(), "bad", Decimal::ZERO)];
    let err = paper_broker::insert_orders(&pool, acct, &tickets).await.unwrap_err();
    assert!(db_error(&err).contains("check constraint"), "{err:#}");
    assert!(paper_broker::order(&pool, acct, first).await.unwrap().is_none(), "rolled back with the bad one");
    assert!(paper_broker::open_orders(&pool, acct).await.unwrap().is_empty());

    // The same ticket fixed goes in whole, decimals intact.
    let tickets = [order(first, "entry", Decimal::ONE), order(Uuid::new_v4(), "more", "0.00012345".parse().unwrap())];
    paper_broker::insert_orders(&pool, acct, &tickets).await.unwrap();
    let open = paper_broker::open_orders(&pool, acct).await.unwrap();
    assert_eq!(open.len(), 2);
    let small = open.iter().find(|o| o.key == "more").unwrap();
    assert_eq!(small.qty, "0.00012345".parse::<Decimal>().unwrap());
    assert_eq!(small.limit_price, Some(Decimal::new(6_543_210, 2)));
}

/// An account's ledger goes with the account.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn deleting_a_broker_account_removes_its_paper_ledger(pool: PgPool) {
    let acct = paper_account(&pool).await;
    paper_broker::book_cash(&pool, acct, "deposit", Decimal::ONE).await.unwrap();
    paper_broker::insert_orders(&pool, acct, &[order(Uuid::new_v4(), "k", Decimal::ONE)]).await.unwrap();
    assert!(otw_store::brokers::delete(&pool, acct).await.unwrap());
    let (cash, orders): (i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM paper_cash), (SELECT count(*) FROM paper_orders)")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((cash, orders), (0, 0));
}

// ── Journal: sort and date range ────────────────────────────────────────────

fn trade(category: Uuid, ticker: &str, entry: OffsetDateTime, exit: Option<OffsetDateTime>) -> journal::TradeInput {
    serde_json::from_value(json!({
        "category_id": category,
        "ticker": ticker,
        "entry_at": entry.format(&time::format_description::well_known::Rfc3339).unwrap(),
        "exit_at": exit.map(|e| e.format(&time::format_description::well_known::Rfc3339).unwrap()),
        "entry_price": 100.0,
        "quantity": 1.0,
    }))
    .unwrap()
}

/// Trades list newest first on their effective date (exit, else entry), and the date range
/// includes both of its bounds.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn trades_sort_newest_first_and_the_range_includes_its_bounds(pool: PgPool) {
    let cat = journal::add_category(&pool, "main", None).await.unwrap().id;
    let d = |day: u8| datetime!(2026-03-01 12:00:00 UTC).replace_day(day).unwrap();
    // Open since day 1, closed on day 5: effective day 5. Open on day 3: effective day 3.
    journal::add_trade(&pool, &trade(cat, "AAA", d(1), Some(d(5)))).await.unwrap();
    journal::add_trade(&pool, &trade(cat, "BBB", d(3), None)).await.unwrap();
    journal::add_trade(&pool, &trade(cat, "CCC", d(4), None)).await.unwrap();

    let all = journal::list_trades(&pool, &journal::TradeFilter::default()).await.unwrap();
    assert_eq!(all.iter().map(|t| t.ticker.as_str()).collect::<Vec<_>>(), vec!["AAA", "CCC", "BBB"]);

    let filter = journal::TradeFilter { since: Some(d(3)), until: Some(d(4)), ..Default::default() };
    let range = journal::list_trades(&pool, &filter).await.unwrap();
    assert_eq!(range.iter().map(|t| t.ticker.as_str()).collect::<Vec<_>>(), vec!["CCC", "BBB"], "both bounds included");

    let filter = journal::TradeFilter {
        since: Some(d(3) + time::Duration::seconds(1)),
        until: Some(d(5) - time::Duration::seconds(1)),
        ..Default::default()
    };
    let range = journal::list_trades(&pool, &filter).await.unwrap();
    assert_eq!(range.iter().map(|t| t.ticker.as_str()).collect::<Vec<_>>(), vec!["CCC"]);

    // The ticker filter is case-insensitive and trimmed.
    let filter = journal::TradeFilter { ticker: Some("  aaa ".into()), ..Default::default() };
    assert_eq!(journal::list_trades(&pool, &filter).await.unwrap().len(), 1);
}

/// A trade cannot point at a category that does not exist, and deleting a category takes
/// its trades with it.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_trade_belongs_to_an_existing_category(pool: PgPool) {
    let t = datetime!(2026-03-01 12:00:00 UTC);
    let err = journal::add_trade(&pool, &trade(Uuid::new_v4(), "AAA", t, None)).await.unwrap_err();
    assert!(db_error(&err).contains("foreign key"), "{err:#}");

    let cat = journal::add_category(&pool, "main", None).await.unwrap().id;
    let id = journal::add_trade(&pool, &trade(cat, "AAA", t, None)).await.unwrap().id;
    assert!(journal::delete_category(&pool, cat).await.unwrap());
    assert!(journal::get_trade(&pool, id).await.unwrap().is_none());
}

// ── Mailbox: pagination ─────────────────────────────────────────────────────

/// Pages of messages are disjoint, cover the whole list in order, and an offset past the end
/// is an empty page. A message delivered twice is stored once.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn message_pages_are_disjoint_ordered_and_complete(pool: PgPool) {
    // An OAuth account holds no credential until its first sign-in, so none is needed here.
    let input: mailbox::AccountInput = serde_json::from_value(json!({
        "name": "inbox", "host": "imap.example.invalid", "username": "me",
        "auth_kind": "oauth", "oauth_provider": "google"
    }))
    .unwrap();
    let account = mailbox::create_account(&pool, &input).await.unwrap();
    let sender = mailbox::touch_sender(&pool, "news@example.invalid", "News", "s", "", "kept", "news")
        .await
        .unwrap();
    let base = datetime!(2026-03-01 08:00:00 UTC);
    let msg = |i: i64| mailbox::NewMessage {
        account_id: account.id,
        sender_id: sender.id,
        uid: i,
        uid_validity: 1,
        message_id: format!("<{i}@example.invalid>"),
        subject: format!("m{i}"),
        snippet: String::new(),
        body_html: None,
        body_text: None,
        list_unsubscribe: String::new(),
        list_unsub_post: false,
        has_remote_images: false,
        size_bytes: 1,
        received_at: base + time::Duration::hours(i),
    };
    for i in 0..7 {
        assert!(mailbox::insert_message(&pool, &msg(i)).await.unwrap().is_some());
    }
    assert!(mailbox::insert_message(&pool, &msg(3)).await.unwrap().is_none(), "a re-delivery is not a new message");

    let page = |offset: i64| {
        let pool = pool.clone();
        async move {
            let q = mailbox::MessageQuery { limit: Some(3), offset: Some(offset), ..Default::default() };
            mailbox::list_messages(&pool, &q).await.unwrap().into_iter().map(|m| m.subject).collect::<Vec<_>>()
        }
    };
    let (p1, p2, p3, past) = (page(0).await, page(3).await, page(6).await, page(7).await);
    assert_eq!(p1, vec!["m6", "m5", "m4"], "newest first");
    assert_eq!(p2, vec!["m3", "m2", "m1"]);
    assert_eq!(p3, vec!["m0"]);
    assert!(past.is_empty());

    // The limit is clamped, never trusted: zero or negative still answers one page.
    let q = mailbox::MessageQuery { limit: Some(0), offset: Some(-5), ..Default::default() };
    assert_eq!(mailbox::list_messages(&pool, &q).await.unwrap().len(), 1);
}

// ── Settings ────────────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_setting_is_one_value_per_key_last_write_wins(pool: PgPool) {
    assert_eq!(otw_store::settings::get_or(&pool, "otw_test_key", "dflt").await.unwrap(), "dflt");
    otw_store::settings::set(&pool, "otw_test_key", "a").await.unwrap();
    otw_store::settings::set(&pool, "otw_test_key", "b").await.unwrap();
    assert_eq!(otw_store::settings::get(&pool, "otw_test_key").await.unwrap().as_deref(), Some("b"));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM app_settings WHERE key = 'otw_test_key'").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1);
}

// ── Feeds ───────────────────────────────────────────────────────────────────

/// A source's Retry-After moves the feed's next poll later, never earlier than the cadence
/// already set.
#[sqlx::test(migrations = "./migrations")]
#[cfg_attr(not(feature = "pg-tests"), ignore = "needs Postgres: set DATABASE_URL and run with --features pg-tests")]
async fn a_retry_after_defers_the_next_poll_and_never_brings_it_earlier(pool: PgPool) {
    let feed = otw_store::feeds::create_feed(&pool, "t", "rss", &json!({ "url": "http://x.test" }), 600, None).await.unwrap();
    let next = |pool: PgPool| async move {
        sqlx::query_scalar::<_, f64>("SELECT extract(epoch FROM next_run_at - now())::float8 FROM feeds WHERE id = $1")
            .bind(feed.id)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert!(next(pool.clone()).await <= 0.0, "a new feed is due at once");
    otw_store::feeds::defer(&pool, feed.id, 120).await.unwrap();
    let after = next(pool.clone()).await;
    assert!((119.0..=120.0).contains(&after), "deferred by the asked wait: {after}");
    otw_store::feeds::defer(&pool, feed.id, 10).await.unwrap();
    let kept = next(pool.clone()).await;
    assert!(kept > 100.0, "a shorter wait does not bring the poll earlier: {kept}");
}
