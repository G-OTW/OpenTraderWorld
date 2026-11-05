//! Builds the Trading Journal periodic (weekly/monthly) report document.
//!
//! Pure mapping from `otw_store::journal::ReportData` onto the shared report
//! engine (`crate::report`) — the same document renders to Markdown or PDF.

use otw_store::journal::ReportData;

use crate::report::{fmt_num, Align, Block, Cell, Chart, Report, Section, Stat, Table, Tone};

/// Context lines that frame the numbers (period, scope, active filters).
pub struct ReportContext {
    /// "Weekly trading report" / "Monthly trading report".
    pub title: String,
    /// Human period ("Jul 13 – Jul 19, 2026 (week 29)").
    pub period_label: String,
    /// Machine period slug ("2026-W29", "2026-07") for the front matter.
    pub period_slug: String,
    /// "All categories" or the category's name.
    pub scope: String,
    /// Extra active filters as "label: value" pairs (may be empty).
    pub filters: Vec<(String, String)>,
    /// RFC3339 generation timestamp.
    pub generated_at: String,
}

/// Trade rows above this count are summarized (keeps the PDF a sane length).
const MAX_TRADE_ROWS: usize = 120;

pub fn build(data: &ReportData, ctx: &ReportContext) -> Report {
    let b = &data.breakdown;
    let ccy = b.display_currency.as_str();
    let money = |v: f64| format!("{} {ccy}", fmt_num(v, 2));
    let opt_money = |v: Option<f64>| v.map(money).unwrap_or_else(|| "–".into());
    let opt_pct = |v: Option<f64>| v.map(|x| format!("{}%", fmt_num(x * 100.0, 1))).unwrap_or_else(|| "–".into());

    let mut r = Report::new(&ctx.title);
    r.subtitle = Some(format!("{} · {}", ctx.period_label, ctx.scope));
    r.meta.push(("period".into(), ctx.period_slug.clone()));
    r.meta.push(("scope".into(), ctx.scope.clone()));
    r.meta.push(("currency".into(), ccy.to_string()));
    for (k, v) in &ctx.filters {
        r.meta.push((k.clone(), v.clone()));
    }
    r.meta.push(("generated".into(), ctx.generated_at.clone()));

    // ── Overview ──
    let mut overview = Section::new("Overview");
    let mut cards: Vec<Stat> = Vec::new();
    let mut net = Stat::new("Net PnL", money(b.realized_pnl)).signed(b.realized_pnl);
    if let Some(rp) = b.return_pct {
        net = net.hint(format!("{}% of capital", fmt_num(rp, 2)));
    }
    cards.push(net);
    cards.push(
        Stat::new("Win rate", opt_pct(b.win_rate)).hint(format!("{}W / {}L", b.win_count, b.loss_count)),
    );
    cards.push(
        Stat::new("Trades", b.trade_count.to_string())
            .hint(format!("{} closed · {} open", b.closed_count, b.open_count)),
    );
    cards.push(match b.expectancy {
        Some(e) => Stat::new("Expectancy / trade", money(e)).signed(e),
        None => Stat::new("Expectancy / trade", "–"),
    });
    cards.push(Stat::new(
        "Profit factor",
        b.profit_factor.map(|x| fmt_num(x, 2)).unwrap_or_else(|| "–".into()),
    ));
    cards.push(Stat::new("Total fees", money(b.total_fees)).toned(Tone::Muted));
    cards.push(Stat::new("Avg win", opt_money(b.avg_win)).toned(Tone::Positive));
    cards.push(Stat::new("Avg loss", opt_money(b.avg_loss)).toned(Tone::Negative));
    cards.push(Stat::new(
        "Max drawdown",
        b.max_drawdown.map(|x| format!("{}%", fmt_num(x, 2))).unwrap_or_else(|| "–".into()),
    ));
    cards.push(match b.best_trade {
        Some(v) => Stat::new("Best trade", money(v)).signed(v),
        None => Stat::new("Best trade", "–"),
    });
    cards.push(match b.worst_trade {
        Some(v) => Stat::new("Worst trade", money(v)).signed(v),
        None => Stat::new("Worst trade", "–"),
    });
    cards.push(Stat::new("Invested capital", money(b.invested_capital)).toned(Tone::Muted));
    overview.blocks.push(Block::Stats(cards));
    r.sections.push(overview);

    // ── Equity curve (cumulative realized PnL within the filtered range) ──
    let mut curve = Section::new("Equity curve");
    if b.equity_curve.len() >= 2 {
        curve.blocks.push(Block::Chart(Chart {
            y_label: format!("Cumulative net PnL ({ccy})"),
            points: b
                .equity_curve
                .iter()
                .map(|p| (p.at.unix_timestamp() as f64, p.cum_pnl))
                .collect(),
            baseline: Some(0.0),
            time_axis: true,
        }));
    } else {
        curve.blocks.push(Block::Paragraph(
            "Fewer than two closed trades in this period — no curve to draw.".into(),
        ));
    }
    r.sections.push(curve);

    // ── Group tables ──
    let group_table = |groups: &[otw_store::journal::GroupStat], label: &str| -> Table {
        Table {
            headers: vec![
                label.into(),
                "Trades".into(),
                "Closed".into(),
                "Win rate".into(),
                format!("Net PnL ({ccy})"),
                format!("Fees ({ccy})"),
                format!("Expectancy ({ccy})"),
            ],
            aligns: vec![
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
            ],
            rows: groups
                .iter()
                .map(|g| {
                    vec![
                        Cell::new(g.name.clone()),
                        Cell::new(g.trades.to_string()),
                        Cell::new(g.closed.to_string()),
                        Cell::new(opt_pct(g.win_rate())),
                        Cell::signed(fmt_num(g.net, 2), g.net),
                        Cell::toned(fmt_num(g.fees, 2), Tone::Muted),
                        match g.expectancy() {
                            Some(e) => Cell::signed(fmt_num(e, 2), e),
                            None => Cell::new("–"),
                        },
                    ]
                })
                .collect(),
        }
    };

    if !data.by_strategy.is_empty() {
        let mut s = Section::new("By strategy");
        s.blocks.push(Block::Table(group_table(&data.by_strategy, "Strategy")));
        r.sections.push(s);
    }
    if data.by_category.len() > 1 {
        let mut s = Section::new("By category");
        s.blocks.push(Block::Table(group_table(&data.by_category, "Category")));
        r.sections.push(s);
    }

    // ── Trade log ──
    let mut log = Section::new("Trades");
    if data.trades.is_empty() {
        log.blocks.push(Block::Paragraph("No trades in this period.".into()));
    } else {
        if data.trades.len() > MAX_TRADE_ROWS {
            log.blocks.push(Block::Paragraph(format!(
                "Showing the first {MAX_TRADE_ROWS} of {} trades.",
                data.trades.len()
            )));
        }
        let date_fmt = time::macros::format_description!("[year]-[month]-[day]");
        log.blocks.push(Block::Table(Table {
            headers: vec![
                "Date".into(),
                "Ticker".into(),
                "Side".into(),
                "Qty".into(),
                "Entry".into(),
                "Exit".into(),
                format!("Fees ({ccy})"),
                format!("Net PnL ({ccy})"),
                "Strategy".into(),
            ],
            aligns: vec![
                Align::Left,
                Align::Left,
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Right,
                Align::Left,
            ],
            rows: data
                .trades
                .iter()
                .take(MAX_TRADE_ROWS)
                .map(|t| {
                    let date = t
                        .date
                        .and_then(|d| d.format(date_fmt).ok())
                        .unwrap_or_else(|| "–".into());
                    let net = if t.open {
                        Cell::toned("open", Tone::Muted)
                    } else {
                        match t.net {
                            Some(n) => Cell::signed(fmt_num(n, 2), n),
                            None => Cell::toned("no FX rate", Tone::Muted),
                        }
                    };
                    vec![
                        Cell::new(date),
                        Cell::new(t.ticker.clone()),
                        Cell::new(t.side.clone()),
                        Cell::new(fmt_num(t.qty, 4)),
                        Cell::new(t.avg_entry.map(|p| fmt_num(p, 4)).unwrap_or_else(|| "–".into())),
                        Cell::new(t.avg_exit.map(|p| fmt_num(p, 4)).unwrap_or_else(|| "–".into())),
                        Cell::toned(t.fees.map(|f| fmt_num(f, 2)).unwrap_or_else(|| "–".into()), Tone::Muted),
                        net,
                        Cell::new(t.strategy.clone().unwrap_or_else(|| "–".into())),
                    ]
                })
                .collect(),
        }));
    }
    r.sections.push(log);

    let mut note = format!(
        "Generated by OpenTraderWorld on {} · amounts converted to {ccy} at each trade's effective-date FX rate.",
        ctx.generated_at
    );
    if b.unconverted_trades > 0 {
        note.push_str(&format!(
            " {} closed trade(s) are excluded from the totals because their FX rate is missing (see Journal → Pending tasks).",
            b.unconverted_trades
        ));
    }
    r.footer_note = Some(note);
    r
}
