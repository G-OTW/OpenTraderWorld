//! The researchers. Each reads one [`TradeSet`] and returns the statements that clear the
//! sample floor and the effect threshold; most return nothing on most books, which is the
//! point. Money effects go through the lab's own pure scenario functions.

use std::collections::BTreeMap;

use otw_store::journal_behavior::{escalation, ESCALATION_PCT};

use super::{compare, enough, median, quantile, Context, Insight, SourceKind, TradeSet, MIN_BUCKET, MIN_SAMPLE, MIN_SIDE};
use crate::journal_exits::SignalSpec;
use crate::journal_whatif::{
    self as whatif, ExitsParams, FamilyMove, FamilySide, FeesParams, FilterParams, LabTrade, Outcome, Scenario,
    Sim, TimeWindow, CARD_SIZE_FACTOR,
};

// ── Entry hour and weekday ───────────────────────────────────────────────────

/// Run a time-window filter exactly as the lab does: a trade without an entry time is
/// unmeasured, the others are kept or removed by the window.
pub fn run_window(set: &TradeSet, window: &TimeWindow) -> Outcome {
    let lab = set.lab();
    let mask: Vec<Result<bool, &'static str>> =
        lab.iter().map(|t| whatif::window_keeps(t, window).ok_or("undated")).collect();
    let (trades, keep, unmeasured) = whatif::split_measured(&lab, &set.unmeasured, mask);
    let (steps, detail) = whatif::filter(&trades, &keep);
    let scenario = Scenario::Filter(FilterParams { signal: None, window: Some(window.clone()) });
    whatif::assemble(&trades, steps, detail, scenario, unmeasured, set.currency.as_deref().unwrap_or(""))
}

struct Bucketing {
    researcher: &'static str,
    /// The value name the statement uses for the bucket (`hour`, `weekday`).
    name: &'static str,
    key: fn(&LabTrade) -> Option<u8>,
    window: fn(u8, bool) -> TimeWindow,
}

fn by_bucket(set: &TradeSet, b: &Bucketing) -> Vec<Insight> {
    if !enough(set) {
        return Vec::new();
    }
    let lab = set.lab();
    let mut buckets: BTreeMap<u8, (usize, f64)> = BTreeMap::new();
    for t in &lab {
        if let Some(k) = (b.key)(t) {
            let e = buckets.entry(k).or_default();
            e.0 += 1;
            e.1 += t.net;
        }
    }
    if buckets.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();

    // The worst bucket, dropped: the lab's filter scenario, run here as the lab runs it.
    let worst = buckets
        .iter()
        .filter(|(_, (n, net))| *n >= MIN_BUCKET && *net < 0.0)
        .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1));
    if let Some((k, (n, net))) = worst {
        let window = (b.window)(*k, true);
        let o = run_window(set, &window);
        if o.before.n >= MIN_SAMPLE && o.delta_net > 0.0 && set.material(o.delta_net) {
            let mut i = Insight::new(b.researcher, &format!("{}_worst", b.name), "warn")
                .value(b.name, *k as f64)
                .value("trades", *n as f64)
                .value("net", *net);
            i.evidence.n = o.before.n;
            i.evidence.delta_pnl = Some(o.delta_net);
            i.evidence.delta_dd = Some(o.delta_drawdown);
            i.apply = Some(o.scenario);
            i.strength = set.share(o.delta_net);
            out.push(i);
        }
    }

    // The best bucket, sized up: every trade of that bucket at the card's size factor.
    let total: f64 = lab.iter().map(|t| t.net).sum();
    let avg = total / lab.len() as f64;
    let best = buckets
        .iter()
        .filter(|(_, (n, net))| *n >= MIN_BUCKET && *net > 0.0 && net / *n as f64 > avg)
        .max_by(|a, b| a.1 .1.total_cmp(&b.1 .1));
    if let Some((k, (n, net))) = best {
        let (before, after) = compare(&lab, |_, t| {
            let inside = (b.key)(t) == Some(*k);
            let f = if inside { CARD_SIZE_FACTOR } else { 1.0 };
            Some(Sim { net: t.net * f, fees: t.fees * f })
        });
        let delta = after.net - before.net;
        if set.material(delta) {
            let mut i = Insight::new(b.researcher, &format!("{}_best", b.name), "good")
                .value(b.name, *k as f64)
                .value("trades", *n as f64)
                .value("net", *net)
                .value("factor", CARD_SIZE_FACTOR);
            i.evidence.n = before.n;
            i.evidence.delta_pnl = Some(delta);
            i.evidence.delta_dd = Some(after.max_drawdown - before.max_drawdown);
            i.strength = set.share(delta);
            out.push(i);
        }
    }
    out
}

/// Best and worst entry hours: what dropping the worst one would have done (the lab's
/// filter scenario), and what sizing up the best one would have done.
pub fn hours(set: &TradeSet, _: &Context) -> Vec<Insight> {
    by_bucket(
        set,
        &Bucketing {
            researcher: "hours",
            name: "hour",
            key: |t| t.entry_local.map(|e| e.hour()),
            window: |k, exclude| TimeWindow { hours: vec![k], weekdays: Vec::new(), exclude },
        },
    )
}

/// Same as [`hours`], by entry weekday (1 = Monday).
pub fn weekday(set: &TradeSet, _: &Context) -> Vec<Insight> {
    by_bucket(
        set,
        &Bucketing {
            researcher: "weekday",
            name: "weekday",
            key: |t| t.entry_local.map(|e| e.weekday().number_from_monday()),
            window: |k, exclude| TimeWindow { hours: Vec::new(), weekdays: vec![k], exclude },
        },
    )
}

// ── Holding time ─────────────────────────────────────────────────────────────

/// Ratio between the two medians from which the asymmetry is stated.
pub const HOLD_RATIO: f64 = 1.5;

/// Winners cut short while losers are held: the median hold of the losers against the
/// winners'. No scenario: a time stop is counted in candles, and the candle grain of a
/// book is not something to guess.
pub fn hold_time(set: &TradeSet, _: &Context) -> Vec<Insight> {
    let held: Vec<(f64, f64)> = set
        .trades
        .iter()
        .filter_map(|t| {
            let e = t.lab.entry_local?;
            let m = (t.lab.exit_local - e).whole_minutes() as f64;
            (m >= 0.0).then_some((t.lab.net, m))
        })
        .collect();
    if held.len() < MIN_SAMPLE {
        return Vec::new();
    }
    let wins: Vec<f64> = held.iter().filter(|(n, _)| *n > 0.0).map(|(_, m)| *m).collect();
    let losses: Vec<f64> = held.iter().filter(|(n, _)| *n < 0.0).map(|(_, m)| *m).collect();
    if wins.len() < MIN_SIDE || losses.len() < MIN_SIDE {
        return Vec::new();
    }
    let (Some(w), Some(l)) = (median(&wins), median(&losses)) else { return Vec::new() };
    let found = if w > 0.0 && l >= HOLD_RATIO * w {
        Some(("hold_losers_longer", "warn", l / w))
    } else if l > 0.0 && w >= HOLD_RATIO * l {
        Some(("hold_winners_longer", "good", w / l))
    } else {
        None
    };
    let Some((key, severity, ratio)) = found else { return Vec::new() };
    let mut i = Insight::new("hold_time", key, severity)
        .value("winners", w)
        .value("losers", l)
        .value("ratio", ratio)
        .value("n", held.len() as f64);
    i.evidence.n = held.len();
    i.strength = (ratio - 1.0).min(4.0) / 100.0;
    vec![i]
}

// ── Stop geometry ────────────────────────────────────────────────────────────

/// A winner whose adverse move reached this share of its own stop came close to it.
pub const NEAR_STOP: f64 = 0.8;
/// Share of the winners that came close to their stop from which the stops are tight.
pub const TIGHT_SHARE: f64 = 0.25;
/// The winners' 90th percentile adverse move under this share of the median stop = wide.
pub const WIDE_SHARE: f64 = 0.5;
/// The suggested stop sits this far beyond the winners' 90th percentile adverse move.
pub const STOP_MARGIN: f64 = 1.25;

fn round_to(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

/// Stops against the winners' adverse excursion: too tight when a quarter of the winners
/// came within a fifth of their stop, wide when nine winners in ten never used half of it.
/// The scenario is the exit lab's percent stop just past the winners' 90th percentile.
pub fn stop_geometry(set: &TradeSet, _: &Context) -> Vec<Insight> {
    let pts: Vec<(f64, f64, f64)> = set
        .trades
        .iter()
        .filter_map(|t| Some((t.lab.net, t.stop_pct.filter(|s| *s > 0.0)?, t.lab.mae_pct?)))
        .collect();
    if pts.len() < MIN_SAMPLE {
        return Vec::new();
    }
    let winners: Vec<&(f64, f64, f64)> = pts.iter().filter(|(n, _, _)| *n > 0.0).collect();
    if winners.len() < MIN_SIDE {
        return Vec::new();
    }
    let stops: Vec<f64> = pts.iter().map(|(_, s, _)| *s).collect();
    let maes: Vec<f64> = winners.iter().map(|(_, _, m)| *m).collect();
    let (Some(stop), Some(p90)) = (median(&stops), quantile(&maes, 0.9)) else { return Vec::new() };
    let near = winners.iter().filter(|(_, s, m)| *m >= NEAR_STOP * s).count() as f64 / winners.len() as f64;
    let suggested = round_to(p90 * STOP_MARGIN, 0.1).clamp(0.1, 50.0);
    let (key, severity) = if near >= TIGHT_SHARE && suggested > stop {
        ("stop_tight", "warn")
    } else if p90 <= WIDE_SHARE * stop && suggested < stop {
        ("stop_wide", "info")
    } else {
        return Vec::new();
    };
    let risk = (suggested / stop - 1.0) * 100.0;
    let mut i = Insight::new("stop_geometry", key, severity)
        .value("stop", stop)
        .value("mae", p90)
        .value("share", near * 100.0)
        .value("pct", suggested)
        .value("n", pts.len() as f64);
    i.evidence.n = pts.len();
    i.evidence.delta_risk = Some(risk);
    i.apply = Some(Scenario::Exits(ExitsParams {
        signals: Some(vec![SignalSpec {
            id: "stop_geometry".into(),
            rule: "sl_pct".into(),
            params: [("pct".to_string(), suggested)].into(),
        }]),
        combo: Some(false),
        ..Default::default()
    }));
    i.strength = risk.abs().min(100.0) / 1000.0;
    vec![i]
}

// ── Concentration ────────────────────────────────────────────────────────────

/// Share of the trades one ticker, class or sector must carry to be called concentrated.
pub const CONCENTRATED_PCT: f64 = 60.0;

struct Group {
    trades: usize,
    net: f64,
}

fn groups(lab: &[LabTrade], key: impl Fn(&LabTrade) -> Option<String>) -> BTreeMap<String, Group> {
    let mut m: BTreeMap<String, Group> = BTreeMap::new();
    for t in lab {
        if let Some(k) = key(t) {
            let g = m.entry(k).or_insert(Group { trades: 0, net: 0.0 });
            g.trades += 1;
            g.net += t.net;
        }
    }
    m
}

/// The group that loses, removed: what the set would have made without it.
fn drag(
    set: &TradeSet,
    lab: &[LabTrade],
    by: &BTreeMap<String, Group>,
    key: &impl Fn(&LabTrade) -> Option<String>,
    insight_key: &str,
) -> Option<Insight> {
    if by.len() < 2 {
        return None;
    }
    let (name, g) = by
        .iter()
        .filter(|(_, g)| g.trades >= MIN_SIDE && g.net < 0.0)
        .min_by(|a, b| a.1.net.total_cmp(&b.1.net))?;
    let (before, after) = compare(lab, |_, t| {
        (key(t).as_deref() != Some(name.as_str())).then_some(Sim { net: t.net, fees: t.fees })
    });
    let delta = after.net - before.net;
    if !set.material(delta) || delta <= 0.0 {
        return None;
    }
    let mut i = Insight::new("concentration", insight_key, "warn")
        .label(name.clone())
        .value("trades", g.trades as f64)
        .value("net", g.net);
    i.evidence.n = before.n;
    i.evidence.delta_pnl = Some(delta);
    i.evidence.delta_dd = Some(after.max_drawdown - before.max_drawdown);
    i.strength = set.share(delta);
    Some(i)
}

/// The group that carries most of the trades.
fn share(by: &BTreeMap<String, Group>, covered: usize, insight_key: &str) -> Option<Insight> {
    if by.len() < 2 || covered == 0 {
        return None;
    }
    let (name, g) = by.iter().max_by_key(|(_, g)| g.trades)?;
    let pct = g.trades as f64 / covered as f64 * 100.0;
    if pct < CONCENTRATED_PCT {
        return None;
    }
    let mut i = Insight::new("concentration", insight_key, "info")
        .label(name.clone())
        .value("share", pct)
        .value("trades", g.trades as f64)
        .value("groups", by.len() as f64);
    i.evidence.n = covered;
    i.strength = pct / 100.0 / 100.0;
    Some(i)
}

/// Where the trades and the losses sit: by ticker and asset class, and by sector only for
/// the tickers the fundamentals module holds an EDGAR SIC code for.
pub fn concentration(set: &TradeSet, ctx: &Context) -> Vec<Insight> {
    if !enough(set) {
        return Vec::new();
    }
    let lab = set.lab();
    let mut out = Vec::new();

    let ticker = |t: &LabTrade| Some(t.ticker.trim().to_uppercase());
    let by_ticker = groups(&lab, ticker);
    let dragged = drag(set, &lab, &by_ticker, &ticker, "ticker_drag");
    let dragged_name = dragged.as_ref().map(|i| i.labels[0].clone());
    out.extend(dragged);
    if let Some(i) = share(&by_ticker, lab.len(), "ticker_share") {
        if Some(&i.labels[0]) != dragged_name.as_ref() {
            out.push(i);
        }
    }

    let class = |t: &LabTrade| Some(t.asset_class.clone()).filter(|c| !c.is_empty());
    let by_class = groups(&lab, class);
    out.extend(drag(set, &lab, &by_class, &class, "class_drag"));

    let sector = |t: &LabTrade| ctx.sectors.get(&t.ticker.trim().to_uppercase()).map(|(code, _)| code.clone());
    let by_sector = groups(&lab, sector);
    let covered: usize = by_sector.values().map(|g| g.trades).sum();
    if covered >= MIN_SAMPLE {
        if let Some(mut i) = share(&by_sector, covered, "sector_share") {
            let code = i.labels[0].clone();
            let industry = ctx
                .sectors
                .values()
                .find(|(c, _)| *c == code)
                .map(|(_, ind)| ind.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| code.clone());
            i.labels = vec![industry, code];
            out.push(i);
        }
    }
    out
}

// ── Fees ─────────────────────────────────────────────────────────────────────

/// What the user's own fee schedules would have charged, on the same contracts or on the
/// other contract of a micro/full-size family: the lab's `fees` scenario, run here as the
/// lab runs it, keeping the cheapest. With no schedule, the statement says so and the page
/// links to the journal's fee schedules: a fee is never built in.
pub fn instrument_fees(set: &TradeSet, ctx: &Context) -> Vec<Insight> {
    if !enough(set) {
        return Vec::new();
    }
    let lab = set.lab();
    let paid: f64 = lab.iter().map(|t| t.fees).sum();
    if ctx.schedules.is_empty() {
        if !ctx.journal_installed || paid <= 0.0 {
            return Vec::new();
        }
        let mut i = Insight::new("instrument_fees", "fees_no_schedule", "info")
            .value("n", lab.len() as f64)
            .value("fees", paid);
        i.evidence.n = lab.len();
        return vec![i];
    }

    let mut best: Option<(Outcome, &str, Option<FamilyMove>)> = None;
    for schedule in &ctx.schedules {
        let Some(rates) = ctx.schedule_rates.get(&schedule.id) else { continue };
        let mut moves: Vec<Option<FamilyMove>> = vec![None];
        for family in whatif::families() {
            for to in [FamilySide::Large, FamilySide::Small] {
                let mv = FamilyMove { family, to };
                if lab.iter().any(|t| mv.applies(t)) {
                    moves.push(Some(mv));
                }
            }
        }
        for mv in moves {
            let (scope, scope_rates, excluded) = match mv {
                Some(m) => {
                    let pick: Vec<(LabTrade, Option<f64>)> = lab
                        .iter()
                        .zip(rates)
                        .filter(|(t, _)| m.applies(t))
                        .map(|(t, r)| (t.clone(), *r))
                        .collect();
                    let excluded = lab.len() - pick.len();
                    let (s, r): (Vec<LabTrade>, Vec<Option<f64>>) = pick.into_iter().unzip();
                    (s, r, excluded)
                }
                None => (lab.clone(), rates.clone(), 0),
            };
            let flags: Vec<Result<f64, &'static str>> = scope
                .iter()
                .zip(&scope_rates)
                .map(|(t, r)| if t.entry_price.is_none() { Err("no_price") } else { r.ok_or("fx") })
                .collect();
            let (trades, rates, unmeasured) = whatif::split_measured(&scope, &set.unmeasured, flags);
            if trades.len() < MIN_SAMPLE {
                continue;
            }
            let (steps, detail) = whatif::fees(&trades, schedule, mv, &rates, excluded);
            let scenario = Scenario::Fees(FeesParams {
                schedule_id: schedule.id,
                family: mv.map(|m| m.family.id.clone()),
                to: mv.map(|m| m.to).unwrap_or_default(),
            });
            let o = whatif::assemble(&trades, steps, detail, scenario, unmeasured, set.currency.as_deref().unwrap_or(""));
            if best.as_ref().is_none_or(|(b, _, _)| o.delta_net > b.delta_net) {
                best = Some((o, schedule.name.as_str(), mv));
            }
        }
    }
    let Some((o, name, mv)) = best else { return Vec::new() };
    if o.delta_net <= 0.0 || !set.material(o.delta_net) {
        return Vec::new();
    }
    let key = if mv.is_some() { "fees_family_saving" } else { "fees_saving" };
    let mut i = Insight::new("instrument_fees", key, "info")
        .label(name)
        .value("saving", o.delta_net)
        .value("paid", o.before.fees)
        .value("alternative", o.after.fees)
        .value("n", o.before.n as f64);
    if let Some(m) = mv {
        i = i.label(m.source_root()).label(match m.to {
            FamilySide::Large => m.family.large.clone(),
            FamilySide::Small => m.family.small.clone(),
        });
    }
    i.evidence.n = o.before.n;
    i.evidence.delta_pnl = Some(o.delta_net);
    i.evidence.delta_dd = Some(o.delta_drawdown);
    i.strength = set.share(o.delta_net);
    i.apply = Some(o.scenario);
    vec![i]
}

// ── Sizing after a losing run ────────────────────────────────────────────────

/// The behaviour module's size-after-a-losing-run measure, on a backtest run or a paper
/// session (a rule that adds size after losses). The journal's behaviour tab already
/// states it for the journal, and the page lists that statement unchanged, so it is not
/// stated twice. The money effect: the trades after two losses at the baseline size.
pub fn sizing_streaks(set: &TradeSet, _: &Context) -> Vec<Insight> {
    if set.kind == SourceKind::Journal || !enough(set) {
        return Vec::new();
    }
    let lab = set.lab();
    let nets: Vec<f64> = lab.iter().map(|t| t.net).collect();
    let sizes: Vec<f64> = set.trades.iter().map(|t| t.notional.unwrap_or(0.0)).collect();
    let e = escalation(&nets, &sizes);
    let (Some(pct), Some(base)) = (e.escalation_pct, e.base_median) else { return Vec::new() };
    if pct < ESCALATION_PCT || e.after_streak.len() < MIN_SIDE {
        return Vec::new();
    }
    let (before, after) = compare(&lab, |i, t| {
        let scale = match sizes[i] {
            s if s > 0.0 && e.after_streak.contains(&i) => base / s,
            _ => 1.0,
        };
        Some(Sim { net: t.net * scale, fees: t.fees * scale })
    });
    let delta = after.net - before.net;
    let streak_net: f64 = e.after_streak.iter().map(|i| nets[*i]).sum();
    let mut i = Insight::new("sizing_streaks", "size_after_losses", "warn")
        .value("escalation", pct)
        .value("trades", e.after_streak.len() as f64)
        .value("net", streak_net);
    i.evidence.n = e.after_streak.len();
    i.evidence.delta_pnl = Some(delta);
    i.evidence.delta_dd = Some(after.max_drawdown - before.max_drawdown);
    i.strength = set.share(delta);
    vec![i]
}

// ── Long against short ───────────────────────────────────────────────────────

/// Long against short expectancy: stated when one side loses while the other earns, with
/// what the book would have made without the losing side.
pub fn symmetry(set: &TradeSet, _: &Context) -> Vec<Insight> {
    if !enough(set) {
        return Vec::new();
    }
    let lab = set.lab();
    let side = |s: &str| -> (usize, f64) {
        let v: Vec<f64> = lab.iter().filter(|t| t.side == s).map(|t| t.net).collect();
        (v.len(), v.iter().sum())
    };
    let (ln, lnet) = side("long");
    let (sn, snet) = side("short");
    if ln < MIN_SIDE || sn < MIN_SIDE {
        return Vec::new();
    }
    let (le, se) = (lnet / ln as f64, snet / sn as f64);
    let (weak, strong, wn, wnet, we, ste) =
        if le < se { ("long", "short", ln, lnet, le, se) } else { ("short", "long", sn, snet, se, le) };
    if !(we < 0.0 && ste > 0.0) {
        return Vec::new();
    }
    let (before, after) = compare(&lab, |_, t| (t.side != weak).then_some(Sim { net: t.net, fees: t.fees }));
    let delta = after.net - before.net;
    if !set.material(delta) {
        return Vec::new();
    }
    let mut i = Insight::new("symmetry", "side_drag", "warn")
        .label(weak)
        .label(strong)
        .value("weak", we)
        .value("strong", ste)
        .value("trades", wn as f64)
        .value("net", wnet);
    i.evidence.n = before.n;
    i.evidence.delta_pnl = Some(delta);
    i.evidence.delta_dd = Some(after.max_drawdown - before.max_drawdown);
    i.strength = set.share(delta);
    vec![i]
}
