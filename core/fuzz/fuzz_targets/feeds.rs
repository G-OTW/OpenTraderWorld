//! A news feed's answer: the body as RSS/Atom, and as a JSON API answer read through a
//! configured set of paths. Never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;
use otw_store::feeds::Feed;
use serde_json::{json, Value};

fn feed(kind: &str, config: Value) -> Feed {
    Feed {
        id: uuid::Uuid::nil(),
        name: String::new(),
        kind: kind.into(),
        config,
        enabled: true,
        interval_secs: 600,
        last_fetched_at: None,
        last_success_at: None,
        last_error: None,
        color: None,
        updated_at: time::OffsetDateTime::UNIX_EPOCH,
        item_count: 0,
    }
}

fuzz_target!(|data: &[u8]| {
    let rss = feed("rss", json!({ "url": "http://feed.invalid" }));
    for ct in ["", "text/html", "application/json", "application/rss+xml"] {
        let _ = otw_feeds::parse_rss(&rss, ct, data);
    }
    if let Ok(v) = serde_json::from_slice::<Value>(data) {
        for cfg in [
            json!({}),
            json!({ "items_path": "data.articles", "title_path": "headline", "url_path": "link.0", "date_path": "ts", "id_path": "id" }),
            json!({ "items_path": "feed", "date_path": "time_published", "summary_path": "summary" }),
        ] {
            let _ = otw_feeds::items_from_api(&feed("api", cfg), &v);
        }
    }
});
