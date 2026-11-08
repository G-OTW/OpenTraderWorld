//! Corpus runner: how much of a folder of public Pine indicators the engine handles.
//!
//!   cargo run -p otw-script --release --example corpus -- <dir> [--stage parse|compile|run]
//!
//! Keeps `//@version=5|6` files that declare `indicator(`, runs each stage inside
//! `catch_unwind`, prints a JSON report. See scripts/pine-corpus/README.md.

#[path = "support/bars.rs"]
mod bars;

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use otw_script::diag::{DiagCode, Diagnostic};
use otw_script::input::{Limits, RunInput, SymInfo, TfUnit, Timeframe};

#[derive(PartialEq, PartialOrd, Clone, Copy)]
enum Stage {
    Parse,
    Compile,
    Run,
}

/// What blocks a file: an error code plus, for syntax and unsupported, its first argument.
fn blocker(d: &Diagnostic) -> String {
    match d.code {
        DiagCode::Unsupported | DiagCode::UnknownName | DiagCode::Syntax => {
            format!("{:?}: {}", d.code, d.args.first().cloned().unwrap_or_default())
        }
        c => format!("{c:?}"),
    }
}

/// 2000 synthetic hourly bars (seed 42) with full instrument metadata, so the run measures
/// the engine, not missing metadata.
fn run_input() -> RunInput {
    let b = bars::synth(2000, 42);
    let syminfo = SymInfo {
        ticker: "TEST".into(),
        tickerid: "X:TEST".into(),
        mintick: Some(0.01),
        pointvalue: Some(1.0),
        timezone: Some("UTC".into()),
        session: Some("24x7".into()),
        kind: Some("crypto".into()),
        currency: Some("USD".into()),
        basecurrency: Some("TST".into()),
        description: Some("Test".into()),
    };
    RunInput { bars: b, syminfo, timeframe: Timeframe { multiplier: 60, unit: TfUnit::Minute }, inputs: BTreeMap::new(), limits: Limits::default() }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).expect("usage: corpus <dir> [--stage parse|compile|run]");
    let stage = match args.iter().position(|a| a == "--stage").and_then(|i| args.get(i + 1)).map(String::as_str) {
        Some("compile") => Stage::Compile,
        Some("run") => Stage::Run,
        _ => Stage::Parse,
    };
    // `--file <path>`: print every diagnostic of one file.
    if let Some(f) = args.iter().position(|a| a == "--file").and_then(|i| args.get(i + 1)) {
        let src = std::fs::read_to_string(f).expect("file");
        match otw_script::parser::parse(&src) {
            Ok(script) => {
                let checked = otw_script::check::check(&script);
                checked.diags.iter().for_each(|d| println!("{}:{} {:?} {:?}", d.span.line, d.span.col, d.code, d.args));
                println!("parsed");
            }
            Err(diags) => diags.iter().for_each(|d| println!("{}:{} {:?} {:?}", d.span.line, d.span.col, d.code, d.args)),
        }
        return;
    }
    std::panic::set_hook(Box::new(|_| {}));

    let (mut files, mut kept, mut parsed, mut compiled, mut ran, mut panics) = (0, 0, 0, 0, 0, 0);
    let mut ran_strict = 0;
    // Visual-only features skipped in scripts that ran, by script count.
    let mut skips: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut errors: BTreeMap<String, usize> = BTreeMap::new();
    let mut examples: BTreeMap<String, String> = BTreeMap::new();
    let mut paths: Vec<_> = std::fs::read_dir(dir).expect("dir").flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        files += 1;
        let Ok(src) = std::fs::read_to_string(&path) else { continue };
        let v56 = src.contains("@version=5") || src.contains("@version=6");
        if !v56 || !src.contains("indicator(") {
            continue;
        }
        kept += 1;
        if std::env::var_os("CORPUS_TRACE").is_some() {
            eprintln!("{}", path.display());
        }
        match catch_unwind(AssertUnwindSafe(|| otw_script::parser::parse(&src))) {
            Err(_) => {
                panics += 1;
                examples.entry("PANIC".into()).or_insert_with(|| path.display().to_string());
            }
            Ok(Ok(script)) => {
                parsed += 1;
                if stage >= Stage::Compile {
                    match catch_unwind(AssertUnwindSafe(|| otw_script::check::check(&script))) {
                        Err(_) => {
                            panics += 1;
                            examples.entry("PANIC".into()).or_insert_with(|| path.display().to_string());
                        }
                        Ok(checked) if !checked.has_errors() => {
                            compiled += 1;
                            if stage == Stage::Run {
                                let res = catch_unwind(AssertUnwindSafe(|| otw_script::compile(&src).map(|p| p.run(&run_input()))));
                                match res {
                                    Err(_) => {
                                        panics += 1;
                                        examples.entry("PANIC".into()).or_insert_with(|| path.display().to_string());
                                    }
                                    Ok(Ok(Ok(_))) => {
                                        ran += 1;
                                        // Strict: nothing skipped as visual-only.
                                        let skipped: std::collections::BTreeSet<String> = checked
                                            .diags
                                            .iter()
                                            .filter(|d| d.code == otw_script::diag::DiagCode::SkippedVisual)
                                            .map(|d| d.args.first().cloned().unwrap_or_default())
                                            .collect();
                                        // Strict refuses unsupported calls. Alerting (out of scope by
                                        // decision) and cosmetic arguments are accepted as is.
                                        const ACCEPTED: [&str; 6] = ["alertcondition", "alert", "editable", "force_overlay", "show_last", "trackprice"];
                                        if skipped.iter().all(|f| ACCEPTED.contains(&f.as_str()) || f.starts_with("alert.") || f == "format") {
                                            ran_strict += 1;
                                        }
                                        for f in skipped {
                                            *skips.entry(f).or_default() += 1;
                                        }
                                    }
                                    Ok(Ok(Err(e))) => {
                                        let b = format!("Runtime {:?}: {}", e.code, e.args.first().cloned().unwrap_or_default());
                                        *errors.entry(b.clone()).or_default() += 1;
                                        examples.entry(b).or_insert_with(|| format!("{}:{}", path.display(), e.span.line));
                                    }
                                    Ok(Err(diags)) => {
                                        for d in diags.iter().filter(|d| d.is_error()).take(1) {
                                            let b = format!("Compile {}", blocker(d));
                                            *errors.entry(b.clone()).or_default() += 1;
                                            examples.entry(b).or_insert_with(|| format!("{}:{}", path.display(), d.span.line));
                                        }
                                    }
                                }
                            }
                        }
                        Ok(checked) => {
                            let mut seen = std::collections::BTreeSet::new();
                            for d in checked.diags.iter().filter(|d| d.is_error()) {
                                let b = blocker(d);
                                if seen.insert(b.clone()) {
                                    *errors.entry(b.clone()).or_default() += 1;
                                    examples.entry(b).or_insert_with(|| format!("{}:{}", path.display(), d.span.line));
                                }
                            }
                        }
                    }
                }
            }
            Ok(Err(diags)) => {
                let mut seen = std::collections::BTreeSet::new();
                for d in diags.iter().filter(|d| d.is_error()) {
                    let b = blocker(d);
                    if seen.insert(b.clone()) {
                        *errors.entry(b.clone()).or_default() += 1;
                        examples.entry(b).or_insert_with(|| format!("{}:{}", path.display(), d.span.line));
                    }
                }
            }
        }
    }
    let mut top: Vec<_> = errors.into_iter().collect();
    top.sort_by_key(|e| std::cmp::Reverse(e.1));
    let top: Vec<_> = top
        .into_iter()
        .take(400)
        .map(|(k, n)| serde_json::json!([k, n, examples.get(&k).cloned().unwrap_or_default()]))
        .collect();
    let report = serde_json::json!({
        "files": files,
        "v5v6_indicators": kept,
        "parsed": parsed,
        "compiled": compiled,
        "ran": ran,
        "ran_strict": ran_strict,
        "skipped_visual": skips,
        "panics": panics,
        "panic_example": examples.get("PANIC"),
        "top_errors": top,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
