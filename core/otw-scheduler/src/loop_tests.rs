//! The poll loop's cadence on paused time, and the pure helpers around a poll. No database:
//! what a tick does is a counter here.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::*;

/// Let the spawned loop run up to its next await on the paused clock.
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

fn counting(n: Arc<AtomicUsize>, fail_every_other: bool) -> impl FnMut() -> std::future::Ready<anyhow::Result<()>> {
    move || {
        let k = n.fetch_add(1, Ordering::SeqCst);
        std::future::ready(if fail_every_other && k.is_multiple_of(2) { Err(anyhow::anyhow!("db down")) } else { Ok(()) })
    }
}

#[tokio::test(start_paused = true)]
async fn the_first_tick_is_immediate_then_one_every_period() {
    let n = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn(run_every(TICK, counting(n.clone(), false)));
    settle().await;
    assert_eq!(n.load(Ordering::SeqCst), 1, "a fresh start polls at once");
    tokio::time::advance(Duration::from_secs(9)).await;
    settle().await;
    assert_eq!(n.load(Ordering::SeqCst), 1, "not before the period");
    tokio::time::advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(n.load(Ordering::SeqCst), 2);
    for _ in 0..3 {
        tokio::time::advance(TICK).await;
        settle().await;
    }
    assert_eq!(n.load(Ordering::SeqCst), 5);
    task.abort();
}

#[tokio::test(start_paused = true)]
async fn a_failed_tick_does_not_stop_the_loop() {
    let n = Arc::new(AtomicUsize::new(0));
    let task = tokio::spawn(run_every(TICK, counting(n.clone(), true)));
    settle().await;
    for _ in 0..4 {
        tokio::time::advance(TICK).await;
        settle().await;
    }
    assert_eq!(n.load(Ordering::SeqCst), 5, "every tick ran, failing or not");
    assert!(!task.is_finished());
    task.abort();
}

/// A poll slower than the period never overlaps the next one: the loop awaits each tick.
#[tokio::test(start_paused = true)]
async fn a_slow_poll_never_overlaps_the_next_tick() {
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let runs = Arc::new(AtomicUsize::new(0));
    let (r, p, c) = (running.clone(), peak.clone(), runs.clone());
    let task = tokio::spawn(run_every(TICK, move || {
        let (r, p, c) = (r.clone(), p.clone(), c.clone());
        async move {
            let now = r.fetch_add(1, Ordering::SeqCst) + 1;
            p.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(25)).await; // virtual time
            r.fetch_sub(1, Ordering::SeqCst);
            c.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }));
    for _ in 0..12 {
        tokio::time::advance(Duration::from_secs(5)).await;
        settle().await;
    }
    assert_eq!(peak.load(Ordering::SeqCst), 1, "two polls ran at once");
    assert!(runs.load(Ordering::SeqCst) >= 2);
    task.abort();
}

#[test]
fn vault_references_are_found_anywhere_in_the_config_once_each() {
    let cfg = serde_json::json!({
        "url": "https://api.example.test/v1?key={{AlphaVantage.apikey}}",
        "headers": { "X-Token": "{{Broker.token}}", "X-Again": "{{AlphaVantage.apikey}}" },
        "query": { "s": "{{secret:legacy}}", "deep": ["{{a.b.c}}", "{{.x}}", "{{y.}}", "{{unterminated"] },
    });
    assert_eq!(scan_vault_refs(&cfg), ["AlphaVantage.apikey", "Broker.token"]);
}

#[test]
fn the_rate_dashboard_groups_by_bare_host() {
    assert_eq!(host_of("https://user:pw@api.example.test:8443/v1/news?x=1"), "api.example.test");
    assert_eq!(host_of("http://feeds.example.test"), "feeds.example.test");
    assert_eq!(host_of("api.example.test/path"), "api.example.test");
    assert_eq!(host_of(""), "");
}

#[test]
fn a_secret_echoed_by_the_venue_is_redacted_everywhere() {
    let secrets: std::collections::HashMap<String, String> = [("k".to_string(), "S3CR3TKEY".to_string())].into();
    let out = sanitize_error("bad key S3CR3TKEY, retry with S3CR3TKEY", &secrets);
    assert_eq!(out, "bad key ***, retry with ***");
}
