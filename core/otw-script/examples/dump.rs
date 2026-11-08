//! Native side of the native/WASM parity check (`scripts/script-parity/`): compiles and runs
//! every `.pine` of the given directories on synthetic bars and writes one JSON line per
//! script with the inputs and the native results, for `parity.mjs` to replay in WASM.
//!
//! cargo run -p otw-script --release --example dump -- --cases DIR... [--corpus DIR] --bars N --out FILE
//!
//! With `--sessions`, each line is a live sequence instead: open on all bars but the last 3
//! (the last as forming), two forming updates of that bar, then its close and the last 3 bars.

#[path = "support/bars.rs"]
mod bars;

use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use otw_script::host::{self, BarCols, RunCtx};
use otw_script::input::{Limits, SymInfo};
use serde_json::json;

fn ctx() -> RunCtx {
    RunCtx {
        syminfo: SymInfo {
            ticker: "TEST".into(),
            tickerid: "X:TEST".into(),
            mintick: Some(0.01),
            pointvalue: Some(1.0),
            timezone: Some("America/New_York".into()),
            session: Some("0930-1600".into()),
            kind: Some("stock".into()),
            currency: Some("USD".into()),
            basecurrency: None,
            description: Some("Test".into()),
        },
        timeframe: "1h".into(),
        inputs: Default::default(),
    }
}

fn pine_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "pine"))
        .collect();
    v.sort();
    v
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut cases, mut corpus, mut n, mut out, mut sessions) = (Vec::new(), None, 2000usize, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cases" => {}
            "--corpus" => corpus = it.next().cloned(),
            "--bars" => n = it.next().and_then(|x| x.parse().ok()).expect("--bars N"),
            "--out" => out = it.next().cloned(),
            "--sessions" => sessions = true,
            dir => cases.push(PathBuf::from(dir)),
        }
    }
    let out = out.expect("--out FILE");
    let mut files: Vec<PathBuf> = cases.iter().flat_map(|d| pine_files(d)).collect();
    if let Some(c) = corpus {
        // Corpus files the runner keeps: v5/v6 indicators.
        files.extend(pine_files(Path::new(&c)).into_iter().filter(|p| {
            let s = std::fs::read_to_string(p).unwrap_or_default();
            (s.contains("@version=5") || s.contains("@version=6")) && s.contains("indicator(")
        }));
    }
    let b = bars::synth(n, 42);
    let time: Vec<f64> = b.time.iter().map(|t| *t as f64).collect();
    let cols = BarCols { time: &time, open: &b.open, high: &b.high, low: &b.low, close: &b.close, volume: &b.volume };
    let ctx = ctx();
    let ctx_json = serde_json::to_string(&ctx).expect("ctx");
    let limits = Limits { timeout_ms: None, ..Limits::default() };
    let mut f = std::io::BufWriter::new(std::fs::File::create(&out).expect("out file"));
    if sessions {
        dump_sessions(&files, &b, &ctx_json, limits, &mut f);
        eprintln!("dump: {} session sequences, {n} bars → {out}", files.len());
        return;
    }
    let (mut total, mut ran) = (0, 0);
    for p in files {
        let Ok(source) = std::fs::read_to_string(&p) else { continue };
        let name = p.display().to_string();
        // A script that panics natively is a Phase 1 bug, not a parity case: skip it loudly.
        let Ok((compile, run)) = catch_unwind(AssertUnwindSafe(|| {
            let compile = host::compile_json(&source);
            let run = otw_script::compile(&source).ok().map(|prog| host::run_json(&prog, cols, &ctx_json, limits));
            (compile, run)
        })) else {
            eprintln!("panic: {name}");
            continue;
        };
        total += 1;
        ran += run.is_some() as usize;
        let line = json!({
            "name": name, "source": source, "time": time, "open": b.open, "high": b.high, "low": b.low,
            "close": b.close, "volume": b.volume, "ctx": ctx_json, "compile": compile, "run": run,
        });
        writeln!(f, "{line}").expect("write");
    }
    eprintln!("dump: {total} scripts, {ran} compiled, {n} bars → {out}");
}

/// Columns `from..to` of `b`, the close of the last one scaled by `scale`.
fn cols_json(b: &otw_script::input::Bars, from: usize, to: usize, scale: f64) -> serde_json::Value {
    let mut close = b.close[from..to].to_vec();
    let mut high = b.high[from..to].to_vec();
    let mut low = b.low[from..to].to_vec();
    if let Some(last) = close.last_mut() {
        *last *= scale;
        let k = close.len() - 1;
        high[k] = high[k].max(close[k]);
        low[k] = low[k].min(close[k]);
    }
    json!({
        "time": b.time[from..to].iter().map(|t| *t as f64).collect::<Vec<_>>(),
        "open": &b.open[from..to], "high": high, "low": low, "close": close, "volume": &b.volume[from..to],
    })
}

fn col(v: &serde_json::Value, k: &str) -> Vec<f64> {
    v[k].as_array().expect("column").iter().map(|x| x.as_f64().expect("number")).collect()
}

fn dump_sessions(files: &[PathBuf], b: &otw_script::input::Bars, ctx_json: &str, limits: Limits, f: &mut impl Write) {
    let n = b.close.len();
    let steps = [
        json!({ "op": "open", "cols": cols_json(b, 0, n - 3, 1.0), "forming": true }),
        json!({ "op": "advance", "cols": cols_json(b, n - 4, n - 3, 1.001), "forming": true }),
        json!({ "op": "advance", "cols": cols_json(b, n - 4, n - 3, 0.999), "forming": true }),
        json!({ "op": "advance", "cols": cols_json(b, n - 4, n, 1.0), "forming": false }),
    ];
    for p in files {
        let Ok(source) = std::fs::read_to_string(p) else { continue };
        let answers = catch_unwind(AssertUnwindSafe(|| {
            let mut session = None;
            let mut answers = Vec::new();
            for st in &steps {
                let c = &st["cols"];
                let (time, open, high, low, close, volume) = (col(c, "time"), col(c, "open"), col(c, "high"), col(c, "low"), col(c, "close"), col(c, "volume"));
                let cols = BarCols { time: &time, open: &open, high: &high, low: &low, close: &close, volume: &volume };
                let forming = st["forming"].as_bool().unwrap_or(false);
                if st["op"] == "open" {
                    match host::HostSession::open(&source, cols, ctx_json, forming, limits) {
                        Ok(s) => {
                            answers.push(s.output_json(0));
                            session = Some(s);
                        }
                        Err(e) => {
                            answers.push(e);
                            break;
                        }
                    }
                } else if let Some(s) = session.as_mut() {
                    answers.push(s.advance(cols, forming));
                }
            }
            answers
        }));
        let Ok(answers) = answers else {
            eprintln!("panic: {}", p.display());
            continue;
        };
        let line = json!({ "name": p.display().to_string(), "source": source, "ctx": ctx_json, "steps": steps, "answers": answers });
        writeln!(f, "{line}").expect("write");
    }
}
