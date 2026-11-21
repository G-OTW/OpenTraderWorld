mod common;
use common::*;

/// 100k bars, ~20 statements, 5 `ta.*` calls: must stay interactive (run in release).
#[test]
#[ignore = "timing: cargo test -p otw-script --release --test perf -- --ignored"]
fn perf_100k_bars() {
    let p = compile(&v6(PERF_BODY));
    let bars = synth(100_000, 42);
    let i = input(bars);
    let t = std::time::Instant::now();
    let out = p.run(&i).expect("runs");
    let ms = t.elapsed().as_millis();
    assert_eq!(out.plots[0].values.len(), 100_000);
    eprintln!("perf_100k_bars: {ms} ms");
    assert!(ms < 100, "{ms} ms");
}

/// One forming-bar update after 20,000 bars: the live chart's cost per tick (spec §6 #8).
#[test]
#[ignore = "timing: cargo test -p otw-script --release --test perf -- --ignored"]
fn perf_session_advance() {
    let p = std::rc::Rc::new(compile(&v6(PERF_BODY)));
    let bars = synth(20_001, 42);
    let mut s = otw_script::session::Session::open(p, input(sub(&bars, 0, 20_000)), false).expect("opens");
    let mut times = Vec::new();
    for k in 0..20 {
        let mut tail = sub(&bars, 20_000, 20_001);
        tail.close[0] *= 1.0 + k as f64 * 1e-4;
        tail.high[0] = tail.high[0].max(tail.close[0]);
        let t = std::time::Instant::now();
        s.advance(tail, true).expect("advances");
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    let median = times[times.len() / 2];
    eprintln!("perf_session_advance: {median:.3} ms");
    assert!(median < 5.0, "{median} ms");
}
