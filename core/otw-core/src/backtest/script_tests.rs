//! Script indicators as backtest operands (Phase 3): checks, history, cache, offsets.

use super::script::{self, RawInput, ScriptDef, ScriptHistory, WarmBars};
use super::tests::settings_always_long;
use super::*;
use otw_script::input::SymInfo;

/// `n` hourly bars from 2024-01-01, a deterministic walk.
pub(super) struct Data {
    pub ts: Vec<String>,
    pub ms: Vec<i64>,
    pub o: Vec<f64>,
    pub h: Vec<f64>,
    pub l: Vec<f64>,
    pub c: Vec<f64>,
    pub v: Vec<f64>,
}

pub(super) fn data(n: usize, seed: u64) -> Data {
    let mut x = seed;
    let mut next = || {
        x = (x * 1103515245 + 12345) % 2147483648;
        x as f64 / 2147483648.0
    };
    let t0 = time::macros::datetime!(2024-01-01 00:00 UTC);
    let mut d = Data { ts: vec![], ms: vec![], o: vec![], h: vec![], l: vec![], c: vec![], v: vec![] };
    let mut close = 100.0f64;
    for i in 0..n {
        let t = t0 + time::Duration::hours(i as i64);
        let open = close;
        close *= 1.0 + (next() - 0.5) * 0.04;
        d.ts.push(t.format(&time::format_description::well_known::Rfc3339).unwrap());
        d.ms.push((t.unix_timestamp_nanos() / 1_000_000) as i64);
        d.o.push(open);
        d.h.push(close.max(open) * 1.001);
        d.l.push(close.min(open) * 0.999);
        d.c.push(close);
        d.v.push(1000.0);
    }
    d
}

pub(super) fn syminfo() -> SymInfo {
    SymInfo { ticker: "X".into(), tickerid: "T:X".into(), ..Default::default() }
}

/// The first `warm` bars of `d` as script history.
pub(super) fn history(d: &Data, warm: usize, syminfo: SymInfo) -> ScriptHistory {
    ScriptHistory::new(
        WarmBars {
            time_ms: d.ms[..warm].to_vec(),
            open: d.o[..warm].to_vec(),
            high: d.h[..warm].to_vec(),
            low: d.l[..warm].to_vec(),
            close: d.c[..warm].to_vec(),
            volume: d.v[..warm].to_vec(),
        },
        syminfo,
        "1h".into(),
    )
}

/// Bars `from..to` of `d`, reading scripts through `h`.
pub(super) fn bars_of<'a>(d: &'a Data, from: usize, to: usize, h: Option<&'a ScriptHistory>) -> Bars<'a> {
    Bars {
        ticker: "X",
        ts: &d.ts[from..to],
        open: &d.o[from..to],
        high: &d.h[from..to],
        low: &d.l[from..to],
        close: &d.c[from..to],
        volume: &d.v[from..to],
        quotes: None,
        sub: None,
        script: h,
    }
}

const CROSS: &str = "//@version=6\nindicator(\"Cross\")\nlen = input.int(5, minval = 2)\nplot(ta.sma(close, len), \"fast\")\nplot(ta.sma(close, 20), \"slow\")";

fn op(id: &str, plot: Option<&str>) -> Operand {
    Operand::CustomIndicator { id: id.into(), plot: plot.map(str::to_string) }
}

/// Long on `left crosses_above right`, out on the reverse.
pub(super) fn cross_settings(source: &str, left: Operand, right: Operand) -> Settings {
    let mut s = settings_always_long();
    let side = s.long.as_mut().unwrap();
    side.entry = Some(SignalGroup { logic: "all".into(), conditions: vec![Signal { left: left.clone(), op: Op::CrossesAbove, right: Some(right.clone()) }] });
    side.exit = Some(SignalGroup { logic: "all".into(), conditions: vec![Signal { left, op: Op::CrossesBelow, right: Some(right) }] });
    s.scripts.insert("s1".into(), ScriptDef { name: "Cross".into(), source: source.into(), inputs: Default::default() });
    s
}

fn sma(c: &[f64], n: usize, i: usize) -> Option<f64> {
    // `pine_sma`: sum of x[i] / n, newest first.
    (i + 1 >= n).then(|| (0..n).fold(0.0, |acc, k| acc + c[i - k] / n as f64))
}

#[test]
fn script_operand_trades() {
    let d = data(300, 7);
    let h = history(&d, 50, syminfo());
    let b = bars_of(&d, 50, 300, Some(&h));
    let s = cross_settings(CROSS, op("s1", Some("fast")), op("s1", Some("slow")));
    assert_eq!(s.validate(), None);
    script::prepare(&s, &[&b]).unwrap();
    let fast = resolve(&op("s1", Some("fast")), &b, &s.defs());
    // Defined from the first traded bar on, thanks to the warmup.
    for (i, v) in fast.iter().enumerate() {
        assert_eq!(*v, sma(&d.c, 5, 50 + i), "bar {i}");
    }
    let slow = resolve(&op("s1", Some("slow")), &b, &s.defs());
    assert!(slow[0].is_some());
    let r = run(&s, &b);
    assert!(!r.trades.is_empty());
    for t in &r.trades {
        // Entry fills at the open after the cross.
        let k = b.ts.iter().position(|x| *x == t.entry_ts).unwrap();
        assert!(fast[k - 1] > slow[k - 1] && fast[k - 2] <= slow[k - 2], "{}", t.entry_ts);
    }
}

#[test]
fn default_plot_is_the_first() {
    let d = data(120, 3);
    let h = history(&d, 20, syminfo());
    let b = bars_of(&d, 20, 120, Some(&h));
    let s = cross_settings(CROSS, op("s1", None), op("s1", Some("slow")));
    script::prepare(&s, &[&b]).unwrap();
    assert_eq!(resolve(&op("s1", None), &b, &s.defs()), resolve(&op("s1", Some("fast")), &b, &s.defs()));
}

#[test]
fn tainted_plot_refused() {
    let src = "//@version=6\nindicator(\"T\")\nplot(barstate.islast ? close : na, \"x\")";
    let s = cross_settings(src, op("s1", Some("x")), Operand::Const { value: 1.0 });
    let e = s.validate().expect("refused");
    assert!(e.contains("barstate.islast, line 3"), "{e}");
}

#[test]
fn plot_title_missing() {
    let s = cross_settings(CROSS, op("s1", Some("nope")), Operand::Const { value: 1.0 });
    let e = s.validate().expect("refused");
    assert!(e.contains("\"nope\""), "{e}");
}

#[test]
fn plot_title_duplicated() {
    let src = "//@version=6\nindicator(\"D\")\nplot(close)\nplot(open)";
    let s = cross_settings(src, op("s1", Some("Plot")), Operand::Const { value: 1.0 });
    let e = s.validate().expect("refused");
    assert!(e.contains("\"Plot\""), "{e}");
}

#[test]
fn unknown_input_key() {
    let mut s = cross_settings(CROSS, op("s1", Some("fast")), Operand::Const { value: 1.0 });
    s.scripts.get_mut("s1").unwrap().inputs.insert("lenght".into(), RawInput::Num(9.0));
    let e = s.validate().expect("refused");
    assert!(e.contains("lenght"), "{e}");
}

#[test]
fn input_out_of_range() {
    let mut s = cross_settings(CROSS, op("s1", Some("fast")), Operand::Const { value: 1.0 });
    s.scripts.get_mut("s1").unwrap().inputs.insert("len".into(), RawInput::Num(1.0));
    let e = s.validate().expect("refused");
    assert!(e.contains("len") && e.contains("range"), "{e}");
}

#[test]
fn unknown_indicator_id_refused() {
    let s = cross_settings(CROSS, op("zz", None), Operand::Const { value: 1.0 });
    let e = s.validate().expect("refused");
    assert!(e.contains("zz"), "{e}");
}

#[test]
fn runtime_error_refuses_the_run() {
    let src = "//@version=6\nindicator(\"M\")\nplot(close / syminfo.mintick, \"t\")";
    let d = data(60, 5);
    let h = history(&d, 10, syminfo());
    let b = bars_of(&d, 10, 60, Some(&h));
    let s = cross_settings(src, op("s1", Some("t")), Operand::Const { value: 1.0 });
    assert_eq!(s.validate(), None);
    let e = script::prepare(&s, &[&b]).unwrap_err();
    assert!(e.contains("SyminfoUnknown") && e.contains("line 3") && e.contains("Cross"), "{e}");
}

#[test]
fn prepare_refuses_missing_history() {
    let d = data(60, 5);
    let b = bars_of(&d, 0, 60, None);
    let s = cross_settings(CROSS, op("s1", Some("fast")), Operand::Const { value: 1.0 });
    let e = script::prepare(&s, &[&b]).unwrap_err();
    assert!(e.contains('X'), "{e}");
}

#[test]
fn positive_offset_reads_the_drawn_value() {
    let src = "//@version=6\nindicator(\"O\")\nplot(close, \"c\", offset = 2)";
    let d = data(40, 9);
    let h = history(&d, 10, syminfo());
    let b = bars_of(&d, 10, 40, Some(&h));
    let s = cross_settings(src, op("s1", Some("c")), Operand::Const { value: 1.0 });
    assert_eq!(s.validate(), None);
    script::prepare(&s, &[&b]).unwrap();
    let col = resolve(&op("s1", Some("c")), &b, &s.defs());
    for (i, v) in col.iter().enumerate() {
        assert_eq!(*v, Some(d.c[10 + i - 2]), "bar {i}");
    }
}

#[test]
fn dca_basket_rule_refused() {
    use super::dca::{DcaBuy, DcaConfig};
    let mut s = cross_settings(CROSS, op("s1", Some("fast")), Operand::Const { value: 1.0 });
    s.kind = "dca".into();
    let cond = SignalGroup { logic: "all".into(), conditions: vec![Signal { left: op("s1", Some("fast")), op: Op::Above, right: Some(Operand::Const { value: 1.0 }) }] };
    let buy: DcaBuy = serde_json::from_value(serde_json::json!({ "amount": 100.0, "per_asset": false })).unwrap();
    s.dca = Some(DcaConfig { buys: vec![DcaBuy { condition: cond, ..buy }], ..serde_json::from_value(serde_json::json!({})).unwrap() });
    let e = s.validate().expect("refused");
    assert!(e.contains("per asset"), "{e}");
}

#[test]
fn cache_shared_across_runs() {
    let d = data(120, 3);
    let h = history(&d, 20, syminfo());
    let b = bars_of(&d, 20, 120, Some(&h));
    let s = cross_settings(CROSS, op("s1", Some("fast")), op("s1", Some("slow")));
    script::prepare(&s, &[&b]).unwrap();
    script::prepare(&s, &[&b]).unwrap();
    let _ = run(&s, &b);
    assert_eq!(h.runs(), 1);
    // Other input values are another run.
    let mut s2 = s.clone();
    s2.scripts.get_mut("s1").unwrap().inputs.insert("len".into(), RawInput::Num(7.0));
    script::prepare(&s2, &[&b]).unwrap();
    assert_eq!(h.runs(), 2);
}

/// Trials running at once on the same input values wait for one run instead of each making it.
#[test]
fn parallel_trials_share_one_run() {
    let d = data(3000, 3);
    let h = history(&d, 20, syminfo());
    let b = bars_of(&d, 20, 3000, Some(&h));
    let s = cross_settings(CROSS, op("s1", Some("fast")), op("s1", Some("slow")));
    std::thread::scope(|sc| {
        for _ in 0..6 {
            sc.spawn(|| script::prepare(&s, &[&b]).unwrap());
        }
    });
    assert_eq!(h.runs(), 1);
}

/// Only the plots a condition reads are kept, 8 bytes a bar, and the cache stays under its
/// byte budget: a run past it is recomputed, never kept.
#[test]
fn cache_keeps_read_plots_within_budget() {
    let d = data(120, 3);
    let h = history(&d, 20, syminfo());
    let b = bars_of(&d, 20, 120, Some(&h));
    let s = cross_settings(CROSS, op("s1", Some("fast")), Operand::Const { value: 1.0 });
    script::prepare(&s, &[&b]).unwrap();
    assert_eq!(h.cached_bytes(), 100 * 8, "one plot of 100 bars");
    let full = run(&s, &b);

    let small = history(&d, 20, syminfo()).with_budget(100 * 8 - 1);
    let b2 = bars_of(&d, 20, 120, Some(&small));
    script::prepare(&s, &[&b2]).unwrap();
    assert_eq!(small.cached_bytes(), 0);
    let again = run(&s, &b2);
    assert!(small.runs() >= 2, "a run past the budget is made again");
    assert_eq!(format!("{:?}", full.trades), format!("{:?}", again.trades));
}
