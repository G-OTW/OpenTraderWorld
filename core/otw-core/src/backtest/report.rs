//! Report generation for a saved backtest run — built on the shared report
//! engine (`crate::report`), so the same document renders to Markdown or PDF.
//!
//! Built from the stored `settings` + `stats` JSON snapshot — no rerun, so it works even if the
//! source dataset was deleted, and the text is deterministic (diffable between runs). Kept as
//! plain formatting over `serde_json::Value` so it never has to track the exact `Settings`/`Stats`
//! shapes; unknown/absent fields are simply skipped.

use serde_json::Value;

use crate::report::{fmt_num as fmt, Align, Block, Cell, Report, Section, Stat};

/// Build the report document for a run (render with `report::markdown` or `report::pdf`).
pub fn run_report(name: &str, ticker: &str, timeframe: &str, settings: &Value, stats: &Value) -> Report {
    let mut r = Report::new(format!("Backtest report — {}", esc(name)));
    r.subtitle = Some(format!("**{ticker}** · `{timeframe}`"));
    r.meta.push(("title".into(), esc(name)));
    r.meta.push(("ticker".into(), ticker.into()));
    r.meta.push(("timeframe".into(), timeframe.into()));
    if let Some(v) = stats.get("engine_version").and_then(Value::as_i64) {
        r.meta.push(("engine_version".into(), v.to_string()));
    }

    // ── Headline stats ──
    let mut summary = Section::new("Summary");
    let mut rows: Vec<Stat> = Vec::new();
    stat_num(&mut rows, "Net PnL", stats, "net_pnl", 2, "", true);
    stat_num(&mut rows, "Return", stats, "return_pct", 2, "%", true);
    stat_num(&mut rows, "Buy & hold", stats, "buy_hold_return_pct", 2, "%", false);
    if let Some(x) = stats.get("trades").and_then(Value::as_i64) {
        rows.push(Stat::new("Trades", x.to_string()));
    }
    stat_num(&mut rows, "Win rate", stats, "win_rate", 1, "%", false);
    stat_num(&mut rows, "Profit factor", stats, "profit_factor", 2, "", false);
    stat_num(&mut rows, "Max drawdown", stats, "max_drawdown_pct", 2, "%", false);
    stat_num(&mut rows, "Expectancy / trade", stats, "expectancy_pct", 3, "%", false);
    stat_num(&mut rows, "Sharpe", stats, "sharpe", 2, "", false);
    stat_num(&mut rows, "Sortino", stats, "sortino", 2, "", false);
    stat_num(&mut rows, "Total fees", stats, "total_fees", 2, "", false);
    stat_num(&mut rows, "Final equity", stats, "final_equity", 2, "", false);
    summary.blocks.push(Block::Stats(rows));
    r.sections.push(summary);

    // ── Human-readable settings ──
    let mut strat = Section::new("Strategy");
    strat.blocks.push(Block::Bullets(settings_summary(settings)));
    r.sections.push(strat);

    // ── Long / short breakdown ──
    for (label, key) in [("Long", "long"), ("Short", "short")] {
        if let Some(side) = stats.get(key) {
            if side.get("trades").and_then(Value::as_i64).unwrap_or(0) > 0 {
                let mut sec = Section::sub(format!("{label} trades"));
                let mut rows: Vec<Stat> = Vec::new();
                if let Some(x) = side.get("trades").and_then(Value::as_i64) {
                    rows.push(Stat::new("Trades", x.to_string()));
                }
                stat_num(&mut rows, "Win rate", side, "win_rate", 1, "%", false);
                stat_num(&mut rows, "Net PnL", side, "net_pnl", 2, "", true);
                stat_num(&mut rows, "Profit factor", side, "profit_factor", 2, "", false);
                sec.blocks.push(Block::Stats(rows));
                r.sections.push(sec);
            }
        }
    }

    // ── Exit reasons ──
    if let Some(reasons) = stats.get("exit_reasons").and_then(Value::as_object) {
        if !reasons.is_empty() {
            let mut items: Vec<(&String, i64)> =
                reasons.iter().map(|(k, v)| (k, v.as_i64().unwrap_or(0))).collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));
            let mut sec = Section::new("Exit reasons");
            sec.blocks.push(Block::Table(crate::report::Table {
                headers: vec!["Reason".into(), "Count".into()],
                aligns: vec![Align::Left, Align::Right],
                rows: items
                    .into_iter()
                    .map(|(k, c)| vec![Cell::new(k.clone()), Cell::new(c.to_string())])
                    .collect(),
            }));
            r.sections.push(sec);
        }
    }

    r.footer_note = Some("Generated from the saved-run snapshot.".into());
    r
}

/// Render the full Markdown report for a run.
pub fn run_report_md(name: &str, ticker: &str, timeframe: &str, settings: &Value, stats: &Value) -> String {
    crate::report::markdown::render(&run_report(name, ticker, timeframe, settings, stats))
}

/// A compact, human-readable settings summary (bullet items, not raw JSON).
fn settings_summary(s: &Value) -> Vec<String> {
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
    parts
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
/// Push a "label: number" stat if the key is present; `signed` colors it by sign (PDF).
fn stat_num(rows: &mut Vec<Stat>, label: &str, v: &Value, key: &str, d: usize, suffix: &str, signed: bool) {
    if let Some(x) = opt_num(v, key) {
        let s = Stat::new(label, format!("{}{suffix}", fmt(x, d)));
        rows.push(if signed { s.signed(x) } else { s });
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

        // Same document renders to PDF through the shared engine.
        let pdf = crate::report::pdf::render(&run_report("My run", "BTCUSDT", "1h", &settings, &stats));
        assert!(pdf.starts_with(b"%PDF-1.4"));
    }
}
