//! Report generation for a saved backtest run — built on the shared report
//! engine (`crate::report`), so the same document renders to Markdown or PDF.
//!
//! The document mirrors what the results screen shows: headline stats, the equity curve, the
//! strategy definition, the All/Long/Short performance table, out-of-sample, per-asset and the
//! execution notes. It deliberately carries **no trade list**: a run has thousands of them and
//! the interesting part of a report is the aggregate.
//!
//! Two sources feed it. The stored `settings` + `stats` snapshot is always there (a report works
//! even if the source dataset was deleted); `extra` is the optional replay of the run, which is
//! what supplies the curve, the per-asset split, the OOS blocks and the execution counts. Kept
//! as plain formatting over `serde_json::Value` so it never has to track the exact
//! `Settings`/`Stats` shapes; unknown/absent fields are simply skipped.

use serde_json::Value;

use super::analysis;
use crate::report::{fmt_num as fmt, Align, Block, Cell, Chart, Report, Section, Stat, Table, Tone};

/// Everything a run report is built from.
pub struct RunDoc<'a> {
    pub name: &'a str,
    pub ticker: &'a str,
    pub timeframe: &'a str,
    /// Engine settings as they were posted (the strategy definition).
    pub settings: &'a Value,
    /// Stored stats snapshot; used when `extra` carries no fresher one.
    pub stats: &'a Value,
    /// Replay of the run: `stats`, `equity`, `per_asset`, `oos`, `grid`, `bars`, warm-up and the
    /// execution counters. `Value::Null` when the run could not be replayed.
    pub extra: &'a Value,
}

/// Standing "no replay" value, so `RunDoc::new` can borrow one.
static NO_EXTRA: Value = Value::Null;

impl<'a> RunDoc<'a> {
    pub fn new(name: &'a str, ticker: &'a str, timeframe: &'a str, settings: &'a Value, stats: &'a Value) -> Self {
        RunDoc { name, ticker, timeframe, settings, stats, extra: &NO_EXTRA }
    }
    pub fn with_extra(mut self, extra: &'a Value) -> Self {
        self.extra = extra;
        self
    }
    /// Freshest stats available: the replay's when it ran, else the stored snapshot.
    fn stats(&self) -> &Value {
        self.extra.get("stats").filter(|v| v.is_object()).unwrap_or(self.stats)
    }
    fn extra_get(&self, key: &str) -> Option<&Value> {
        self.extra.get(key).filter(|v| !v.is_null())
    }
}

/// Build the report document for a run (render with `report::markdown` or `report::pdf`).
pub fn run_report(doc: &RunDoc) -> Report {
    let (settings, stats) = (doc.settings, doc.stats());
    let mut r = Report::new(format!("Backtest report — {}", esc(doc.name)));
    r.subtitle = Some(format!("**{}** · `{}`", doc.ticker, doc.timeframe));
    r.meta.push(("title".into(), esc(doc.name)));
    r.meta.push(("ticker".into(), doc.ticker.into()));
    r.meta.push(("timeframe".into(), doc.timeframe.into()));
    if let Some(v) = stats.get("engine_version").and_then(Value::as_i64) {
        r.meta.push(("engine_version".into(), v.to_string()));
    }
    if let Some(n) = doc.extra_get("bars").and_then(Value::as_i64) {
        r.meta.push(("bars".into(), n.to_string()));
    }
    let period = period_line(doc);
    if let Some((from, to)) = &period {
        r.meta.push(("period_from".into(), from.clone()));
        r.meta.push(("period_to".into(), to.clone()));
    }

    // ── Headline stats ──
    let mut summary = Section::new("Summary");
    if let Some(p) = context_line(doc, &period) {
        summary.blocks.push(Block::Paragraph(p));
    }
    let is_dca = doc.extra_get("dca").filter(|v| v.is_object()).is_some();
    let mut rows: Vec<Stat> = Vec::new();
    if let Some(x) = opt_num(stats, "return_pct") {
        let bh = opt_num(stats, "buy_hold_return_pct");
        // A savings plan is not compared against buy and hold: its benchmark is the same total
        // money deployed in one shot at the start, which is what that field carries here.
        let against = if is_dca { "lump sum" } else { "buy & hold" };
        let s = Stat::new("Return", format!("{}%", fmt(x, 2)));
        rows.push(match bh {
            Some(b) => s.signed(x).hint(format!(
                "{against} {}%, edge {}{}%",
                fmt(b, 2),
                sign(x - b),
                fmt(x - b, 2)
            )),
            None => s.signed(x),
        });
    }
    if let Some(x) = opt_num(stats, "net_pnl") {
        let hint = match (opt_num(stats, "total_fees"), opt_num(stats, "final_equity")) {
            (Some(f), Some(e)) => Some(format!("{} fees · equity {}", fmt(f, 2), fmt(e, 2))),
            _ => None,
        };
        let s = Stat::new("Net PnL", fmt(x, 2)).signed(x);
        rows.push(match hint {
            Some(h) => s.hint(h),
            None => s,
        });
    }
    if let Some(x) = opt_num(stats, "win_rate") {
        let s = Stat::new("Win rate", format!("{}%", fmt(x, 1)));
        let (w, l, t) = (int(stats, "wins"), int(stats, "losses"), int(stats, "trades"));
        rows.push(match t {
            Some(t) => s.hint(format!("{}W / {}L of {t} trades", w.unwrap_or(0), l.unwrap_or(0))),
            None => s,
        });
    }
    if let Some(x) = opt_num(stats, "profit_factor") {
        // `PROFIT_FACTOR_NO_LOSSES` is a finite stand-in for "no losing trade", so it can sort
        // and serialize. Printing it as a ratio put 1,000,000,000.00 on the page, which is the
        // normal case for a savings plan whose only sell rule is a profit objective.
        let s = if x >= super::PROFIT_FACTOR_NO_LOSSES {
            Stat::new("Profit factor", "no losing trade".to_string())
        } else {
            Stat::new("Profit factor", fmt(x, 2)).signed(x - 1.0)
        };
        rows.push(match opt_num(stats, "expectancy_pct") {
            Some(e) => s.hint(format!("expectancy {}% / trade", fmt(e, 3))),
            None => s,
        });
    }
    if let Some(x) = opt_num(stats, "max_drawdown_pct") {
        let s = Stat::new("Max drawdown", format!("−{}%", fmt(x, 2))).toned(Tone::Negative);
        rows.push(match opt_num(stats, "max_drawdown") {
            Some(v) => s.hint(format!("−{}", fmt(v, 2))),
            None => s,
        });
    }
    if let Some(x) = opt_num(stats, "sharpe") {
        let s = Stat::new("Sharpe", fmt(x, 2)).signed(x);
        rows.push(match opt_num(stats, "sortino") {
            Some(v) => s.hint(format!("Sortino {}", fmt(v, 2))),
            None => s,
        });
    }
    stat_num(&mut rows, "Avg trade", stats, "avg_trade", 2, "", true);
    stat_num(&mut rows, "Final equity", stats, "final_equity", 2, "", false);
    if let Some(f) = doc.extra_get("total_funding").and_then(Value::as_f64) {
        if f != 0.0 {
            rows.push(Stat::new("Funding", fmt(f, 2)).signed(f).hint("estimate"));
        }
    }
    summary.blocks.push(Block::Stats(rows));
    r.sections.push(summary);

    // ── Equity curve ──
    if let Some(points) = curve(doc) {
        let mut sec = Section::new("Equity curve");
        sec.blocks.push(Block::Chart(Chart {
            y_label: "Account equity".into(),
            points,
            baseline: opt_num(settings, "starting_capital"),
            time_axis: true,
        }));
        r.sections.push(sec);
    }

    // ── Human-readable settings ──
    let mut strat = Section::new("Strategy");
    strat.blocks.push(Block::Bullets(settings_summary(settings)));
    r.sections.push(strat);
    for sec in sides_sections(settings) {
        r.sections.push(sec);
    }

    // ── Performance, All / Long / Short ──
    let trades = doc.extra_get("trades").map(analysis::read).unwrap_or_default();
    if let Some(t) = perf_table(stats, &trades, is_dca) {
        let mut sec = Section::new("Performance");
        sec.blocks.push(Block::Table(t));
        r.sections.push(sec);
    }

    // ── Where the per-trade results fall ──
    if let Some(t) = distribution_table(&trades) {
        let mut sec = Section::new("Return distribution");
        sec.blocks.push(Block::Table(t));
        r.sections.push(sec);
    }

    // ── In-sample vs out-of-sample ──
    if let Some(t) = oos_table(doc.extra_get("oos")) {
        let mut sec = Section::new("Out-of-sample");
        sec.blocks.push(Block::Table(t));
        sec.blocks.push(Block::Paragraph(
            "A wide gap between the two columns is the overfitting warning sign.".into(),
        ));
        r.sections.push(sec);
    }

    // ── Per-asset breakdown (portfolio runs) ──
    if let Some(t) = per_asset_table(doc.extra_get("per_asset")) {
        let mut sec = Section::new("Per-asset breakdown");
        sec.blocks.push(Block::Table(t));
        r.sections.push(sec);
        if let Some(detail) = per_asset_stats_table(doc.extra_get("per_asset"), &trades) {
            let mut sub = Section::sub("Per-asset statistics");
            sub.blocks.push(Block::Table(detail));
            r.sections.push(sub);
        }
    }

    // ── Grid-mode inventory ──
    if let Some(g) = doc.extra_get("grid").filter(|v| v.is_object()) {
        let mut sec = Section::new("Grid");
        let mut rows: Vec<Stat> = Vec::new();
        for (label, key) in [("Levels", "levels"), ("Fills", "fills"), ("Round trips", "round_trips")] {
            if let Some(x) = int(g, key) {
                rows.push(Stat::new(label, x.to_string()));
            }
        }
        if let Some(x) = opt_num(g, "end_inventory") {
            let s = Stat::new("End inventory", fmt(x, 4));
            rows.push(match opt_num(g, "end_inventory_value") {
                Some(v) => s.hint(format!("worth {}", fmt(v, 2))),
                None => s,
            });
        }
        sec.blocks.push(Block::Stats(rows));
        r.sections.push(sec);
    }

    // ── DCA plan ──
    if let Some(d) = doc.extra_get("dca").filter(|v| v.is_object()) {
        let mut sec = Section::new("Savings plan");
        let money = |k: &str| opt_num(d, k).unwrap_or(0.0);
        let contributed = money("contributed");
        let final_value = money("final_value");
        let mut rows = vec![
            Stat::new("Money in", fmt(contributed, 2)).hint(format!(
                "{} deposited during the run",
                fmt(money("deposits"), 2)
            )),
            Stat::new("Value", fmt(final_value, 2)).hint(format!(
                "{} in cash",
                fmt(money("cash"), 2)
            )),
            Stat::new("Cost basis deployed", fmt(money("invested"), 2)),
            Stat::new("Realized", fmt(money("realized_pnl"), 2)).toned(tone(money("realized_pnl"))),
            Stat::new("Unrealized", fmt(money("unrealized_pnl"), 2))
                .toned(tone(money("unrealized_pnl"))),
        ];
        let tr = money("total_return_pct");
        rows.push(Stat::new("Return on money in", format!("{}%", fmt(tr, 2))).toned(tone(tr)));
        let twr = money("twr_pct");
        rows.push(
            Stat::new("Time-weighted", format!("{}%", fmt(twr, 2)))
                .hint("deposits removed")
                .toned(tone(twr)),
        );
        if let Some(irr) = opt_num(d, "irr_pct") {
            rows.push(
                Stat::new("Money-weighted (IRR)", format!("{}%/y", fmt(irr, 2))).toned(tone(irr)),
            );
        }
        if money("withdrawn") > 0.0 {
            rows.push(
                Stat::new("Taken out", fmt(money("withdrawn"), 2))
                    .hint("proceeds withdrawn during the run, counted in the return"),
            );
        }
        rows.push(Stat::new("Fees", fmt(money("fees"), 2)));
        let lump = money("lump_sum_return_pct");
        rows.push(
            Stat::new("Lump sum at the start", format!("{}%", fmt(lump, 2)))
                .hint(format!("{} at the end", fmt(money("lump_sum_value"), 2))),
        );
        rows.push(Stat::new(
            "Fills",
            format!("{} buys, {} sells", int(d, "buys").unwrap_or(0), int(d, "sells").unwrap_or(0)),
        ));
        if let Some(u) = int(d, "underfunded").filter(|u| *u > 0) {
            rows.push(Stat::new("Underfunded tranches", u.to_string()).toned(Tone::Negative));
        }
        sec.blocks.push(Block::Stats(rows));
        r.sections.push(sec);

        if let Some(list) = d.get("assets").and_then(Value::as_array).filter(|a| !a.is_empty()) {
            let mut sub = Section::sub("Holdings");
            sub.blocks.push(Block::Table(Table {
                headers: vec![
                    "Asset".into(),
                    "Units".into(),
                    "Avg cost".into(),
                    "Last".into(),
                    "Value".into(),
                    "Weight".into(),
                    "Target".into(),
                    "Unrealized".into(),
                ],
                aligns: vec![
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                rows: list
                    .iter()
                    .map(|a| {
                        let u = opt_num(a, "unrealized_pnl").unwrap_or(0.0);
                        vec![
                            Cell::new(a.get("ticker").and_then(Value::as_str).unwrap_or("")),
                            Cell::new(fmt(opt_num(a, "units").unwrap_or(0.0), 4)),
                            Cell::new(fmt(opt_num(a, "avg_cost").unwrap_or(0.0), 2)),
                            Cell::new(fmt(opt_num(a, "last_price").unwrap_or(0.0), 2)),
                            Cell::new(fmt(opt_num(a, "value").unwrap_or(0.0), 2)),
                            Cell::new(format!("{}%", fmt(opt_num(a, "weight_pct").unwrap_or(0.0), 1))),
                            Cell::new(format!(
                                "{}%",
                                fmt(opt_num(a, "target_weight_pct").unwrap_or(0.0), 1)
                            )),
                            Cell::toned(fmt(u, 2), tone(u)),
                        ]
                    })
                    .collect(),
            }));
            r.sections.push(sub);
        }
    }

    // ── Exit reasons ──
    if let Some(reasons) = stats.get("exit_reasons").and_then(Value::as_object) {
        if !reasons.is_empty() {
            let total: i64 = reasons.values().filter_map(Value::as_i64).sum();
            let mut items: Vec<(&String, i64)> =
                reasons.iter().map(|(k, v)| (k, v.as_i64().unwrap_or(0))).collect();
            items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let mut sec = Section::new("Exit reasons");
            sec.blocks.push(Block::Table(Table {
                headers: vec!["Reason".into(), "Count".into(), "Share".into()],
                aligns: vec![Align::Left, Align::Right, Align::Right],
                rows: items
                    .into_iter()
                    .map(|(k, c)| {
                        let share = if total > 0 { c as f64 * 100.0 / total as f64 } else { 0.0 };
                        vec![
                            Cell::new(pretty_key(k)),
                            Cell::new(c.to_string()),
                            Cell::new(format!("{}%", fmt(share, 1))),
                        ]
                    })
                    .collect(),
            }));
            r.sections.push(sec);
        }
    }

    // ── What the engine refused / skipped ──
    let notes = execution_notes(doc);
    if !notes.is_empty() {
        let mut sec = Section::new("Execution notes");
        sec.blocks.push(Block::Bullets(notes));
        r.sections.push(sec);
    }

    r.footer_note = Some(if doc.extra.is_null() {
        "Generated from the saved-run snapshot: the source data was not replayed, so the curve \
         and the per-asset breakdown are unavailable."
            .into()
    } else {
        "Generated by replaying the saved run over its own datasets and window.".into()
    });
    r
}

/// Render the full Markdown report for a run.
pub fn run_report_md(doc: &RunDoc) -> String {
    crate::report::markdown::render(&run_report(doc))
}

/// Window the run covered, as (first, last) dates, from the replay.
fn period_line(doc: &RunDoc) -> Option<(String, String)> {
    let from = doc.extra_get("first_ts")?.as_str()?.to_string();
    let to = doc.extra_get("last_ts")?.as_str()?.to_string();
    Some((day(&from), day(&to)))
}

/// "2024-01-01 → 2024-06-30 · 4,320 bars · 200-bar warm-up (trading from 2024-01-09)".
fn context_line(doc: &RunDoc, period: &Option<(String, String)>) -> Option<String> {
    let mut bits: Vec<String> = Vec::new();
    if let Some((from, to)) = period {
        bits.push(format!("{from} → {to}"));
    }
    if let Some(n) = doc.extra_get("bars").and_then(Value::as_i64) {
        bits.push(format!("{} bars", fmt(n as f64, 0)));
    }
    if let Some(w) = doc.extra_get("warmup_bars").and_then(Value::as_i64).filter(|w| *w > 0) {
        let start = doc
            .extra_get("trading_start_ts")
            .and_then(Value::as_str)
            .map(|s| format!(" (trading from {})", day(s)))
            .unwrap_or_default();
        bits.push(format!("{w}-bar warm-up{start}"));
    }
    (!bits.is_empty()).then(|| bits.join(" · "))
}

/// The equity curve, thinned to a drawable number of points (both ends always kept).
fn curve(doc: &RunDoc) -> Option<Vec<(f64, f64)>> {
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};
    const MAX_POINTS: usize = 600;
    let pts = doc.extra_get("equity")?.as_array()?;
    if pts.len() < 2 {
        return None;
    }
    let stride = (pts.len() / MAX_POINTS).max(1);
    let mut out: Vec<(f64, f64)> = pts
        .iter()
        .step_by(stride)
        .filter_map(|p| {
            let ts = OffsetDateTime::parse(p.get("ts")?.as_str()?, &Rfc3339).ok()?;
            Some((ts.unix_timestamp() as f64, p.get("equity")?.as_f64()?))
        })
        .collect();
    // The last bar is the run's result: never let the stride drop it.
    if let Some(last) = pts.last() {
        if let (Some(ts), Some(eq)) = (
            last.get("ts").and_then(Value::as_str).and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok()),
            last.get("equity").and_then(Value::as_f64),
        ) {
            let p = (ts.unix_timestamp() as f64, eq);
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
    }
    (out.len() >= 2).then_some(out)
}

/// One metric per row, one column per scope: the results screen's performance table.
fn perf_table(stats: &Value, trades: &[analysis::T], is_dca: bool) -> Option<Table> {
    /// (label, key, format) where format is the same vocabulary the UI table uses.
    const ROWS: [(&str, &str, Fm); 19] = [
        ("Net profit", "net_pnl", Fm::Signed),
        ("Gross profit", "gross_profit", Fm::Num),
        ("Gross loss", "gross_loss", Fm::Neg),
        ("Profit factor", "profit_factor", Fm::Ratio),
        ("Total fees", "total_fees", Fm::Num),
        ("Total trades", "trades", Fm::Int),
        ("Winning trades", "wins", Fm::Int),
        ("Losing trades", "losses", Fm::Int),
        ("Win rate", "win_rate", Fm::Pct),
        ("Avg trade", "avg_trade", Fm::Signed),
        ("Avg winning trade", "avg_win", Fm::Num),
        ("Avg losing trade", "avg_loss", Fm::Neg),
        ("Payoff ratio", "payoff_ratio", Fm::Ratio),
        ("Expectancy / trade", "expectancy_pct", Fm::SignedPct),
        ("Largest winning trade", "largest_win", Fm::Num),
        ("Largest losing trade", "largest_loss", Fm::Neg),
        ("Max consecutive wins", "max_consec_wins", Fm::Int),
        ("Max consecutive losses", "max_consec_losses", Fm::Int),
        ("Avg bars in trade", "avg_bars_held", Fm::Bars),
    ];
    // A savings plan is long-only accumulation: a Long column identical to All and an empty
    // Short one is three columns saying one thing. And its "total fees" here are the fees
    // carried by the closed trades, which is not what the run paid: the label says so.
    let keys: &[&str] = if is_dca { &["all"] } else { &["all", "long", "short"] };
    let scopes: Vec<&Value> = keys.iter().filter_map(|k| stats.get(*k)).collect();
    if scopes.len() != keys.len() {
        return None;
    }
    let mut rows: Vec<Vec<Cell>> = ROWS
        .iter()
        .map(|(label, key, f)| {
            let label = if is_dca && *key == "total_fees" { "Fees on closed trades" } else { *label };
            let mut row = vec![Cell::new(label)];
            row.extend(scopes.iter().map(|s| metric_cell(s, key, *f)));
            row
        })
        .collect();
    // Excursions and the closed-trade drawdown live on the trades, not on `SideStats`, so they
    // only join the table when the run was replayed.
    if !trades.is_empty() {
        let by_scope: Vec<analysis::Stats> = if is_dca {
            vec![analysis::stats(trades)]
        } else {
            vec![
                analysis::stats(trades),
                analysis::stats(&trades.iter().copied().filter(|t| t.long).collect::<Vec<_>>()),
                analysis::stats(&trades.iter().copied().filter(|t| !t.long).collect::<Vec<_>>()),
            ]
        };
        for (label, pick, f) in EXCURSION_ROWS {
            let mut row = vec![Cell::new(label)];
            row.extend(by_scope.iter().map(|s| opt_cell(pick(s), f)));
            rows.push(row);
        }
    }
    let headers: Vec<String> = if is_dca {
        vec!["Metric".into(), "Realized".into()]
    } else {
        vec!["Metric".into(), "All".into(), "Long".into(), "Short".into()]
    };
    let aligns = std::iter::once(Align::Left)
        .chain(std::iter::repeat_n(Align::Right, headers.len() - 1))
        .collect();
    Some(Table { headers, aligns, rows })
}

/// The trade-derived rows the engine's `SideStats` does not carry: the excursion pair, the
/// result over the heat it took, and the drawdown measured on closed trades.
type Pick = fn(&analysis::Stats) -> Option<f64>;
const EXCURSION_ROWS: [(&str, Pick, Fm); 4] = [
    ("Avg run-up", |s| s.avg_runup, Fm::Num),
    ("Avg heat (MAE)", |s| s.avg_heat, Fm::Signed),
    ("Avg R (result / heat)", |s| s.avg_r, Fm::Ratio0),
    ("Max drawdown, closed trades", |s| (s.trades > 0).then_some(s.max_drawdown), Fm::Neg),
];

/// A cell from an optional number, "–" when the scope had nothing to average.
fn opt_cell(v: Option<f64>, f: Fm) -> Cell {
    match v {
        Some(x) => metric_cell(&serde_json::json!({ "v": x }), "v", f),
        None => Cell::new("–"),
    }
}

/// Cell formats of the performance table.
#[derive(Clone, Copy)]
enum Fm {
    /// Money, tinted by sign.
    Signed,
    /// Money, always positive by construction.
    Num,
    /// A magnitude the engine stores positive but that reads as a loss.
    Neg,
    Pct,
    SignedPct,
    /// A ratio where 1 is the break-even line.
    Ratio,
    /// A ratio tinted by its own sign (0 is the line, e.g. R).
    Ratio0,
    Int,
    Bars,
}

fn metric_cell(scope: &Value, key: &str, f: Fm) -> Cell {
    let Some(v) = opt_num(scope, key) else {
        return Cell::new("–");
    };
    match f {
        Fm::Signed => Cell::signed(fmt(v, 2), v),
        Fm::Num => Cell::new(fmt(v, 2)),
        Fm::Neg => {
            if v > 0.0 {
                Cell::toned(format!("−{}", fmt(v, 2)), Tone::Negative)
            } else {
                Cell::new(fmt(0.0, 2))
            }
        }
        Fm::Pct => Cell::new(format!("{}%", fmt(v, 1))),
        Fm::SignedPct => Cell::signed(format!("{}%", fmt(v, 3)), v),
        Fm::Ratio => {
            // The engine reports a finite sentinel instead of an infinite profit factor so it
            // can sort and serialize; a table must not print it as if it were a ratio.
            if v >= super::PROFIT_FACTOR_NO_LOSSES {
                Cell::new("no loss")
            } else if v == 0.0 {
                Cell::new(fmt(v, 2))
            } else {
                Cell::signed(fmt(v, 2), v - 1.0)
            }
        }
        Fm::Ratio0 => Cell::signed(fmt(v, 2), v),
        Fm::Int => Cell::new(fmt(v, 0)),
        Fm::Bars => Cell::new(fmt(v, 1)),
    }
}

/// In-sample vs out-of-sample, the cheap overfitting check.
fn oos_table(oos: Option<&Value>) -> Option<Table> {
    let oos = oos?;
    let (is, os) = (oos.get("in_sample")?, oos.get("out_sample")?);
    let split = oos
        .get("split_pct")
        .and_then(Value::as_f64)
        .map(|p| format!("In-sample ({}%)", fmt(p * 100.0, 0)))
        .unwrap_or_else(|| "In-sample".into());
    let out = match oos.get("split_ts").and_then(Value::as_str) {
        Some(ts) => format!("Out-of-sample (from {})", day(ts)),
        None => "Out-of-sample".into(),
    };
    const ROWS: [(&str, &str, Fm); 6] = [
        ("Return", "return_pct", Fm::SignedPct),
        ("Net PnL", "net_pnl", Fm::Signed),
        ("Win rate", "win_rate", Fm::Pct),
        ("Profit factor", "profit_factor", Fm::Ratio),
        ("Trades", "trades", Fm::Int),
        ("Max drawdown", "max_drawdown_pct", Fm::Pct),
    ];
    Some(Table {
        headers: vec!["Metric".into(), split, out],
        aligns: vec![Align::Left, Align::Right, Align::Right],
        rows: ROWS
            .iter()
            .map(|(label, key, f)| {
                vec![Cell::new(*label), metric_cell(is, key, *f), metric_cell(os, key, *f)]
            })
            .collect(),
    })
}

/// Where the per-trade results fall: one row per return bucket, winners and losers apart.
fn distribution_table(trades: &[analysis::T]) -> Option<Table> {
    let buckets = analysis::return_buckets(trades);
    if buckets.len() < 2 {
        return None;
    }
    let total = trades.len() as f64;
    Some(Table {
        headers: vec![
            "Return".into(),
            "Winners".into(),
            "Losers".into(),
            "Trades".into(),
            "Share".into(),
        ],
        aligns: vec![Align::Left, Align::Right, Align::Right, Align::Right, Align::Right],
        rows: buckets
            .iter()
            .filter(|b| b.winners + b.losers > 0)
            .map(|b| {
                let n = b.winners + b.losers;
                vec![
                    Cell::new(format!("{}% to {}%", fmt(b.from, 2), fmt(b.to, 2))),
                    Cell::toned(b.winners.to_string(), Tone::Positive),
                    Cell::toned(b.losers.to_string(), Tone::Negative),
                    Cell::new(n.to_string()),
                    Cell::new(format!("{}%", fmt(n as f64 * 100.0 / total, 1))),
                ]
            })
            .collect(),
    })
}

/// The statistics screen, one column per asset: the detail the comparison table above leaves
/// out (gross sides, expectancy, excursions, drawdown, streaks) for each ticker on its own.
fn per_asset_stats_table(per_asset: Option<&Value>, trades: &[analysis::T]) -> Option<Table> {
    let rows = per_asset?.as_array()?;
    if rows.len() < 2 || trades.is_empty() {
        return None;
    }
    let tickers: Vec<&str> = rows.iter().filter_map(|a| a.get("ticker").and_then(Value::as_str)).collect();
    if tickers.len() < 2 {
        return None;
    }
    let per: Vec<analysis::Stats> = tickers
        .iter()
        .map(|tk| analysis::stats(&trades.iter().copied().filter(|t| t.ticker == *tk).collect::<Vec<_>>()))
        .collect();
    // Nothing to say when the trades carry no ticker (a single-asset run's trades don't).
    if per.iter().all(|s| s.trades == 0) {
        return None;
    }
    const METRICS: [(&str, Pick, Fm); 15] = [
        ("Trades", |s| Some(s.trades as f64), Fm::Int),
        ("Win rate", |s| (s.trades > 0).then_some(s.win_rate), Fm::Pct),
        ("Net PnL", |s| (s.trades > 0).then_some(s.pnl), Fm::Signed),
        ("Gross profit", |s| (s.trades > 0).then_some(s.gross_win), Fm::Num),
        ("Gross loss", |s| (s.trades > 0).then_some(s.gross_loss), Fm::Neg),
        ("Profit factor", |s| s.profit_factor, Fm::Ratio),
        ("Expectancy", |s| s.expectancy, Fm::Signed),
        ("Avg trade return", |s| s.avg_pnl_pct, Fm::SignedPct),
        ("Avg win", |s| s.avg_win, Fm::Num),
        ("Avg loss", |s| s.avg_loss, Fm::Signed),
        ("Best trade", |s| s.best, Fm::Signed),
        ("Worst trade", |s| s.worst, Fm::Signed),
        ("Max drawdown, closed trades", |s| (s.trades > 0).then_some(s.max_drawdown), Fm::Neg),
        ("Avg R (result / heat)", |s| s.avg_r, Fm::Ratio0),
        ("Avg bars in trade", |s| s.avg_bars, Fm::Bars),
    ];
    let mut headers = vec!["Metric".into()];
    headers.extend(tickers.iter().map(|t| (*t).to_string()));
    let mut aligns = vec![Align::Left];
    aligns.extend(tickers.iter().map(|_| Align::Right));
    Some(Table {
        headers,
        aligns,
        rows: METRICS
            .iter()
            .map(|(label, pick, f)| {
                let mut row = vec![Cell::new(*label)];
                row.extend(per.iter().map(|s| opt_cell(pick(s), *f)));
                row
            })
            .collect(),
    })
}

/// Per-asset performance of a portfolio run (nothing to show for a single asset).
fn per_asset_table(per_asset: Option<&Value>) -> Option<Table> {
    let rows = per_asset?.as_array()?;
    if rows.len() < 2 {
        return None;
    }
    Some(Table {
        headers: vec![
            "Asset".into(),
            "Trades".into(),
            "Win rate".into(),
            "Net PnL".into(),
            "Fees".into(),
            "Exposure".into(),
        ],
        aligns: vec![Align::Left, Align::Right, Align::Right, Align::Right, Align::Right, Align::Right],
        rows: rows
            .iter()
            .map(|a| {
                vec![
                    Cell::new(a.get("ticker").and_then(Value::as_str).unwrap_or("–")),
                    metric_cell(a, "trades", Fm::Int),
                    // An asset that was never sold has no win rate; 0.0% reads as "it lost".
                    if a.get("trades").and_then(Value::as_i64).unwrap_or(0) > 0 {
                        metric_cell(a, "win_rate", Fm::Pct)
                    } else {
                        Cell::new("–")
                    },
                    metric_cell(a, "net_pnl", Fm::Signed),
                    metric_cell(a, "total_fees", Fm::Num),
                    metric_cell(a, "exposure_pct", Fm::Pct),
                ]
            })
            .collect(),
    })
}

/// Entries the engine refused and bars it sat out: the fine print behind the numbers.
fn execution_notes(doc: &RunDoc) -> Vec<String> {
    let count = |key: &str| doc.extra_get(key).and_then(Value::as_i64).filter(|n| *n > 0);
    let mut out = Vec::new();
    if let Some(n) = count("skipped_min_size") {
        out.push(format!("{n} entries refused: below the instrument's minimum size after lot rounding."));
    }
    if let Some(n) = count("skipped_margin") {
        out.push(format!("{n} entries refused: required margin exceeded available equity."));
    }
    if let Some(n) = count("halted_bars") {
        out.push(format!("{n} bars with new entries halted by a circuit breaker."));
    }
    if let Some(n) = count("filtered_bars") {
        out.push(format!("{n} bars outside the trading window (session, weekday or date filters)."));
    }
    if let Some(f) = doc.extra_get("total_funding").and_then(Value::as_f64).filter(|f| *f != 0.0) {
        out.push(format!("Funding is an estimate at a flat annual rate: {} over the run.", fmt(f, 2)));
    }
    out
}

/// A compact, human-readable settings summary (bullet items, not raw JSON).
fn settings_summary(s: &Value) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let kind = s.get("kind").and_then(Value::as_str).unwrap_or("signals");
    if kind == "dca" {
        parts.push("DCA savings plan".into());
        if let Some(d) = s.get("dca") {
            if let Some(ws) = d.get("weights").and_then(Value::as_array).filter(|w| !w.is_empty()) {
                let total: f64 = ws.iter().filter_map(|w| w.get("weight").and_then(Value::as_f64)).sum();
                let list: Vec<String> = ws
                    .iter()
                    .map(|w| {
                        let t = w.get("ticker").and_then(Value::as_str).unwrap_or("");
                        let x = w.get("weight").and_then(Value::as_f64).unwrap_or(0.0);
                        let pct = if total > 0.0 { x / total * 100.0 } else { 0.0 };
                        format!("{t} {}%", fmt(pct, 1))
                    })
                    .collect();
                parts.push(format!("Weights: {}", list.join(", ")));
            }
            if let Some(c) = d.get("contribution").filter(|v| v.is_object()) {
                let every = c.get("every").and_then(Value::as_u64).unwrap_or(1);
                let period = c.get("period").and_then(Value::as_str).unwrap_or("month");
                let each =
                    if every > 1 { format!("every {every} {period}s") } else { format!("every {period}") };
                parts.push(format!(
                    "Contribution: {} {each}{}",
                    fmt(c.get("amount").and_then(Value::as_f64).unwrap_or(0.0), 2),
                    if c.get("invest").and_then(Value::as_bool) == Some(false) {
                        " (held as cash)"
                    } else {
                        ""
                    }
                ));
            }
            let n = |k: &str| d.get(k).and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
            if n("buys") > 0 || n("sells") > 0 {
                parts.push(format!("{} buy rules, {} sell rules", n("buys"), n("sells")));
            }
        }
    } else if kind == "grid" {
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
        if s.get("reverse_side").and_then(Value::as_bool) == Some(true) {
            parts.push("Short side: mirror of long (inverse)".into());
        }
        if s.get("stop_and_reverse").and_then(Value::as_bool) == Some(true) {
            parts.push("Stop and reverse: on".into());
        }
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
    if let Some(sl) = s.get("slippage") {
        let v = sl.get("value").and_then(Value::as_f64).unwrap_or(0.0);
        if v > 0.0 {
            let k = sl.get("kind").and_then(Value::as_str).unwrap_or("pct");
            parts.push(match k {
                "pct" => format!("Slippage: {}% per fill", fmt(v * 100.0, 3)),
                _ => format!("Slippage: {} ticks per fill", fmt(v, 2)),
            });
        }
    }
    if let Some(i) = s.get("instrument") {
        let mult = i.get("multiplier").and_then(Value::as_f64).unwrap_or(1.0);
        if mult != 1.0 {
            parts.push(format!("Point value: ×{}", fmt(mult, 2)));
        }
    }
    if let Some(rk) = s.get("risk") {
        let mut caps = Vec::new();
        if let Some(v) = rk.get("max_drawdown_pct").and_then(Value::as_f64).filter(|v| *v > 0.0) {
            caps.push(format!("{}% drawdown", fmt(v * 100.0, 2)));
        }
        if let Some(v) = rk.get("max_daily_loss_pct").and_then(Value::as_f64).filter(|v| *v > 0.0) {
            caps.push(format!("{}% daily loss", fmt(v * 100.0, 2)));
        }
        if !caps.is_empty() {
            parts.push(format!("Circuit breaker: {}", caps.join(", ")));
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
    if let Some(f) = s.get("filters") {
        parts.extend(filters_summary(f));
    }
    parts
}

/// One sub-section per traded side: the entry rules, the exit rules and the risk line, which
/// is the part of a report that says what the strategy actually does.
fn sides_sections(s: &Value) -> Vec<Section> {
    let kind = s.get("kind").and_then(Value::as_str).unwrap_or("signals");
    if kind == "grid" {
        return Vec::new();
    }
    if kind == "dca" {
        return dca_sections(s.get("dca"));
    }
    let mode = s.get("mode").and_then(Value::as_str).unwrap_or("long");
    let mut out = Vec::new();
    for (label, key) in [("Long", "long"), ("Short", "short")] {
        if (key == "long" && mode == "short") || (key == "short" && mode == "long") {
            continue;
        }
        // A mirrored short side has no rules of its own: it is the long side, inverted.
        let mirrored = key == "short" && s.get("reverse_side").and_then(Value::as_bool) == Some(true);
        let Some(side) = s.get(if mirrored { "long" } else { key }) else { continue };
        let mut items = Vec::new();
        items.extend(group_bullets(side.get("entry"), "Entry"));
        items.extend(group_bullets(side.get("exit"), "Exit"));
        if let Some(r) = risk_line(side) {
            items.push(r);
        }
        if items.is_empty() {
            continue;
        }
        if mirrored {
            items.push("Rules mirrored from the long side.".into());
        }
        let mut sec = Section::sub(format!("{label} side"));
        sec.blocks.push(Block::Bullets(items));
        out.push(sec);
    }
    out
}

/// The plan, rule by rule: what buys, what sells and on what. The DCA equivalent of the
/// long/short side sections: it is the part of the report that says what the plan does.
fn dca_sections(d: Option<&Value>) -> Vec<Section> {
    let Some(d) = d else { return Vec::new() };
    let mut out = Vec::new();
    let rules = |key: &str, verb: &str| -> Vec<String> {
        let Some(list) = d.get(key).and_then(Value::as_array) else { return Vec::new() };
        let mut items = Vec::new();
        for (i, r) in list.iter().enumerate() {
            let name = r.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty());
            let name = name.map(|n| n.to_string()).unwrap_or_else(|| format!("{verb} {}", i + 1));
            let kind = r.get("amount_kind").and_then(Value::as_str).unwrap_or("");
            let amount = r.get("amount").and_then(Value::as_f64).unwrap_or(0.0);
            let size = match kind {
                "pct_cash" => format!("{}% of cash", fmt(amount, 2)),
                "pct_equity" => format!("{}% of the portfolio", fmt(amount, 2)),
                "pct_invested" => format!("{}% of the cost basis", fmt(amount, 2)),
                "pct_position" => format!("{}% of the position", fmt(amount, 2)),
                "all" => "the whole position".into(),
                "units" => format!("{} units", fmt(amount, 4)),
                _ => fmt(amount, 2),
            };
            let scope = if r.get("per_asset").and_then(Value::as_bool) == Some(true) {
                "per asset"
            } else {
                "on the basket"
            };
            let mut line = format!("**{name}:** {size}, {scope}");
            if let Some(t) = r.get("target_gain_pct").and_then(Value::as_f64).filter(|v| *v > 0.0) {
                line.push_str(&format!(", once up {}%", fmt(t, 2)));
            }
            items.push(line);
            items.extend(group_bullets(r.get("condition"), "When"));
            if let Some(n) = r.get("max_fires").and_then(Value::as_u64).filter(|n| *n > 0) {
                items.push(format!("At most {n} times."));
            }
            if let Some(n) = r.get("cooldown_bars").and_then(Value::as_u64).filter(|n| *n > 0) {
                items.push(format!("Then waits {n} bars."));
            }
            if r.get("withdraw").and_then(Value::as_bool) == Some(true) {
                items.push("Proceeds leave the portfolio.".into());
            }
        }
        items
    };
    for (title, key, verb) in [("Buy rules", "buys", "buy"), ("Sell rules", "sells", "sell")] {
        let items = rules(key, verb);
        if items.is_empty() {
            continue;
        }
        let mut sec = Section::sub(title);
        sec.blocks.push(Block::Bullets(items));
        out.push(sec);
    }
    out
}

/// "**Entry:** EMA(20) crosses above close" then one bullet per extra condition, each opened
/// by the group's own joiner so the logic reads without a legend.
fn group_bullets(g: Option<&Value>, label: &str) -> Vec<String> {
    let Some(g) = g else { return Vec::new() };
    let conds = g.get("conditions").and_then(Value::as_array).cloned().unwrap_or_default();
    if conds.is_empty() {
        return Vec::new();
    }
    let joiner = if g.get("logic").and_then(Value::as_str) == Some("any") { "or" } else { "and" };
    let mut out = vec![format!("**{label}:** {}", condition_text(&conds[0]))];
    for c in &conds[1..] {
        out.push(format!("**{joiner}** {}", condition_text(c)));
    }
    out
}

/// SL / TP / reverse-exit as one bullet, or nothing when the side sets none.
fn risk_line(side: &Value) -> Option<String> {
    let mut bits = Vec::new();
    if let Some(s) = stop_text(side, "stop_loss", "stop_loss_pct") {
        bits.push(format!("SL {s}"));
    }
    if let Some(s) = stop_text(side, "take_profit", "take_profit_pct") {
        bits.push(format!("TP {s}"));
    }
    if side.get("exit_on_reverse").and_then(Value::as_bool) == Some(true) {
        bits.push("exit on reverse signal".into());
    }
    (!bits.is_empty()).then(|| format!("**Risk:** {}", bits.join(" · ")))
}

/// A stop rule in its v2 object form, falling back to the legacy percent field.
fn stop_text(side: &Value, obj_key: &str, pct_key: &str) -> Option<String> {
    if let Some(o) = side.get(obj_key).filter(|v| v.is_object()) {
        let v = o.get("value").and_then(Value::as_f64).unwrap_or(0.0);
        if v > 0.0 {
            return Some(match o.get("kind").and_then(Value::as_str) {
                Some("atr") => {
                    let p = o.get("period").and_then(Value::as_i64).unwrap_or(0);
                    format!("{}×ATR({p})", fmt(v, 2))
                }
                _ => format!("{}%", fmt(v * 100.0, 2)),
            });
        }
    }
    side.get(pct_key)
        .and_then(Value::as_f64)
        .filter(|v| *v > 0.0)
        .map(|v| format!("{}%", fmt(v * 100.0, 2)))
}

/// "EMA(20) crosses above close": the engine's `Signal` in words.
fn condition_text(c: &Value) -> String {
    let op = c.get("op").and_then(Value::as_str).unwrap_or("");
    let (label, binary) = match op {
        "crosses_above" => ("crosses above", true),
        "crosses_below" => ("crosses below", true),
        "cross" => ("crosses (either)", true),
        "above" => ("is above", true),
        "below" => ("is below", true),
        "rising" => ("is rising", false),
        "falling" => ("is falling", false),
        "closing_above" => ("closes above", true),
        "closing_below" => ("closes below", true),
        "opening_above" => ("opens above", true),
        "opening_below" => ("opens below", true),
        other => (other, true),
    };
    let left = operand_text(c.get("left"));
    match (binary, c.get("right").filter(|v| !v.is_null())) {
        (true, Some(r)) => format!("{left} {label} {}", operand_text(Some(r))),
        _ => format!("{left} {label}"),
    }
}

/// "EMA(20)", "close", "70": one operand in words.
fn operand_text(o: Option<&Value>) -> String {
    let Some(o) = o else { return String::new() };
    match o.get("kind").and_then(Value::as_str).unwrap_or("") {
        "price" => o.get("field").and_then(Value::as_str).unwrap_or("close").into(),
        "const" => fmt(o.get("value").and_then(Value::as_f64).unwrap_or(0.0), 2),
        "custom_indicator" => {
            let id = o.get("id").and_then(Value::as_str).unwrap_or("");
            format!("custom({id})")
        }
        "metric" => {
            let m = o.get("metric").and_then(Value::as_str).unwrap_or("");
            let p = o.get("period").and_then(Value::as_u64).unwrap_or(0);
            let name = match m {
                "dd_from_high" => "drawdown from high %".to_string(),
                "up_from_low" => "rise from low %".to_string(),
                "change_from_start" => "change since start %".to_string(),
                "change_pct" => format!("change over {p} bars %"),
                other => other.to_string(),
            };
            name
        }
        "position" => match o.get("field").and_then(Value::as_str).unwrap_or("") {
            "pnl_pct" => "position P&L %".into(),
            "since_last_buy_pct" => "change since last buy %".into(),
            "avg_cost" => "average cost".into(),
            "units" => "units held".into(),
            "value" => "position value".into(),
            "weight_pct" => "portfolio weight %".into(),
            "cash_pct" => "cash %".into(),
            "drawdown_pct" => "portfolio drawdown %".into(),
            other => other.to_string(),
        },
        _ => {
            let name = o.get("indicator").and_then(Value::as_str).unwrap_or("?").to_uppercase();
            // Params in the engine's own order; a 0/absent one is unused by that indicator.
            let ps: Vec<String> = ["period", "fast", "slow", "signal_period", "mult"]
                .iter()
                .filter_map(|k| o.get(*k).and_then(Value::as_f64).filter(|v| *v > 0.0))
                .map(|v| fmt(v, if v.fract() == 0.0 { 0 } else { 2 }))
                .collect();
            if ps.is_empty() {
                name
            } else {
                format!("{name}({})", ps.join(","))
            }
        }
    }
}

/// Trading-window filters, one bullet each and nothing when they are inert.
fn filters_summary(f: &Value) -> Vec<String> {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let list = |k: &str| f.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    let off = f.get("tz_offset_min").and_then(Value::as_i64).unwrap_or(0);
    let tz = {
        let (sign, a) = if off < 0 { ('-', -off) } else { ('+', off) };
        format!("UTC{sign}{:02}:{:02}", a / 60, a % 60)
    };
    let mut parts = Vec::new();
    let days = list("weekdays");
    if !days.is_empty() {
        let names: Vec<&str> = days
            .iter()
            .filter_map(Value::as_u64)
            .filter_map(|d| DAYS.get((d as usize).saturating_sub(1)).copied())
            .collect();
        parts.push(format!("Days: {}", names.join(", ")));
    }
    let sessions = list("sessions");
    if !sessions.is_empty() {
        let w: Vec<String> = sessions
            .iter()
            .map(|s| {
                let g = |k: &str| s.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                format!("{}-{}", g("from"), g("to"))
            })
            .collect();
        parts.push(format!("Session: {} ({tz})", w.join(", ")));
    }
    let dates = |k: &str| -> String {
        list(k)
            .iter()
            .map(|d| {
                let from = d.get("from").and_then(Value::as_str).unwrap_or("");
                match d.get("to").and_then(Value::as_str) {
                    Some(to) if !to.is_empty() && to != from => format!("{from} → {to}"),
                    _ => from.to_string(),
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let only = dates("include_dates");
    if !only.is_empty() {
        parts.push(format!("Only on: {only}"));
    }
    let never = dates("exclude_dates");
    if !never.is_empty() {
        parts.push(format!("Never on: {never}"));
    }
    if !parts.is_empty() && f.get("on_window_end").and_then(Value::as_str) == Some("flat") {
        parts.push("Flat outside the window".into());
    }
    parts
}

// ── small helpers ──
fn esc(s: &str) -> String {
    s.replace('"', "'")
}
/// Green above zero, red below, plain at zero: the sign of a money figure.
fn tone(v: f64) -> Tone {
    if v > 0.0 {
        Tone::Positive
    } else if v < 0.0 {
        Tone::Negative
    } else {
        Tone::Neutral
    }
}
fn opt_num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}
fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64)
}
fn num(v: &Value, key: &str) -> String {
    fmt(opt_num(v, key).unwrap_or(0.0), 2)
}
fn sign(v: f64) -> &'static str {
    if v >= 0.0 {
        "+"
    } else {
        ""
    }
}
/// The date half of an RFC3339 timestamp.
fn day(ts: &str) -> String {
    ts.split('T').next().unwrap_or(ts).to_string()
}
/// "take_profit" → "Take profit".
fn pretty_key(k: &str) -> String {
    let s = k.replace('_', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
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

    fn settings() -> Value {
        json!({
            "mode": "long",
            "sizing": { "mode": "percent_equity", "percent": 100 },
            "starting_capital": 10000,
            "fees": { "amount_kind": "pct", "per": "trade", "amount": 0.1 },
            "long": {
                "entry": { "logic": "all", "conditions": [
                    { "left": { "kind": "indicator", "indicator": "ema", "period": 20 },
                      "op": "crosses_above",
                      "right": { "kind": "price", "field": "close" } },
                    { "left": { "kind": "indicator", "indicator": "rsi", "period": 14 },
                      "op": "below",
                      "right": { "kind": "const", "value": 70 } }
                ]},
                "exit": { "logic": "all", "conditions": [] },
                "stop_loss_pct": 0.02,
                "take_profit_pct": 0.04,
                "exit_on_reverse": true
            }
        })
    }

    fn side_block(trades: i64, net: f64) -> Value {
        json!({
            "trades": trades, "wins": 7, "losses": 5, "win_rate": 58.3, "net_pnl": net,
            "gross_profit": 2000.0, "gross_loss": 800.0, "profit_factor": 2.5, "total_fees": 30.0,
            "avg_trade": 100.0, "avg_win": 285.0, "avg_loss": 160.0, "payoff_ratio": 1.78,
            "largest_win": 600.0, "largest_loss": 240.0, "max_consec_wins": 3,
            "max_consec_losses": 2, "avg_bars_held": 14.5, "expectancy_pct": 0.5
        })
    }

    fn stats() -> Value {
        json!({
            "engine_version": 2, "trades": 12, "wins": 7, "losses": 5, "net_pnl": 1234.5,
            "return_pct": 12.34, "win_rate": 58.3, "profit_factor": 1.7, "max_drawdown_pct": 8.2,
            "max_drawdown": 900.0, "avg_trade": 102.9, "final_equity": 11234.5,
            "buy_hold_return_pct": 9.9, "expectancy_pct": 0.5, "sharpe": 1.4, "sortino": 2.1,
            "total_fees": 30.0,
            "exit_reasons": { "take_profit": 7, "stop_loss": 5 },
            "all": side_block(12, 1234.5), "long": side_block(12, 1234.5), "short": side_block(0, 0.0)
        })
    }

    fn extra() -> Value {
        json!({
            "bars": 4320,
            "first_ts": "2024-01-01T00:00:00Z",
            "last_ts": "2024-06-30T23:00:00Z",
            "warmup_bars": 20,
            "trading_start_ts": "2024-01-01T20:00:00Z",
            "equity": [
                { "ts": "2024-01-01T00:00:00Z", "equity": 10000.0 },
                { "ts": "2024-03-01T00:00:00Z", "equity": 10600.0 },
                { "ts": "2024-06-30T23:00:00Z", "equity": 11234.5 }
            ],
            "per_asset": [
                { "ticker": "BTCUSDT", "trades": 8, "win_rate": 62.5, "net_pnl": 900.0,
                  "total_fees": 20.0, "exposure_pct": 41.0 },
                { "ticker": "ETHUSDT", "trades": 4, "win_rate": 50.0, "net_pnl": 334.5,
                  "total_fees": 10.0, "exposure_pct": 22.0 }
            ],
            "oos": { "split_pct": 0.7, "split_ts": "2024-05-01T00:00:00Z",
                     "in_sample": side_block(9, 1000.0), "out_sample": side_block(3, 234.5) },
            "trades": [
                { "ticker": "BTCUSDT", "direction": "long", "pnl": 600.0, "return_pct": 3.2,
                  "mae": 120.0, "mfe": 700.0, "bars_held": 18 },
                { "ticker": "BTCUSDT", "direction": "long", "pnl": -240.0, "return_pct": -1.4,
                  "mae": 300.0, "mfe": 40.0, "bars_held": 9 },
                { "ticker": "BTCUSDT", "direction": "short", "pnl": 540.0, "return_pct": 2.6,
                  "mae": 80.0, "mfe": 610.0, "bars_held": 22 },
                { "ticker": "ETHUSDT", "direction": "long", "pnl": 410.0, "return_pct": 2.1,
                  "mae": 95.0, "mfe": 480.0, "bars_held": 14 },
                { "ticker": "ETHUSDT", "direction": "short", "pnl": -75.5, "return_pct": -0.4,
                  "mae": 110.0, "mfe": 25.0, "bars_held": 6 }
            ],
            "skipped_margin": 3,
            "filtered_bars": 120,
            "total_funding": -42.0
        })
    }

    #[test]
    fn snapshot_only_report_keeps_working() {
        let (s, st) = (settings(), stats());
        let md = run_report_md(&RunDoc::new("My run", "BTCUSDT", "1h", &s, &st));
        assert!(md.starts_with("---\n"), "front matter");
        assert!(md.contains("# Backtest report — My run"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("## Strategy"));
        assert!(md.contains("### Long side"));
        assert!(md.contains("EMA(20) crosses above close"));
        assert!(md.contains("**and** RSI(14) is below 70.00"));
        assert!(md.contains("SL 2.00% · TP 4.00% · exit on reverse signal"));
        assert!(md.contains("## Performance"));
        assert!(md.contains("| Payoff ratio |"));
        assert!(md.contains("## Exit reasons"));
        assert!(md.contains("| Take profit | 7 |"));
        // No replay: no curve, no per-asset, no execution notes.
        assert!(!md.contains("## Equity curve"));
        assert!(!md.contains("## Per-asset breakdown"));
        assert!(!md.contains("## Execution notes"));
        assert!(!md.contains("## Return distribution"), "no trades, no distribution");
        // Never a trade list.
        assert!(!md.contains("## Trades"));
    }

    #[test]
    fn replayed_report_adds_curve_breakdowns_and_notes() {
        let (s, st, ex) = (settings(), stats(), extra());
        let doc = RunDoc::new("My run", "BTCUSDT", "1h", &s, &st).with_extra(&ex);
        let md = run_report_md(&doc);
        assert!(md.contains("bars: 4320"), "bar count in front matter");
        assert!(md.contains("2024-01-01 → 2024-06-30"));
        assert!(md.contains("20-bar warm-up"));
        assert!(md.contains("## Equity curve"));
        assert!(md.contains("## Out-of-sample"));
        assert!(md.contains("## Per-asset breakdown"));
        assert!(md.contains("| BTCUSDT |"));
        assert!(md.contains("## Execution notes"));
        assert!(md.contains("3 entries refused"));
        // Trade-derived statistics: the excursion rows, the distribution, the per-asset detail.
        assert!(md.contains("| Avg run-up |"));
        assert!(md.contains("| Avg R (result / heat) |"));
        assert!(md.contains("## Return distribution"));
        assert!(md.contains("### Per-asset statistics"));
        assert!(md.contains("| Metric | BTCUSDT | ETHUSDT |"));
        assert!(md.contains("120 bars outside the trading window"));

        // Same document renders to PDF through the shared engine.
        let pdf = crate::report::pdf::render(&run_report(&doc));
        assert!(pdf.starts_with(b"%PDF-1.4"));
    }

    #[test]
    fn replay_stats_win_over_the_snapshot() {
        let (s, st) = (settings(), stats());
        let fresher = json!({ "stats": { "net_pnl": 99.0, "trades": 1 } });
        let md = run_report_md(&RunDoc::new("R", "BTCUSDT", "1h", &s, &st).with_extra(&fresher));
        assert!(md.contains("| Net PnL | 99.00 |"));
    }
}
