//! Fetching a feed from a mock server: what each answer a source can give turns into.
//! Bodies are synthetic. Format problems carry the `format:` prefix the UI suppresses while a
//! feed is being set up; everything else is a hard error.

use std::collections::HashMap;

use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

fn feed(kind: &str, config: Value) -> Feed {
    Feed {
        id: uuid::Uuid::nil(),
        name: "Test feed".into(),
        kind: kind.into(),
        config,
        enabled: true,
        interval_secs: 600,
        last_fetched_at: None,
        last_success_at: None,
        last_error: None,
        color: None,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        item_count: 0,
    }
}

async fn serve(t: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(t).mount(&server).await;
    server
}

async fn rss(t: ResponseTemplate) -> anyhow::Result<Vec<NewItem>> {
    let server = serve(t).await;
    fetch(&feed("rss", json!({ "url": format!("{}/feed.xml", server.uri()) })), &HashMap::new()).await
}

async fn api(t: ResponseTemplate, extra: Value) -> anyhow::Result<Vec<NewItem>> {
    let server = serve(t).await;
    let mut cfg = json!({ "url": format!("{}/news", server.uri()) });
    for (k, v) in extra.as_object().unwrap() {
        cfg[k] = v.clone();
    }
    fetch(&feed("api", cfg), &HashMap::new()).await
}

/// The error a fetch failed with, as the feed's health line shows it.
fn fails(r: anyhow::Result<Vec<NewItem>>) -> String {
    match r {
        Ok(v) => panic!("expected an error, got {} item(s)", v.len()),
        Err(e) => format!("{e:#}"),
    }
}

fn xml(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).insert_header("content-type", "application/rss+xml").set_body_string(body.to_string())
}

const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel><title>Markets</title>
<item><title>Fed holds</title><link>https://news.example.test/a</link><guid>id-a</guid>
<pubDate>Mon, 09 Mar 2026 09:30:00 -0400</pubDate><description>Rates unchanged.</description></item>
<item><title>No guid</title><link>https://news.example.test/b</link></item>
</channel></rss>"#;

#[tokio::test]
async fn an_rss_feed_reads_its_items_with_utc_instants() {
    let items = rss(xml(RSS)).await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title, "Fed holds");
    assert_eq!(items[0].source_name, "Test feed");
    // 09:30 New York on summer time (since 8 March) is 13:30 UTC.
    assert_eq!(items[0].published_at.map(|d| d.unix_timestamp()), Some(1_773_063_000));
    assert_eq!(items[0].summary.as_deref(), Some("Rates unchanged."));
}

#[tokio::test]
async fn an_rss_item_keeps_a_stable_dedup_key() {
    let a = rss(xml(RSS)).await.unwrap();
    let b = rss(xml(RSS)).await.unwrap();
    assert_eq!(a.iter().map(|i| &i.dedup_key).collect::<Vec<_>>(), b.iter().map(|i| &i.dedup_key).collect::<Vec<_>>());
    assert_ne!(a[0].dedup_key, a[1].dedup_key);
}

#[tokio::test]
async fn an_html_page_with_a_200_is_a_hard_error_naming_its_title() {
    let page = "<!DOCTYPE html><html><head><title>Access denied</title></head><body/></html>";
    let e = fails(rss(ResponseTemplate::new(200).insert_header("content-type", "text/html").set_body_string(page)).await);
    let msg = e;
    assert!(msg.contains("HTML page") && msg.contains("Access denied"), "{msg}");
    assert!(!msg.starts_with(FORMAT_ERR_PREFIX));
}

#[tokio::test]
async fn an_empty_rss_answer_is_a_hard_error() {
    let e = fails(rss(ResponseTemplate::new(200).set_body_string("")).await);
    assert!(e.contains("empty"), "{e}");
}

#[tokio::test]
async fn a_truncated_rss_document_is_a_format_error() {
    let cut = &RSS[..RSS.find("<item><title>No guid").unwrap() + 12];
    let e = fails(rss(xml(cut)).await);
    assert!(e.starts_with(FORMAT_ERR_PREFIX), "{e}");
}

#[tokio::test]
async fn an_rss_outage_is_a_hard_error_with_its_status() {
    for status in [404, 429, 500, 503] {
        let e = fails(rss(ResponseTemplate::new(status)).await);
        let msg = e;
        assert!(msg.contains(&status.to_string()), "{status}: {msg}");
        assert!(!msg.starts_with(FORMAT_ERR_PREFIX));
    }
}

#[tokio::test]
async fn an_api_feed_reads_items_through_its_paths() {
    let body = json!({ "data": { "articles": [
        { "headline": "A", "link": "https://x.test/a", "ts": "2026-03-09T13:30:00Z", "id": 7 },
        { "headline": "B", "link": null, "ts": "not a date", "id": 8 },
    ] } });
    let cfg = json!({ "items_path": "data.articles", "title_path": "headline", "url_path": "link", "date_path": "ts", "id_path": "id" });
    let items = api(ResponseTemplate::new(200).set_body_json(body), cfg).await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!((items[0].title.as_str(), items[0].dedup_key.as_str()), ("A", "7"));
    assert_eq!(items[0].published_at.map(|d| d.unix_timestamp()), Some(1_773_063_000));
    // A null link and an unreadable date are absent, not invented.
    assert_eq!((items[1].url.as_deref(), items[1].published_at), (None, None));
}

#[tokio::test]
async fn an_api_quota_notice_with_a_200_is_a_hard_error() {
    let body = json!({ "Note": "Thank you for using Alpha Vantage! Our standard API rate limit is 25 requests per day." });
    let e = fails(api(ResponseTemplate::new(200).set_body_json(body), json!({ "items_path": "feed" })).await);
    let msg = e;
    assert!(msg.contains("rate limit") && !msg.starts_with(FORMAT_ERR_PREFIX), "{msg}");
}

#[tokio::test]
async fn malformed_api_json_is_a_format_error() {
    let e = fails(api(ResponseTemplate::new(200).set_body_string(r#"{"feed":[{"title":"A""#), json!({})).await);
    assert!(e.starts_with(FORMAT_ERR_PREFIX), "{e}");
}

#[tokio::test]
async fn an_items_path_that_matches_nothing_names_the_keys_it_saw() {
    let e = fails(api(ResponseTemplate::new(200).set_body_json(json!({ "results": [], "page": 1 })), json!({ "items_path": "feed" })).await);
    let msg = e;
    assert!(msg.starts_with(FORMAT_ERR_PREFIX) && msg.contains("results") && msg.contains("page"), "{msg}");
}

#[tokio::test]
async fn an_api_throttle_is_a_hard_error_naming_429() {
    let e = fails(api(ResponseTemplate::new(429).insert_header("Retry-After", "30"), json!({})).await);
    let msg = e;
    assert!(msg.contains("429") && !msg.starts_with(FORMAT_ERR_PREFIX), "{msg}");
}

#[tokio::test]
async fn an_empty_api_array_is_no_items() {
    assert!(api(ResponseTemplate::new(200).set_body_json(json!([])), json!({})).await.unwrap().is_empty());
}

/// The secret reaches the request, substituted in the query and the headers.
#[tokio::test]
async fn secrets_are_substituted_into_query_and_headers() {
    let server = MockServer::start().await;
    Mock::given(path("/news"))
        .and(query_param("apikey", "K3Y-123"))
        .and(wiremock::matchers::header("X-Token", "T0K"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "title": "ok" }])))
        .mount(&server)
        .await;
    let cfg = json!({ "url": format!("{}/news", server.uri()), "query": { "apikey": "{{secret:key}}" }, "headers": { "X-Token": "{{vault.tok}}" } });
    let secrets: HashMap<String, String> = [("key".to_string(), "K3Y-123".to_string()), ("vault.tok".to_string(), "T0K".to_string())].into();
    let items = fetch(&feed("api", cfg), &secrets).await.unwrap();
    assert_eq!(items.len(), 1);
}

#[tokio::test]
async fn an_unknown_kind_or_a_missing_url_is_refused() {
    assert!(fails(fetch(&feed("smoke", json!({})), &HashMap::new()).await).contains("unknown feed kind"));
    assert!(fails(fetch(&feed("rss", json!({})), &HashMap::new()).await).contains("config.url"));
    assert!(fails(fetch(&feed("api", json!({})), &HashMap::new()).await).contains("config.url"));
}

// ── Retry-After and timeout ──────────────────────────────────────────────────

/// A throttled source that says when to come back: the fetch fails (the poll is not
/// retried on the spot) and carries the wait, which the scheduler turns into the next poll.
#[tokio::test]
async fn a_429_with_retry_after_seconds_carries_the_wait() {
    let e = api(ResponseTemplate::new(429).insert_header("Retry-After", "30"), json!({})).await.err().expect("an error");
    assert_eq!(retry_after(&e), Some(Duration::from_secs(30)));
    let msg = format!("{e:#}");
    assert!(msg.contains("429") && !msg.starts_with(FORMAT_ERR_PREFIX), "{msg}");
}

/// The HTTP-date form is read too, and a date far ahead is clamped rather than parking the
/// feed for decades.
#[tokio::test]
async fn a_503_with_an_http_date_retry_after_is_read_and_clamped() {
    let t = ResponseTemplate::new(503).insert_header("Retry-After", "Fri, 01 Jan 2100 00:00:00 GMT");
    let e = rss(t).await.err().expect("an error");
    assert_eq!(retry_after(&e), Some(MAX_RETRY_AFTER));
    assert!(format!("{e:#}").contains("503"));
}

/// No header, an unreadable one, or a status that does not throttle: a plain error, polled
/// again on the feed's own cadence.
#[tokio::test]
async fn a_retry_after_is_only_honoured_when_readable_and_on_429_or_503() {
    for t in [
        ResponseTemplate::new(429),
        ResponseTemplate::new(429).insert_header("Retry-After", "soon"),
        ResponseTemplate::new(500).insert_header("Retry-After", "30"),
    ] {
        let e = api(t, json!({})).await.err().expect("an error");
        assert_eq!(retry_after(&e), None, "{e:#}");
    }
}

#[test]
fn retry_after_reads_seconds_and_http_dates_from_an_injected_now() {
    let now = time::macros::datetime!(2026-11-07 10:00:00 UTC);
    assert_eq!(parse_retry_after("120", now), Some(Duration::from_secs(120)));
    assert_eq!(parse_retry_after(" 0 ", now), Some(Duration::ZERO));
    assert_eq!(parse_retry_after("Sat, 07 Nov 2026 10:01:30 GMT", now), Some(Duration::from_secs(90)));
    assert_eq!(parse_retry_after("Sat, 07 Nov 2026 09:00:00 GMT", now), Some(Duration::ZERO), "a date already past is no wait");
    assert_eq!(parse_retry_after("999999999", now), Some(MAX_RETRY_AFTER));
    assert_eq!(parse_retry_after("-5", now), None);
    assert_eq!(parse_retry_after("tomorrow", now), None);
}

#[test]
fn the_default_timeout_is_twenty_seconds() {
    assert_eq!(FetchConfig::default().timeout, Duration::from_secs(20));
}

/// The timeout is the caller's: a source slower than it fails as a timeout, a hard error.
#[tokio::test]
async fn a_source_slower_than_the_injected_timeout_fails_as_a_timeout() {
    let server = serve(ResponseTemplate::new(200).set_body_json(json!([])).set_delay(Duration::from_secs(30))).await;
    let cfg = FetchConfig { timeout: Duration::from_millis(50) };
    let f = feed("api", json!({ "url": format!("{}/news", server.uri()) }));
    let e = fetch_with(&f, &HashMap::new(), &cfg).await.err().expect("an error");
    assert!(
        e.chain().any(|c| c.downcast_ref::<reqwest::Error>().is_some_and(|r| r.is_timeout())),
        "{e:#}"
    );
    assert!(!format!("{e:#}").starts_with(FORMAT_ERR_PREFIX));
}
