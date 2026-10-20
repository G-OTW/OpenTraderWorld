//! Script indicators on the server: compile a Pine v5/v6 compatible source with
//! `otw-script`, on a thread big enough for the parser's nesting limit.

use otw_script::diag::Diagnostic;
use otw_script::input::SymInfo;
use otw_script::output::ScriptMeta;
use otw_store::instrument_meta::InstrumentMeta;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

/// Largest source accepted, in bytes.
pub const MAX_SOURCE_BYTES: usize = 200_000;
/// Stack of the compile thread: release needs at least 2 MiB at parser depth 256.
pub const COMPILE_STACK: usize = 8 << 20;

/// What a successful compile tells the server (the program itself stays on its thread).
#[derive(Debug, Clone)]
pub struct Compiled {
    pub meta: ScriptMeta,
    pub warnings: Vec<Diagnostic>,
}

/// Compile on a dedicated big-stack thread. `Err` holds every diagnostic (errors and
/// warnings), as the editor shows them.
pub async fn compile(source: String) -> Result<Compiled, Vec<Diagnostic>> {
    let joined = tokio::task::spawn_blocking(move || {
        std::thread::Builder::new()
            .name("script-compile".into())
            .stack_size(COMPILE_STACK)
            .spawn(move || {
                otw_script::compile(&source).map(|p| Compiled { meta: p.meta().clone(), warnings: p.warnings().to_vec() })
            })
            .map(|h| h.join())
    })
    .await;
    match joined {
        Ok(Ok(Ok(r))) => r,
        // Spawn failure or a panic inside the engine: never leak it as a diagnostic.
        other => {
            tracing::error!("script compile thread failed: {:?}", other.err().map(|e| e.to_string()));
            Err(Vec::new())
        }
    }
}

// ── Chart runs on the server ──
// A chart's scripts run here over the stored warmup and the bars the chart shows, natively,
// and the answer is kept: a chart reopened on the same bars, script and inputs gets its plots
// back without running anything.

/// Answers kept, and the bytes they may hold in all.
const RUN_CACHE_ENTRIES: usize = 64;
const RUN_CACHE_BYTES: usize = 64 << 20;

#[derive(Default)]
struct RunCache {
    seq: u64,
    bytes: usize,
    map: HashMap<u64, (u64, Arc<str>)>,
}

fn run_cache() -> &'static Mutex<RunCache> {
    static CACHE: OnceLock<Mutex<RunCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn cached_run(key: u64) -> Option<Arc<str>> {
    let mut c = run_cache().lock().ok()?;
    c.seq += 1;
    let seq = c.seq;
    let hit = c.map.get_mut(&key)?;
    hit.0 = seq;
    Some(hit.1.clone())
}

fn remember_run(key: u64, answer: Arc<str>) {
    let Ok(mut c) = run_cache().lock() else { return };
    if answer.len() > RUN_CACHE_BYTES / 4 {
        return;
    }
    c.seq += 1;
    let seq = c.seq;
    c.bytes += answer.len();
    if let Some((_, old)) = c.map.insert(key, (seq, answer)) {
        c.bytes -= old.len();
    }
    while c.map.len() > RUN_CACHE_ENTRIES || c.bytes > RUN_CACHE_BYTES {
        let Some((&oldest, _)) = c.map.iter().min_by_key(|(_, (s, _))| *s) else { break };
        if let Some((_, gone)) = c.map.remove(&oldest) {
            c.bytes -= gone.len();
        }
    }
}

/// OHLCV columns, times in epoch ms. An empty `volume` means none.
#[derive(Debug, Clone, Default)]
pub struct Cols {
    pub time: Vec<i64>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub volume: Vec<f64>,
}

impl Cols {
    fn hash_into(&self, h: &mut impl Hasher) {
        self.time.hash(h);
        for col in [&self.open, &self.high, &self.low, &self.close, &self.volume] {
            col.len().hash(h);
            for v in col {
                v.to_bits().hash(h);
            }
        }
    }
}

/// Run `source` with `ctx` (the WASM host's run context: syminfo, timeframe, inputs) over
/// `warm` then `bars`, and answer like the WASM host's run, the output cut to `bars`:
/// `{"ok":true,"output":…}`, `{"ok":false,"error":…}`, `{"ok":false,"context":"…"}` or the
/// compile answer. Cached on everything the answer depends on.
pub async fn run_on_chart(source: String, ctx: String, warm: Arc<Cols>, bars: Arc<Cols>) -> Arc<str> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&source, &ctx).hash(&mut h);
    warm.hash_into(&mut h);
    bars.hash_into(&mut h);
    let key = h.finish();
    if let Some(hit) = cached_run(key) {
        return hit;
    }
    let joined = tokio::task::spawn_blocking(move || {
        std::thread::Builder::new()
            .name("script-run".into())
            .stack_size(COMPILE_STACK)
            .spawn(move || run_blocking(&source, &ctx, &warm, &bars))
            .map(|h| h.join())
    })
    .await;
    let answer: Arc<str> = match joined {
        Ok(Ok(Ok(a))) => a.into(),
        other => {
            tracing::error!("script run thread failed: {:?}", other.err().map(|e| e.to_string()));
            return json!({ "ok": false, "context": "the script could not run" }).to_string().into();
        }
    };
    remember_run(key, answer.clone());
    answer
}

fn run_blocking(source: &str, ctx: &str, warm: &Cols, bars: &Cols) -> String {
    use otw_script::host::{close_times, compile_json, RunCtx};
    use otw_script::input::{Bars, Limits, RunInput, Timeframe};
    let program = match otw_script::compile(source) {
        Ok(p) => p,
        Err(_) => return compile_json(source),
    };
    let ctx: RunCtx = match serde_json::from_str(ctx) {
        Ok(c) => c,
        Err(e) => return json!({ "ok": false, "context": e.to_string() }).to_string(),
    };
    let Some(timeframe) = Timeframe::parse(&ctx.timeframe) else {
        return json!({ "ok": false, "context": format!("unknown timeframe {}", ctx.timeframe) }).to_string();
    };
    // A chart without volume runs its warmup without volume too.
    let warm_n = warm.time.len();
    let cat = |w: &[f64], b: &[f64]| -> Vec<f64> { w[..warm_n.min(w.len())].iter().chain(b).copied().collect() };
    let time: Vec<i64> = warm.time[..warm_n].iter().chain(&bars.time).copied().collect();
    let input = RunInput {
        bars: Bars {
            time_close: close_times(&time, timeframe),
            time,
            open: cat(&warm.open, &bars.open),
            high: cat(&warm.high, &bars.high),
            low: cat(&warm.low, &bars.low),
            close: cat(&warm.close, &bars.close),
            volume: if bars.volume.is_empty() { Vec::new() } else { cat(&warm.volume, &bars.volume) },
        },
        syminfo: ctx.syminfo,
        timeframe,
        inputs: ctx.inputs,
        limits: Limits::default(),
    };
    match program.run(&input) {
        Ok(out) => json!({ "ok": true, "output": out.cut(warm_n) }),
        // A runtime error names the bar it stopped on, counted from the first bar shown.
        Err(mut e) => {
            e.bar = e.bar.saturating_sub(warm_n);
            json!({ "ok": false, "error": e })
        }
    }
    .to_string()
}

/// `SymInfo` for a backtest asset, the rules of `frontend/src/lib/script/host.js` `syminfoFor`:
/// `PROVIDER:TICKER` id, facts from the stored row (`None` = unknown, a script reading it
/// stops), crypto venues trading around the clock in UTC for the fields the row leaves unset.
pub fn syminfo_for(provider: &str, asset_type: &str, ticker: &str, meta: Option<&InstrumentMeta>) -> SymInfo {
    let m = |f: fn(&InstrumentMeta) -> Option<String>| meta.and_then(f);
    let mut s = SymInfo {
        ticker: ticker.into(),
        tickerid: format!("{}:{}", provider.to_uppercase(), ticker.to_uppercase()),
        mintick: meta.and_then(|x| x.mintick),
        pointvalue: meta.and_then(|x| x.pointvalue),
        timezone: m(|x| x.timezone.clone()),
        session: m(|x| x.session.clone()),
        kind: m(|x| x.kind.clone()),
        currency: m(|x| x.currency.clone()),
        basecurrency: m(|x| x.basecurrency.clone()),
        description: m(|x| x.description.clone()),
    };
    if asset_type == "crypto" {
        s.timezone.get_or_insert_with(|| "Etc/UTC".into());
        s.session.get_or_insert_with(|| "24x7".into());
        s.kind.get_or_insert_with(|| "crypto".into());
    }
    s
}

/// The `meta` column of a script row: `{"script": ScriptMeta, "warnings": [Diagnostic]}`.
pub fn stored_meta(c: &Compiled) -> Value {
    json!({ "script": c.meta, "warnings": c.warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use otw_script::diag::DiagCode;

    #[tokio::test]
    async fn deep_nesting_compiles() {
        let src = format!("//@version=6\nindicator(\"d\")\nplot({}close{})", "(".repeat(250), ")".repeat(250));
        let c = compile(src).await.expect("compiles");
        assert_eq!(c.meta.plots.len(), 1);
    }

    #[tokio::test]
    async fn strategy_is_refused() {
        let d = compile("//@version=6\nstrategy(\"s\")\n".into()).await.unwrap_err();
        assert!(d.iter().any(|d| d.code == DiagCode::NotIndicator), "{d:?}");
    }

    #[test]
    fn syminfo_for_mirrors_the_chart() {
        let meta = InstrumentMeta {
            provider: "binance".into(),
            asset_type: "crypto".into(),
            ticker: "btcusdt".into(),
            mintick: Some(0.01),
            pointvalue: None,
            timezone: None,
            session: Some("0000-2400".into()),
            kind: None,
            currency: Some("USDT".into()),
            basecurrency: None,
            description: None,
        };
        let s = syminfo_for("binance", "crypto", "btcusdt", Some(&meta));
        assert_eq!((s.ticker.as_str(), s.tickerid.as_str()), ("btcusdt", "BINANCE:BTCUSDT"));
        assert_eq!((s.mintick, s.pointvalue), (Some(0.01), None));
        // The crypto rule fills only what the stored row leaves unset.
        assert_eq!((s.timezone.as_deref(), s.session.as_deref(), s.kind.as_deref()), (Some("Etc/UTC"), Some("0000-2400"), Some("crypto")));
        let eq = syminfo_for("alpaca", "stock", "AAPL", None);
        assert_eq!((eq.tickerid.as_str(), eq.timezone, eq.mintick), ("ALPACA:AAPL", None, None));
    }

    #[test]
    fn stored_meta_shape() {
        let c = Compiled { meta: otw_script::compile("//@version=6\nindicator(\"t\")\nplot(close)").unwrap().meta().clone(), warnings: vec![] };
        let v = stored_meta(&c);
        assert_eq!(v["script"]["plots"][0]["title"], "Plot");
        assert!(v["warnings"].as_array().unwrap().is_empty());
    }

    fn cols(from: i64, n: i64) -> Cols {
        let c: Vec<f64> = (from..from + n).map(|i| 100.0 + i as f64).collect();
        Cols { time: (from..from + n).map(|i| i * 3_600_000).collect(), open: c.clone(), high: c.clone(), low: c.clone(), close: c, volume: vec![1.0; n as usize] }
    }
    const CTX: &str = r#"{"syminfo":{"ticker":"T","tickerid":"X:T","mintick":null,"pointvalue":null,"timezone":null,"session":null,"kind":null,"currency":null,"basecurrency":null,"description":null},"timeframe":"1h","inputs":{}}"#;

    /// The warmup runs first, the answer covers the chart's bars only.
    #[test]
    fn chart_run_cuts_the_warmup() {
        let a: Value = serde_json::from_str(&run_blocking("//@version=6\nindicator(\"t\")\nplot(ta.sma(close, 3))\nplot(bar_index)", CTX, &cols(0, 5), &cols(5, 3))).unwrap();
        assert_eq!(a["ok"], true, "{a}");
        assert_eq!(a["output"]["plots"][1]["values"], json!([5.0, 6.0, 7.0]), "bar_index counts the warmup");
        assert_eq!(a["output"]["plots"][0]["values"][0], json!(104.0), "the average already has its history");
    }

    /// A runtime error names the bar counted from the first one the chart shows.
    #[test]
    fn chart_run_error_bar_is_the_charts() {
        let a: Value = serde_json::from_str(&run_blocking("//@version=6\nindicator(\"t\")\nif bar_index == 6\n    runtime.error(\"x\")\nplot(close)", CTX, &cols(0, 5), &cols(5, 3))).unwrap();
        assert_eq!(a["ok"], false);
        assert_eq!(a["error"]["bar"], 1);
    }

    /// The same script, context and bars answer from the cache, other bars do not.
    #[tokio::test]
    async fn chart_runs_are_cached_on_their_bars() {
        let src = "//@version=6\nindicator(\"cache\")\nplot(close)".to_string();
        let warm = Arc::new(cols(0, 2));
        let a = run_on_chart(src.clone(), CTX.into(), warm.clone(), Arc::new(cols(2, 3))).await;
        let b = run_on_chart(src.clone(), CTX.into(), warm.clone(), Arc::new(cols(2, 3))).await;
        assert!(Arc::ptr_eq(&a, &b), "second run served from the cache");
        let c = run_on_chart(src, CTX.into(), warm, Arc::new(cols(2, 4))).await;
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(c.contains("\"ok\":true"));
    }
}
