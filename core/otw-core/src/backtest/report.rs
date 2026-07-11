//! Markdown report generation for a saved backtest run.
//!
//! Built from the stored `settings` + `stats` JSON snapshot — no rerun, so it works even if the
//! source dataset was deleted, and the text is deterministic (diffable between runs). Kept as
//! plain formatting over `serde_json::Value` so it never has to track the exact `Settings`/`Stats`
//! shapes; unknown/absent fields are simply skipped.

use serde_json::Value;

/// Render the full Markdown report for a run.
pub fn run_report_md(name: &str, ticker: &str, timeframe: &str, settings: &Value, stats: &Value) -> String {
    let mut o = String::new();

    // Front matter (YAML) — machine-readable header.
    o.push_str("---\n");
    o.push_str(&format!("title: \"{}\"\n", esc(name)));
    o.push_str(&format!("ticker: {ticker}\n"));
    o.push_str(&format!("timeframe: {timeframe}\n"));
    if let Some(v) = stats.get("engine_version").and_then(Value::as_i64) {
        o.push_str(&format!("engine_version: {v}\n"));
    }
    o.push_str("---\n\n");

    o.push_str(&format!("# Backtest report — {}\n\n", esc(name)));
    o.push_str(&format!("**{ticker}** · `{timeframe}`\n\n"));

    // ── Headline stats ──
    o.push_str("## Summary\n\n");
    o.push_str("| Metric | Value |\n|---|---|\n");
    row_num(&mut o, "Net PnL", stats, "net_pnl", 2, "");
    row_num(&mut o, "Return", stats, "return_pct", 2, "%");
    row_num(&mut o, "Buy & hold", stats, "buy_hold_return_pct", 2, "%");
    row_int(&mut o, "Trades", stats, "trades");
    row_num(&mut o, "Win rate", stats, "win_rate", 1, "%");
    row_num(&mut o, "Profit factor", stats, "profit_factor", 2, "");
    row_num(&mut o, "Max drawdown", stats, "max_drawdown_pct", 2, "%");
    row_num(&mut o, "Expectancy / trade", stats, "expectancy_pct", 3, "%");
    if let Some(s) = opt_num(stats, "sharpe") {
        o.push_str(&format!("| Sharpe | {} |\n", fmt(s, 2)));
    }
    if let Some(s) = opt_num(stats, "sortino") {
        o.push_str(&format!("| Sortino | {} |\n", fmt(s, 2)));
    }
    row_num(&mut o, "Total fees", stats, "total_fees", 2, "");
    row_num(&mut o, "Final equity", stats, "final_equity", 2, "");
    o.push('\n');

    // ── Human-readable settings ──
    o.push_str("## Strategy\n\n");
    o.push_str(&settings_summary(settings));
    o.push('\n');

    // ── Long / short breakdown ──
    for (label, key) in [("Long", "long"), ("Short", "short")] {
        if let Some(side) = stats.get(key) {
            if side.get("trades").and_then(Value::as_i64).unwrap_or(0) > 0 {
                o.push_str(&format!("### {label} trades\n\n"));
                o.push_str("| Metric | Value |\n|---|---|\n");
                row_int(&mut o, "Trades", side, "trades");
                row_num(&mut o, "Win rate", side, "win_rate", 1, "%");
                row_num(&mut o, "Net PnL", side, "net_pnl", 2, "");
                row_num(&mut o, "Profit factor", side, "profit_factor", 2, "");
                o.push('\n');
            }
        }
    }

    // ── Exit reasons ──
    if let Some(reasons) = stats.get("exit_reasons").and_then(Value::as_object) {
        if !reasons.is_empty() {
            o.push_str("## Exit reasons\n\n| Reason | Count |\n|---|---|\n");
            let mut items: Vec<(&String, i64)> =
                reasons.iter().map(|(k, v)| (k, v.as_i64().unwrap_or(0))).collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));
            for (k, c) in items {
                o.push_str(&format!("| {k} | {c} |\n"));
            }
            o.push('\n');
        }
    }

    o.push_str("---\n_Generated from the saved-run snapshot._\n");
    o
}

/// A compact, human-readable settings paragraph (not raw JSON).
fn settings_summary(s: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    let kind = s.get("kind").and_then(Value::as_str).unwrap_or("signals");
    if kind == "grid" {
        parts.push("Grid strategy".into());
        if let Some(g) = s.get("grid") {
            let lo = g.get("lower").and_then(Value::as_f64).unwrap_or(0.0);
            let hi = g.get("upper").and_then(Value::as_f64).unwrap_or(0.0);
            let lv = g.get("levels").and_then(Value::as_i64).unwrap_or(0);
            let dir = g.get("direction").and_then(Value::as_str).unwrap_or("long");
            parts.push(format!("{lv} levels in [{}, {}], {dir}", fmt(lo, 2), fmt(hi, 2)));
        }
    } else {
        let mode = s.get("mode").and_then(Value::as_str).unwrap_or("long");
        parts.push(format!("Mode: {mode}"));
    }
    if let Some(sz) = s.get("sizing") {
        let m = sz.get("mode").and_then(Value::as_str).unwrap_or("");
        let detail = match m {
            "percent_equity" => format!("{}% of equity", num(sz, "percent")),
            "fixed_qty" => format!("{} qty", num(sz, "qty")),
            "risk" => format!("{}% risk/trade", num(sz, "risk_pct")),
            "equity_tiers" => "equity-tier table".into(),
            "kelly" => "fractional Kelly".into(),
            _ => m.into(),
        };
        parts.push(format!("Sizing: {detail}"));
    }
    if let Some(p) = s.get("pyramiding").and_then(Value::as_i64) {
        if p > 1 {
            parts.push(format!("Pyramiding: up to {p}"));
        }
    }
    parts.push(format!("Starting capital: {}", num(s, "starting_capital")));
    if let Some(l) = s.get("leverage").and_then(Value::as_f64) {
        if l != 1.0 {
            parts.push(format!("Leverage: {}×", fmt(l, 1)));
        }
    }
    if let Some(f) = s.get("fees") {
        let amt = f.get("amount").and_then(Value::as_f64).unwrap_or(0.0);
        if amt > 0.0 {
            let k = f.get("amount_kind").and_then(Value::as_str).unwrap_or("pct");
            let per = f.get("per").and_then(Value::as_str).unwrap_or("trade");
            parts.push(format!("Fees: {}{} / {per}", fmt(amt, 3), if k == "pct" { "%" } else { "" }));
        }
    }
    if let Some(sp) = s.get("spread_pct").and_then(Value::as_f64) {
        if sp > 0.0 {
            parts.push(format!("Spread: {}%", fmt(sp * 100.0, 3)));
        }
    }
    if let Some(oos) = s.get("oos_split_pct").and_then(Value::as_f64) {
        if oos > 0.0 {
            parts.push(format!("OOS split: {}%", fmt(oos * 100.0, 0)));
        }
    }
    if let Some(fd) = s.get("funding") {
        let r = fd.get("annual_rate_pct").and_then(Value::as_f64).unwrap_or(0.0);
        if r != 0.0 {
            parts.push(format!("Funding: {}%/yr (estimated)", fmt(r, 2)));
        }
    }
    parts.iter().map(|p| format!("- {p}\n")).collect()
}

// ── small helpers ──
fn esc(s: &str) -> String {
    s.replace('"', "'")
}
fn opt_num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}
fn num(v: &Value, key: &str) -> String {
    fmt(opt_num(v, key).unwrap_or(0.0), 2)
}
fn row_num(o: &mut String, label: &str, v: &Value, key: &str, d: usize, suffix: &str) {
    if let Some(x) = opt_num(v, key) {
        o.push_str(&format!("| {label} | {}{suffix} |\n", fmt(x, d)));
    }
}
fn row_int(o: &mut String, label: &str, v: &Value, key: &str) {
    if let Some(x) = v.get(key).and_then(Value::as_i64) {
        o.push_str(&format!("| {label} | {x} |\n"));
    }
}
/// Format a float with `d` decimals and thousands separators, trimming trailing zeros lightly.
fn fmt(x: f64, d: usize) -> String {
    if !x.is_finite() {
        return "–".into();
    }
    let s = format!("{x:.*}", d);
    // Thousands separators on the integer part.
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') { ("-", r) } else { ("", s.as_str()) };
    let (int_part, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let mut grouped = String::new();
    let bytes = int_part.as_bytes();
    for (i, ch) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*ch as char);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn report_has_sections_and_stats() {
        let settings = json!({
            "mode": "long",
            "sizing": { "mode": "percent_equity", "percent": 100 },
            "starting_capital": 10000,
            "fees": { "amount_kind": "pct", "per": "trade", "amount": 0.1 }
        });
        let stats = json!({
            "engine_version": 1, "trades": 12, "net_pnl": 1234.5, "return_pct": 12.34,
            "win_rate": 58.3, "profit_factor": 1.7, "max_drawdown_pct": 8.2,
            "final_equity": 11234.5, "buy_hold_return_pct": 9.9, "expectancy_pct": 0.5,
            "exit_reasons": { "take_profit": 7, "stop_loss": 5 }
        });
        let md = run_report_md("My run", "BTCUSDT", "1h", &settings, &stats);
        assert!(md.starts_with("---\n"), "front matter");
        assert!(md.contains("# Backtest report — My run"));
        assert!(md.contains("| Net PnL | 1,234.50 |"));
        assert!(md.contains("| Return | 12.34% |"));
        assert!(md.contains("## Strategy"));
        assert!(md.contains("Sizing: 100.00% of equity") || md.contains("100"));
        assert!(md.contains("## Exit reasons"));
        assert!(md.contains("| take_profit | 7 |"));
    }
}
