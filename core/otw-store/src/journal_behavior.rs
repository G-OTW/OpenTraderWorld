//! Behavioural analytics for the Trading Journal.
//!
//! [`crate::journal_analytics`] answers "is the edge real". This file answers the other
//! half: **what the trader does around the edge**, and what it costs. Same closed-trade
//! projection ([`Closed`]), same filter, same FX pass, so nothing here reads the database
//! and nothing here needs a candle.
//!
//! Five questions, one struct each:
//!
//! * [`Concentration`]: is the result regular, or carried by a handful of trades;
//! * [`SizingAfterLoss`]: does the size grow after a losing run;
//! * [`Overtrading`]: does the pace quicken after a loss, and does it pay;
//! * [`SessionDecay`]: does the day's later trades earn less than its first;
//! * [`AfterOutcome`]: the after-a-win trader against the after-a-loss trader.
//!
//! On top of them, [`Insight`] is the only opinionated part: a statement fires when the
//! sample clears a floor **and** the effect clears a threshold. The server decides
//! whether it fires and supplies the raw numbers; the client formats them.
//!
//! Ordering: every metric below reads `closed` in chronological order (the loader sorts
//! it), because "after a loss" and "the third trade of the day" are sequence questions.
//! Money is already in the display currency; R and every ratio are currency-free.

use serde::Serialize;
use std::collections::BTreeMap;
use time::Date;

use crate::journal_analytics::{mean, total, Bucket, Closed};

// ── Small statistics ─────────────────────────────────────────────────────────

/// Median of an unsorted slice. Copies, because every caller here builds a throwaway
/// vector anyway and none of them wants its input reordered.
fn median(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    Some(if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    })
}

/// Least-squares slope of `y` against `x`. `None` when x never varies (one bucket, or a
/// single observation): a vertical fit has no slope to report.
fn slope(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let mx = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let my = points.iter().map(|(_, y)| y).sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (x, y) in points {
        num += (x - mx) * (y - my);
        den += (x - mx) * (x - mx);
    }
    (den > 0.0).then(|| num / den)
}

/// Gini coefficient over non-negative values: 0 = every trade contributed the same,
/// 1 = one trade contributed everything. Reported on the winners only, because that is
/// the question ("is the profit concentrated"), and a mixed-sign Gini means nothing.
fn gini(values: &[f64]) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| *x > 0.0).collect();
    if v.len() < 2 {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len() as f64;
    let sum: f64 = v.iter().sum();
    if sum <= 0.0 {
        return None;
    }
    // G = (2·Σ i·x_i) / (n·Σ x_i) − (n+1)/n, with i one-based over the sorted values.
    let weighted: f64 = v.iter().enumerate().map(|(i, x)| (i as f64 + 1.0) * x).sum();
    Some((2.0 * weighted) / (n * sum) - (n + 1.0) / n)
}

/// Percent change from `base` to `value`, guarded against a zero or negative base (a
/// ratio against "no size" or "a losing baseline" is not a percentage anyone can read).
fn pct_change(value: Option<f64>, base: Option<f64>) -> Option<f64> {
    match (value, base) {
        (Some(v), Some(b)) if b > 0.0 => Some((v - b) / b * 100.0),
        _ => None,
    }
}

// ── Profit concentration ─────────────────────────────────────────────────────

/// Whether the performance is a habit or a lottery ticket.
///
/// The decisive number is `net_ex_top5`: an account whose net turns negative once its
/// five best trades are removed does not have an edge yet, it has outliers.
#[derive(Debug, Serialize)]
pub struct Concentration {
    pub winners: i64,
    pub losers: i64,
    pub breakeven: i64,
    pub gross_win: f64,
    pub gross_loss: f64,
    pub net: f64,
    /// Share of gross profit made by the single best trade, the best 5, the best 10.
    pub top1_share: Option<f64>,
    pub top5_share: Option<f64>,
    pub top10_share: Option<f64>,
    /// Same, on the loss side: share of gross loss carried by the 5 worst trades.
    pub worst5_share: Option<f64>,
    /// How many winners it takes to make half the gross profit.
    pub wins_for_half: Option<i64>,
    /// Net PnL with the five best winners removed, and with the five worst losers
    /// removed. The first says whether the edge survives without its outliers.
    pub net_ex_top5: Option<f64>,
    pub net_ex_worst5: Option<f64>,
    /// Concentration of the gross profit across winners, 0 = flat, 1 = one trade.
    pub gini: Option<f64>,
    pub avg_win: Option<f64>,
    pub median_win: Option<f64>,
    pub avg_loss: Option<f64>,
    pub median_loss: Option<f64>,
    /// Mean win ÷ median win. Well above 1 = the average is dragged by a few big ones.
    pub win_skew: Option<f64>,
}

fn concentration(closed: &[Closed]) -> Concentration {
    let mut wins: Vec<f64> = closed.iter().map(|c| c.net).filter(|n| *n > 0.0).collect();
    let mut losses: Vec<f64> = closed.iter().map(|c| c.net).filter(|n| *n < 0.0).collect();
    let breakeven = closed.iter().filter(|c| c.net == 0.0).count() as i64;
    wins.sort_by(|a, b| b.total_cmp(a)); // biggest first
    losses.sort_by(f64::total_cmp); // worst first

    let gross_win = total(wins.iter().copied());
    let gross_loss = total(losses.iter().copied()).abs();
    let net = gross_win - gross_loss;

    // Share of the gross profit made by the top `n` winners. `None` while the sample is
    // smaller than the slice asked for: "the top 10 of 4 trades" is not a statistic.
    let share = |n: usize| -> Option<f64> {
        (wins.len() >= n && gross_win > 0.0)
            .then(|| total(wins.iter().take(n).copied()) / gross_win * 100.0)
    };
    let worst5_share = (losses.len() >= 5 && gross_loss > 0.0)
        .then(|| total(losses.iter().take(5).copied()).abs() / gross_loss * 100.0);

    let wins_for_half = (gross_win > 0.0).then(|| {
        let mut acc = 0.0;
        let mut k = 0i64;
        for w in &wins {
            acc += w;
            k += 1;
            if acc >= gross_win / 2.0 {
                break;
            }
        }
        k
    });

    let net_ex_top5 =
        (wins.len() > 5).then(|| net - total(wins.iter().take(5).copied()));
    let net_ex_worst5 =
        (losses.len() > 5).then(|| net - total(losses.iter().take(5).copied()));

    let avg_win = mean(&wins);
    let median_win = median(&wins);

    Concentration {
        winners: wins.len() as i64,
        losers: losses.len() as i64,
        breakeven,
        gross_win,
        gross_loss,
        net,
        top1_share: share(1),
        top5_share: share(5),
        top10_share: share(10),
        worst5_share,
        wins_for_half,
        net_ex_top5,
        net_ex_worst5,
        gini: gini(&wins),
        avg_win,
        median_win,
        avg_loss: mean(&losses),
        median_loss: median(&losses),
        win_skew: match (avg_win, median_win) {
            (Some(a), Some(m)) if m > 0.0 => Some(a / m),
            _ => None,
        },
    }
}

// ── Size after a losing run ──────────────────────────────────────────────────

/// One loss-streak depth: what was risked, and what it returned.
#[derive(Debug, Serialize)]
pub struct StreakSizeRow {
    /// `0` = the trade came after a win (or opened the book), `1`, `2`, `3plus`.
    pub key: String,
    pub trades: i64,
    pub avg_size: Option<f64>,
    pub median_size: Option<f64>,
    pub avg_risk_pct: Option<f64>,
    pub net: f64,
    pub expectancy: Option<f64>,
    pub win_rate: Option<f64>,
    pub avg_r: Option<f64>,
}

/// Does the position grow after losses. The classic doubling-down tell, measured on the
/// entry notional (median, so one outsized trade cannot make the pattern) and, when the
/// journal carries stops, on the risk as a share of capital.
#[derive(Debug, Serialize)]
pub struct SizingAfterLoss {
    pub rows: Vec<StreakSizeRow>,
    /// Median size after two or more consecutive losses, against the after-a-win median,
    /// in percent. `+60` = the trades that follow a losing run are 60% bigger.
    pub escalation_pct: Option<f64>,
    /// Same comparison on risk as a share of capital.
    pub risk_escalation_pct: Option<f64>,
    /// Trades that followed at least two losses, and what they made.
    pub after_streak_trades: i64,
    pub after_streak_net: f64,
    pub after_streak_expectancy: Option<f64>,
    pub baseline_expectancy: Option<f64>,
}

/// Number of consecutive losses immediately before each trade, in order. A breakeven
/// trade neither continues nor resets a run: it is not a loss, and calling it a win
/// would erase a real streak.
fn loss_run_before(closed: &[Closed]) -> Vec<i64> {
    let mut out = Vec::with_capacity(closed.len());
    let mut run = 0i64;
    for c in closed {
        out.push(run);
        if c.net < 0.0 {
            run += 1;
        } else if c.net > 0.0 {
            run = 0;
        }
    }
    out
}

const STREAK_KEYS: [&str; 4] = ["0", "1", "2", "3plus"];

fn streak_key(run: i64) -> &'static str {
    match run {
        0 => "0",
        1 => "1",
        2 => "2",
        _ => "3plus",
    }
}

fn sizing_after_loss(closed: &[Closed]) -> SizingAfterLoss {
    let runs = loss_run_before(closed);
    let mut sizes: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut risks: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut nets: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut rs: BTreeMap<&str, Vec<f64>> = BTreeMap::new();

    for (c, run) in closed.iter().zip(&runs) {
        let k = streak_key(*run);
        if c.notional > 0.0 {
            sizes.entry(k).or_default().push(c.notional);
        }
        if let Some(p) = c.risk_pct {
            risks.entry(k).or_default().push(p);
        }
        nets.entry(k).or_default().push(c.net);
        if let Some(r) = c.r {
            rs.entry(k).or_default().push(r);
        }
    }

    let rows: Vec<StreakSizeRow> = STREAK_KEYS
        .iter()
        .map(|k| {
            let n = nets.get(k).map(Vec::as_slice).unwrap_or(&[]);
            let wins = n.iter().filter(|v| **v > 0.0).count() as i64;
            StreakSizeRow {
                key: (*k).to_string(),
                trades: n.len() as i64,
                avg_size: sizes.get(k).and_then(|v| mean(v)),
                median_size: sizes.get(k).and_then(|v| median(v)),
                avg_risk_pct: risks.get(k).and_then(|v| mean(v)),
                net: total(n.iter().copied()),
                expectancy: mean(n),
                win_rate: (!n.is_empty()).then(|| wins as f64 / n.len() as f64 * 100.0),
                avg_r: rs.get(k).and_then(|v| mean(v)),
            }
        })
        .collect();

    // "After a losing run" is two consecutive losses or more: one loss is noise, two is
    // where the doubling-down reflex shows up.
    let after: Vec<f64> = closed
        .iter()
        .zip(&runs)
        .filter(|(_, r)| **r >= 2)
        .map(|(c, _)| c.net)
        .collect();
    let deep_sizes: Vec<f64> = closed
        .iter()
        .zip(&runs)
        .filter(|(c, r)| **r >= 2 && c.notional > 0.0)
        .map(|(c, _)| c.notional)
        .collect();
    let deep_risks: Vec<f64> = closed
        .iter()
        .zip(&runs)
        .filter(|(_, r)| **r >= 2)
        .filter_map(|(c, _)| c.risk_pct)
        .collect();
    let base_sizes: Vec<f64> = closed
        .iter()
        .zip(&runs)
        .filter(|(c, r)| **r == 0 && c.notional > 0.0)
        .map(|(c, _)| c.notional)
        .collect();
    let base_risks: Vec<f64> = closed
        .iter()
        .zip(&runs)
        .filter(|(_, r)| **r == 0)
        .filter_map(|(c, _)| c.risk_pct)
        .collect();
    let base_nets: Vec<f64> = closed
        .iter()
        .zip(&runs)
        .filter(|(_, r)| **r == 0)
        .map(|(c, _)| c.net)
        .collect();

    SizingAfterLoss {
        rows,
        escalation_pct: pct_change(median(&deep_sizes), median(&base_sizes)),
        risk_escalation_pct: pct_change(mean(&deep_risks), mean(&base_risks)),
        after_streak_trades: after.len() as i64,
        after_streak_net: total(after.iter().copied()),
        after_streak_expectancy: mean(&after),
        baseline_expectancy: mean(&base_nets),
    }
}

// ── Overtrading after a loss ─────────────────────────────────────────────────

/// Does the pace quicken after a loss, and does the hurry pay.
///
/// The gap is measured from a trade's exit to the **next trade's entry**: that is the
/// decision window, the moment where "get it back" happens. Trades with no entry stamp
/// are skipped rather than guessed.
#[derive(Debug, Serialize)]
pub struct Overtrading {
    pub pairs: i64,
    /// Median minutes between one exit and the next entry: overall, after a win, after
    /// a loss.
    pub median_gap_min: Option<f64>,
    pub gap_after_win_min: Option<f64>,
    pub gap_after_loss_min: Option<f64>,
    /// How much faster the next trade comes after a loss, in percent of the baseline
    /// gap. `+70` = it comes 70% sooner.
    pub speedup_pct: Option<f64>,
    /// A trade opened after a loss in less than this many minutes counts as a revenge
    /// trade: a quarter of the trader's own usual gap, not a fixed clock.
    pub revenge_threshold_min: Option<f64>,
    pub revenge_trades: i64,
    pub revenge_net: f64,
    pub revenge_win_rate: Option<f64>,
    pub revenge_expectancy: Option<f64>,
    /// Every other trade, for the comparison the revenge number needs.
    pub normal_expectancy: Option<f64>,
    pub normal_win_rate: Option<f64>,
    /// Trades per day, on days that contain at least one loss and on days that do not.
    pub avg_trades_loss_day: Option<f64>,
    pub avg_trades_clean_day: Option<f64>,
    pub loss_days: i64,
    pub clean_days: i64,
}

/// `(index of the follower, gap in minutes, previous trade was a loss)` for every
/// consecutive pair where both timestamps exist.
fn gaps(closed: &[Closed]) -> Vec<(usize, f64, bool)> {
    let mut out = Vec::new();
    for i in 1..closed.len() {
        let Some(entry) = closed[i].entry_at else { continue };
        let prev_exit = closed[i - 1].at;
        if entry < prev_exit {
            // Overlapping positions: the next trade was already open when this one
            // closed, so there is no decision gap to measure.
            continue;
        }
        out.push((
            i,
            (entry - prev_exit).whole_seconds() as f64 / 60.0,
            closed[i - 1].net < 0.0,
        ));
    }
    out
}

fn overtrading(closed: &[Closed]) -> Overtrading {
    let g = gaps(closed);
    let all: Vec<f64> = g.iter().map(|(_, m, _)| *m).collect();
    let after_win: Vec<f64> = g.iter().filter(|(_, _, l)| !*l).map(|(_, m, _)| *m).collect();
    let after_loss: Vec<f64> = g.iter().filter(|(_, _, l)| *l).map(|(_, m, _)| *m).collect();
    let baseline = median(&all);

    // A quarter of the usual gap: fast enough to be a reaction, defined in the trader's
    // own tempo so it means the same thing to a scalper and to a swing trader.
    let threshold = baseline.filter(|b| *b > 0.0).map(|b| b / 4.0);
    let mut is_revenge = vec![false; closed.len()];
    if let Some(th) = threshold {
        for (i, m, loss) in &g {
            if *loss && *m <= th {
                is_revenge[*i] = true;
            }
        }
    }
    let revenge_nets: Vec<f64> = closed
        .iter()
        .zip(&is_revenge)
        .filter(|(_, r)| **r)
        .map(|(c, _)| c.net)
        .collect();
    let normal_nets: Vec<f64> = closed
        .iter()
        .zip(&is_revenge)
        .filter(|(_, r)| !**r)
        .map(|(c, _)| c.net)
        .collect();
    let win_rate = |v: &[f64]| {
        (!v.is_empty())
            .then(|| v.iter().filter(|n| **n > 0.0).count() as f64 / v.len() as f64 * 100.0)
    };

    // Days: a day carrying a loss against a day that does not, counted on the trader's
    // local dates (the loader already shifted them).
    let mut per_day: BTreeMap<Date, (i64, bool)> = BTreeMap::new();
    for c in closed {
        let e = per_day.entry(c.at.date()).or_insert((0, false));
        e.0 += 1;
        e.1 |= c.net < 0.0;
    }
    let loss_days: Vec<f64> = per_day
        .values()
        .filter(|(_, l)| *l)
        .map(|(n, _)| *n as f64)
        .collect();
    let clean_days: Vec<f64> = per_day
        .values()
        .filter(|(_, l)| !*l)
        .map(|(n, _)| *n as f64)
        .collect();

    Overtrading {
        pairs: g.len() as i64,
        median_gap_min: baseline,
        gap_after_win_min: median(&after_win),
        gap_after_loss_min: median(&after_loss),
        speedup_pct: match (median(&after_loss), baseline) {
            (Some(l), Some(b)) if b > 0.0 => Some((b - l) / b * 100.0),
            _ => None,
        },
        revenge_threshold_min: threshold,
        revenge_trades: revenge_nets.len() as i64,
        revenge_net: total(revenge_nets.iter().copied()),
        revenge_win_rate: win_rate(&revenge_nets),
        revenge_expectancy: mean(&revenge_nets),
        normal_expectancy: mean(&normal_nets),
        normal_win_rate: win_rate(&normal_nets),
        avg_trades_loss_day: mean(&loss_days),
        avg_trades_clean_day: mean(&clean_days),
        loss_days: loss_days.len() as i64,
        clean_days: clean_days.len() as i64,
    }
}

// ── Session decay ────────────────────────────────────────────────────────────

/// Does the day get worse as it goes on. Trades are ranked inside their **local day** by
/// entry time (exit time when the entry is missing), so rank 1 is the day's first idea
/// and rank 6+ is what is left after the plan ran out.
#[derive(Debug, Serialize)]
pub struct SessionDecay {
    /// One bucket per rank: `1`…`5`, then `6plus`.
    pub by_rank: Vec<Bucket>,
    /// Average R per rank, in the same order as `by_rank` (a bucket carries money only).
    pub avg_r: Vec<Option<f64>>,
    /// Least-squares slope of net PnL against rank, over the multi-trade days only.
    /// Negative = each further trade of the day is worth that much less.
    pub slope: Option<f64>,
    /// Expectancy of the day's first trade against everything from the fourth on.
    pub first_expectancy: Option<f64>,
    pub later_expectancy: Option<f64>,
    pub first_trades: i64,
    pub later_trades: i64,
    pub first_win_rate: Option<f64>,
    pub later_win_rate: Option<f64>,
    /// Days carrying more than one trade: the only ones the slope is measured on.
    pub multi_trade_days: i64,
    /// Trades on the busiest day, and the median count on a trading day.
    pub max_trades_day: i64,
    pub median_trades_day: Option<f64>,
}

const RANK_KEYS: [&str; 6] = ["1", "2", "3", "4", "5", "6plus"];

/// Bucket index for a one-based rank inside the day; everything from the sixth trade on
/// lands in the last bucket.
fn rank_idx(rank: usize) -> usize {
    rank.saturating_sub(1).min(RANK_KEYS.len() - 1)
}

fn session_decay(closed: &[Closed]) -> SessionDecay {
    // Rank inside the local day, by the moment the position was opened.
    let mut order: Vec<usize> = (0..closed.len()).collect();
    order.sort_by_key(|i| closed[*i].entry_at.unwrap_or(closed[*i].at));
    let mut seen: BTreeMap<Date, usize> = BTreeMap::new();
    let mut rank = vec![0usize; closed.len()];
    for i in order {
        let day = closed[i].entry_at.unwrap_or(closed[i].at).date();
        let n = seen.entry(day).or_insert(0);
        *n += 1;
        rank[i] = *n;
    }
    let day_counts: Vec<f64> = seen.values().map(|n| *n as f64).collect();
    let multi_days = seen.values().filter(|n| **n > 1).count() as i64;

    let mut buckets: Vec<Bucket> = RANK_KEYS.iter().map(|k| Bucket::new(*k)).collect();
    let mut rs: Vec<Vec<f64>> = vec![Vec::new(); RANK_KEYS.len()];
    let mut fit: Vec<(f64, f64)> = Vec::new();
    for (i, c) in closed.iter().enumerate() {
        let idx = rank_idx(rank[i]);
        buckets[idx].push(c.net);
        if let Some(r) = c.r {
            rs[idx].push(r);
        }
        // The slope is only meaningful where a "later trade" existed to compare against.
        if seen
            .get(&c.entry_at.unwrap_or(c.at).date())
            .is_some_and(|n| *n > 1)
        {
            fit.push((rank[i] as f64, c.net));
        }
    }

    let first: Vec<f64> = closed
        .iter()
        .enumerate()
        .filter(|(i, _)| rank[*i] == 1)
        .map(|(_, c)| c.net)
        .collect();
    let later: Vec<f64> = closed
        .iter()
        .enumerate()
        .filter(|(i, _)| rank[*i] >= 4)
        .map(|(_, c)| c.net)
        .collect();
    let win_rate = |v: &[f64]| {
        (!v.is_empty())
            .then(|| v.iter().filter(|n| **n > 0.0).count() as f64 / v.len() as f64 * 100.0)
    };

    SessionDecay {
        by_rank: buckets.into_iter().map(Bucket::seal).collect(),
        avg_r: rs.iter().map(|v| mean(v)).collect(),
        slope: slope(&fit),
        first_expectancy: mean(&first),
        later_expectancy: mean(&later),
        first_trades: first.len() as i64,
        later_trades: later.len() as i64,
        first_win_rate: win_rate(&first),
        later_win_rate: win_rate(&later),
        multi_trade_days: multi_days,
        max_trades_day: seen.values().copied().max().unwrap_or(0) as i64,
        median_trades_day: median(&day_counts),
    }
}

// ── After a win vs after a loss ──────────────────────────────────────────────

/// One side of the comparison: every trade that followed a win, or every trade that
/// followed a loss. Same columns on both sides, so the table reads across.
#[derive(Debug, Serialize, Default)]
pub struct OutcomeRow {
    pub trades: i64,
    pub net: f64,
    pub win_rate: Option<f64>,
    pub expectancy: Option<f64>,
    pub avg_r: Option<f64>,
    pub median_size: Option<f64>,
    pub avg_risk_pct: Option<f64>,
    pub median_hold_min: Option<f64>,
    pub median_gap_min: Option<f64>,
}

/// The two traders in one account. Everything here is measured on the trade *after* the
/// outcome, never on the outcome itself.
#[derive(Debug, Serialize)]
pub struct AfterOutcome {
    pub after_win: OutcomeRow,
    pub after_loss: OutcomeRow,
    /// After-a-loss expectancy minus after-a-win expectancy: negative = the account
    /// gives back what it just lost.
    pub expectancy_delta: Option<f64>,
    /// Median size after a loss against after a win, in percent.
    pub size_delta_pct: Option<f64>,
    /// Median hold after a loss against after a win, in percent (negative = cut sooner).
    pub hold_delta_pct: Option<f64>,
}

fn outcome_row(closed: &[Closed], idx: &[usize], gap_of: &BTreeMap<usize, f64>) -> OutcomeRow {
    let nets: Vec<f64> = idx.iter().map(|i| closed[*i].net).collect();
    let sizes: Vec<f64> = idx
        .iter()
        .map(|i| closed[*i].notional)
        .filter(|n| *n > 0.0)
        .collect();
    let rs: Vec<f64> = idx.iter().filter_map(|i| closed[*i].r).collect();
    let risks: Vec<f64> = idx.iter().filter_map(|i| closed[*i].risk_pct).collect();
    let holds: Vec<f64> = idx
        .iter()
        .filter_map(|i| closed[*i].hold_min.map(|m| m as f64))
        .collect();
    let gs: Vec<f64> = idx.iter().filter_map(|i| gap_of.get(i).copied()).collect();
    OutcomeRow {
        trades: nets.len() as i64,
        net: total(nets.iter().copied()),
        win_rate: (!nets.is_empty())
            .then(|| nets.iter().filter(|n| **n > 0.0).count() as f64 / nets.len() as f64 * 100.0),
        expectancy: mean(&nets),
        avg_r: mean(&rs),
        median_size: median(&sizes),
        avg_risk_pct: mean(&risks),
        median_hold_min: median(&holds),
        median_gap_min: median(&gs),
    }
}

fn after_outcome(closed: &[Closed]) -> AfterOutcome {
    let gap_of: BTreeMap<usize, f64> = gaps(closed).into_iter().map(|(i, m, _)| (i, m)).collect();
    let mut win_idx = Vec::new();
    let mut loss_idx = Vec::new();
    for i in 1..closed.len() {
        // A breakeven trade sets no mood: it belongs to neither side.
        if closed[i - 1].net > 0.0 {
            win_idx.push(i);
        } else if closed[i - 1].net < 0.0 {
            loss_idx.push(i);
        }
    }
    let after_win = outcome_row(closed, &win_idx, &gap_of);
    let after_loss = outcome_row(closed, &loss_idx, &gap_of);
    AfterOutcome {
        expectancy_delta: match (after_loss.expectancy, after_win.expectancy) {
            (Some(l), Some(w)) => Some(l - w),
            _ => None,
        },
        size_delta_pct: pct_change(after_loss.median_size, after_win.median_size),
        hold_delta_pct: pct_change(after_loss.median_hold_min, after_win.median_hold_min),
        after_win,
        after_loss,
    }
}

// ── Insights ─────────────────────────────────────────────────────────────────

/// One plain statement about the account, and the numbers behind it.
///
/// `key` is an i18n key suffix, so the wording lives in the language packs and the server
/// never ships a sentence. `values` are raw numbers: the client knows, per key, which one
/// is money, which is a percentage and which is a count.
#[derive(Debug, Serialize)]
pub struct Insight {
    pub key: String,
    /// `warn` = a leak to fix, `good` = something the account does right, `info` = neutral.
    pub severity: String,
    pub values: BTreeMap<String, f64>,
    /// Names the sentence has to carry that are not numbers (two tickers, a strategy).
    /// Positional, read by the client as `{0}`, `{1}`; empty for most statements.
    pub labels: Vec<String>,
}

fn insight(key: &str, severity: &str, values: &[(&str, f64)]) -> Insight {
    Insight {
        key: key.to_string(),
        severity: severity.to_string(),
        values: values.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
        labels: Vec::new(),
    }
}

/// Minimum closed trades before any behavioural statement is made at all. Below it the
/// panels still draw, but nothing is asserted: three trades cannot show a habit.
const MIN_SAMPLE: usize = 20;
/// Minimum trades on each side of a comparison.
const MIN_SIDE: i64 = 8;

/// Fire the statements the numbers actually support. Every rule carries a sample floor
/// and an effect threshold, and the order is the reading order: the money-losing habits
/// first, the compliments last.
fn insights(
    closed: &[Closed],
    conc: &Concentration,
    sizing: &SizingAfterLoss,
    over: &Overtrading,
    decay: &SessionDecay,
    after: &AfterOutcome,
) -> Vec<Insight> {
    let mut out = Vec::new();
    if closed.len() < MIN_SAMPLE {
        return out;
    }

    // The edge only exists because of a handful of trades.
    if let (Some(ex), true) = (conc.net_ex_top5, conc.net > 0.0) {
        if ex < 0.0 {
            out.push(insight(
                "fragileEdge",
                "warn",
                &[("net", conc.net), ("without", ex)],
            ));
        }
    }
    if let Some(s) = conc.top5_share {
        if s >= 60.0 && conc.winners >= 10 {
            out.push(insight(
                "concentrated",
                "warn",
                &[("share", s), ("winners", conc.winners as f64)],
            ));
        }
    }

    // Sizing up after a losing run.
    if let Some(e) = sizing.escalation_pct {
        if e >= 25.0 && sizing.after_streak_trades >= MIN_SIDE {
            out.push(insight(
                "sizeEscalation",
                "warn",
                &[
                    ("escalation", e),
                    ("trades", sizing.after_streak_trades as f64),
                    ("net", sizing.after_streak_net),
                ],
            ));
        } else if e <= 0.0 && sizing.after_streak_trades >= MIN_SIDE {
            out.push(insight(
                "sizeDiscipline",
                "good",
                &[("trades", sizing.after_streak_trades as f64)],
            ));
        }
    }

    // Trading faster after a loss, and paying for it.
    if over.revenge_trades >= 5 {
        if let (Some(rev), Some(norm)) = (over.revenge_expectancy, over.normal_expectancy) {
            if rev < norm {
                out.push(insight(
                    "revenge",
                    "warn",
                    &[
                        ("trades", over.revenge_trades as f64),
                        ("net", over.revenge_net),
                        ("expectancy", rev),
                        ("normal", norm),
                    ],
                ));
            }
        }
    }
    if let (Some(loss_day), Some(clean_day)) = (over.avg_trades_loss_day, over.avg_trades_clean_day)
    {
        if over.loss_days >= 5 && over.clean_days >= 5 && loss_day >= clean_day * 1.3 {
            out.push(insight(
                "overtrading",
                "warn",
                &[("loss", loss_day), ("clean", clean_day)],
            ));
        }
    }

    // The day decays.
    if let (Some(first), Some(later)) = (decay.first_expectancy, decay.later_expectancy) {
        if decay.multi_trade_days >= 10 && decay.later_trades >= MIN_SIDE && later < first {
            out.push(insight(
                "sessionDecay",
                "warn",
                &[
                    ("first", first),
                    ("later", later),
                    ("trades", decay.later_trades as f64),
                ],
            ));
        }
    }

    // Two different traders on the two sides of a result.
    if after.after_win.trades >= MIN_SIDE && after.after_loss.trades >= MIN_SIDE {
        if let (Some(w), Some(l)) = (after.after_win.expectancy, after.after_loss.expectancy) {
            // Material = at least a third of the winning side's expectancy, so a rounding
            // difference on a flat account never fires this.
            if l < w && (w - l).abs() >= w.abs().max(1e-9) / 3.0 {
                out.push(insight(
                    "afterLossTilt",
                    "warn",
                    &[
                        ("afterWin", w),
                        ("afterLoss", l),
                        ("trades", after.after_loss.trades as f64),
                    ],
                ));
            } else if l >= w {
                out.push(insight("afterLossSteady", "good", &[("afterLoss", l)]));
            }
        }
    }

    // The compliment, and it has to be earned: a regular profit, not a lucky one.
    if let Some(g) = conc.gini {
        if g <= 0.35 && conc.net > 0.0 && conc.winners >= 10 {
            out.push(insight("regular", "good", &[("gini", g)]));
        }
    }

    out
}

// ── Payload ──────────────────────────────────────────────────────────────────

/// Everything the behaviour tab draws, computed from the trades the filter selected.
#[derive(Debug, Serialize)]
pub struct Behavior {
    /// Closed trades the section was computed on, and the floor the statements need.
    pub sample: i64,
    pub min_sample: i64,
    pub concentration: Concentration,
    pub sizing: SizingAfterLoss,
    pub overtrading: Overtrading,
    pub session: SessionDecay,
    pub after: AfterOutcome,
    pub insights: Vec<Insight>,
}

/// Build the behaviour block. `closed` must be in chronological order, which is what
/// [`crate::journal_analytics`] hands over.
pub(crate) fn behavior(closed: &[Closed]) -> Behavior {
    let concentration = concentration(closed);
    let sizing = sizing_after_loss(closed);
    let overtrading = overtrading(closed);
    let session = session_decay(closed);
    let after = after_outcome(closed);
    let insights = insights(closed, &concentration, &sizing, &overtrading, &session, &after);
    Behavior {
        sample: closed.len() as i64,
        min_sample: MIN_SAMPLE as i64,
        concentration,
        sizing,
        overtrading,
        session,
        after,
        insights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use uuid::Uuid;

    /// A closed trade with just the columns the behaviour metrics read.
    fn c(day: u8, hour: u8, net: f64, notional: f64, hold_min: i64) -> Closed {
        let entry = datetime!(2026-01-01 00:00:00 UTC)
            + time::Duration::days(day as i64 - 1)
            + time::Duration::hours(hour as i64);
        let exit = entry + time::Duration::minutes(hold_min);
        Closed {
            id: Uuid::new_v4(),
            net,
            fees: 0.0,
            r: Some(net / 100.0),
            risk_pct: Some(1.0),
            at: exit,
            at_utc: exit,
            entry_at: Some(entry),
            entry_at_utc: Some(entry),
            hold_min: Some(hold_min),
            notional,
            qty: 1.0,
            leverage: 1.0,
            entry_price: Some(100.0),
            exit_price: Some(100.0),
            strategy_id: None,
            ticker: "AAA".into(),
            asset_class: "stock".into(),
            side: "long".into(),
            tags: Vec::new(),
            market: None,
        }
    }

    #[test]
    fn empty_input_is_all_none() {
        let b = behavior(&[]);
        assert_eq!(b.sample, 0);
        assert!(b.insights.is_empty());
        assert_eq!(b.concentration.winners, 0);
        assert!(b.concentration.top5_share.is_none());
        assert!(b.session.slope.is_none());
        assert!(b.after.expectancy_delta.is_none());
    }

    #[test]
    fn single_trade_reports_no_pair_metric() {
        let b = behavior(&[c(1, 9, 50.0, 1000.0, 30)]);
        assert_eq!(b.overtrading.pairs, 0);
        assert!(b.overtrading.median_gap_min.is_none());
        assert_eq!(b.after.after_win.trades, 0);
        assert_eq!(b.session.multi_trade_days, 0);
    }

    #[test]
    fn concentration_finds_the_outlier() {
        // Nine small winners, one huge one, six losers: the profit is one trade.
        let mut v: Vec<Closed> = (0..9).map(|i| c(i + 1, 9, 10.0, 1000.0, 30)).collect();
        v.push(c(10, 9, 900.0, 1000.0, 30));
        v.extend((0..6).map(|i| c(i + 11, 9, -20.0, 1000.0, 30)));
        let conc = concentration(&v);
        assert_eq!(conc.winners, 10);
        assert_eq!(conc.losers, 6);
        assert_eq!(conc.gross_win, 990.0);
        assert_eq!(conc.gross_loss, 120.0);
        assert_eq!(conc.wins_for_half, Some(1));
        assert!(conc.top1_share.unwrap() > 90.0);
        // Net is +870; without the five best (900 + 4×10) it is negative.
        assert!(conc.net_ex_top5.unwrap() < 0.0);
        assert!(conc.gini.unwrap() > 0.5);
    }

    #[test]
    fn even_profit_is_not_concentrated() {
        let v: Vec<Closed> = (0..12).map(|i| c(i + 1, 9, 10.0, 1000.0, 30)).collect();
        let conc = concentration(&v);
        assert!(conc.gini.unwrap().abs() < 1e-9);
        assert_eq!(conc.win_skew, Some(1.0));
        assert_eq!(conc.wins_for_half, Some(6));
    }

    #[test]
    fn loss_run_resets_on_a_win_not_on_a_scratch() {
        let v = vec![
            c(1, 9, -10.0, 1000.0, 30),
            c(2, 9, -10.0, 1000.0, 30),
            c(3, 9, 0.0, 1000.0, 30),
            c(4, 9, -10.0, 1000.0, 30),
            c(5, 9, 20.0, 1000.0, 30),
            c(6, 9, 5.0, 1000.0, 30),
        ];
        assert_eq!(loss_run_before(&v), vec![0, 1, 2, 2, 3, 0]);
    }

    #[test]
    fn sizing_escalation_is_measured_against_the_after_win_size() {
        // After two losses the size doubles.
        let mut v = Vec::new();
        for day in 0..6u8 {
            v.push(c(day * 3 + 1, 9, -50.0, 1000.0, 30));
            v.push(c(day * 3 + 2, 9, -50.0, 1000.0, 30));
            v.push(c(day * 3 + 3, 9, 40.0, 2000.0, 30)); // run == 2 here
        }
        // Seed a few after-a-win trades at the base size so the baseline exists.
        for day in 0..8u8 {
            v.push(c(30 + day, 9, 10.0, 1000.0, 30));
        }
        let s = sizing_after_loss(&v);
        assert_eq!(s.after_streak_trades, 6);
        assert_eq!(s.escalation_pct, Some(100.0));
        assert_eq!(s.rows.len(), 4);
        assert_eq!(s.rows.iter().map(|r| r.trades).sum::<i64>(), v.len() as i64);
    }

    #[test]
    fn revenge_trades_use_the_traders_own_tempo() {
        // Usual pace: one trade every six hours. The loss at rank 2 is answered one hour
        // later by a trade that loses again; the pace then goes back to normal.
        let nets = [12.0, 12.0, -30.0, -25.0, 12.0];
        let mut v = Vec::new();
        let mut hour = 0i64;
        for i in 0..20usize {
            let k = i % nets.len();
            v.push(c(1 + (hour / 24) as u8, (hour % 24) as u8, nets[k], 1000.0, 10));
            hour += if k == 2 { 1 } else { 6 };
        }
        let o = overtrading(&v);
        assert_eq!(o.pairs, 19);
        assert_eq!(o.gap_after_loss_min, Some(200.0));
        assert_eq!(o.gap_after_win_min, Some(350.0));
        // A quarter of the 350-minute usual gap, in the trader's own tempo.
        assert_eq!(o.revenge_threshold_min, Some(87.5));
        assert_eq!(o.revenge_trades, 4);
        assert_eq!(o.revenge_expectancy, Some(-25.0));
        assert_eq!(o.revenge_win_rate, Some(0.0));
        assert!(o.revenge_expectancy.unwrap() < o.normal_expectancy.unwrap());
        assert!(o.speedup_pct.unwrap() > 0.0);
    }

    #[test]
    fn session_decay_sees_the_day_get_worse() {
        let mut v = Vec::new();
        for day in 1..=12u8 {
            v.push(c(day, 9, 60.0, 1000.0, 20));
            v.push(c(day, 11, 20.0, 1000.0, 20));
            v.push(c(day, 13, -10.0, 1000.0, 20));
            v.push(c(day, 15, -40.0, 1000.0, 20));
        }
        let d = session_decay(&v);
        assert_eq!(d.multi_trade_days, 12);
        assert_eq!(d.first_trades, 12);
        assert_eq!(d.later_trades, 12);
        assert!(d.slope.unwrap() < 0.0);
        assert!(d.later_expectancy.unwrap() < d.first_expectancy.unwrap());
        assert_eq!(d.max_trades_day, 4);
        assert_eq!(d.by_rank.len(), 6);
        assert_eq!(d.by_rank[5].trades, 0);
    }

    #[test]
    fn after_outcome_splits_the_two_traders() {
        // The day opens red, and the two trades that follow a loss are three times the
        // size and a third of the hold. The two that follow a win are the normal ones.
        let mut v = Vec::new();
        for day in 1..=10u8 {
            v.push(c(day, 9, -50.0, 1000.0, 60));
            v.push(c(day, 11, -70.0, 3000.0, 20));
            v.push(c(day, 13, 40.0, 3000.0, 20));
            v.push(c(day, 15, 30.0, 1000.0, 60));
        }
        let a = after_outcome(&v);
        assert_eq!(a.after_loss.trades, 20);
        // 10 evenings, plus the 9 mornings that opened after the previous day closed
        // green: the split follows the sequence, day boundary included.
        assert_eq!(a.after_win.trades, 19);
        assert!(a.expectancy_delta.unwrap() < 0.0);
        assert_eq!(a.size_delta_pct, Some(200.0));
        assert!((a.hold_delta_pct.unwrap() + 200.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn insights_stay_silent_under_the_sample_floor() {
        let v: Vec<Closed> = (1..=10u8).map(|d| c(d, 9, -100.0, 5000.0, 30)).collect();
        assert!(behavior(&v).insights.is_empty());
    }

    #[test]
    fn insights_fire_on_a_real_pattern() {
        // Every ingredient at once: the day decays, the size triples once two losses are
        // on the board, and the trades that follow a loss are the worst of the book.
        let mut v = Vec::new();
        for day in 1..=12u8 {
            v.push(c(day, 9, 80.0, 1000.0, 60));
            v.push(c(day, 10, 40.0, 1000.0, 60));
            v.push(c(day, 11, -60.0, 1000.0, 60));
            v.push(c(day, 12, -70.0, 1000.0, 60));
            v.push(c(day, 13, -80.0, 3000.0, 60));
        }
        let b = behavior(&v);
        let keys: Vec<&str> = b.insights.iter().map(|i| i.key.as_str()).collect();
        assert!(keys.contains(&"sizeEscalation"), "{keys:?}");
        assert!(keys.contains(&"sessionDecay"), "{keys:?}");
        assert!(keys.contains(&"afterLossTilt"), "{keys:?}");
        assert!(b.insights.iter().all(|i| !i.values.is_empty()));
    }

    #[test]
    fn all_wins_never_reports_a_loss_side() {
        let v: Vec<Closed> = (1..=25u8).map(|d| c(d, 9, 10.0, 1000.0, 30)).collect();
        let b = behavior(&v);
        assert_eq!(b.concentration.losers, 0);
        assert_eq!(b.concentration.gross_loss, 0.0);
        assert_eq!(b.after.after_loss.trades, 0);
        assert!(b.after.after_loss.expectancy.is_none());
        assert_eq!(b.overtrading.revenge_trades, 0);
        assert_eq!(b.sizing.after_streak_trades, 0);
    }
}
