//! A trade book that is damaged, reshuffled or partly unreadable: every row the file holds
//! either becomes a trade or an error naming its line, never a silent half-import. Fixtures
//! in `fixtures/imports/generic/` (synthetic, see its README).

use super::*;

const ROUNDTRIP: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/imports/generic/journal_roundtrip.csv"));
const EXECUTIONS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/imports/generic/journal_executions.csv"));
const FR_LATIN1: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/imports/generic/journal_fr_latin1.csv"));

/// Detect + build the way `analyze` does, without a database.
fn run(bytes: &[u8]) -> (Mapping, build::BuildResult) {
    run_with(bytes, |_| {})
}

/// [`run`], with the mapping adjusted after detection the way the user edits it.
fn run_with(bytes: &[u8], edit: impl FnOnce(&mut Mapping)) -> (Mapping, build::BuildResult) {
    let grid = parse::read_grid("book.csv", bytes, None, None, None).expect("grid");
    let profiles: Vec<Profile> = (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
    let s = detect::suggest(&grid, &profiles, &Default::default(), &dict::JOURNAL);
    let mut mapping = Mapping {
        shape: shape_of(&s).to_string(),
        decimal: s.decimal.to_string(),
        date_order: s.date_order.as_str().to_string(),
        columns: (0..grid.width()).map(|i| (i.to_string(), s.columns.get(&i).cloned().unwrap_or("ignore".into()))).collect(),
        ..Default::default()
    };
    mapping.conventions.qty_sign_is_side = s.qty_sign_is_side;
    edit(&mut mapping);
    let ctx = BuildCtx { categories: vec![], strategies: vec![], default_category: Uuid::nil() };
    let built = build::run(&grid, &mapping, &profiles, &ctx);
    (mapping, built)
}

/// Every source row is accounted for: a trade or an error, each on its own line.
fn accounted(built: &build::BuildResult) -> Vec<usize> {
    let mut lines: Vec<usize> = built.trades.iter().flat_map(|t| t.source_rows.clone()).chain(built.errors.iter().map(|e| e.row)).collect();
    lines.sort();
    lines
}

/// One CSV column removed from every line.
fn without_column(text: &str, idx: usize) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let mut rdr = csv::ReaderBuilder::new().has_headers(false).from_reader(line.as_bytes());
        let rec = rdr.records().next().unwrap().unwrap();
        let cells: Vec<String> = rec.iter().enumerate().filter(|(i, _)| *i != idx).map(|(_, c)| if c.contains(',') { format!("\"{c}\"") } else { c.to_string() }).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

#[test]
fn the_roundtrip_fixture_builds_every_trade() {
    let (m, built) = run(ROUNDTRIP.as_bytes());
    assert_eq!(m.shape, "roundtrip");
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    assert_eq!(built.trades.len(), 3);
    let es = &built.trades[2].input;
    assert_eq!((es.ticker.as_str(), es.side.as_str()), ("ES", "short"));
    assert_eq!((es.entry_price, es.exit_price), (Some(5012.75), Some(4980.25)), "thousands separators inside quotes");
    assert_eq!(accounted(&built), [2, 3, 4]);
}

#[test]
fn the_executions_fixture_folds_into_one_trade() {
    let (m, built) = run(EXECUTIONS.as_bytes());
    assert_eq!(m.shape, "executions");
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    assert_eq!(built.trades.len(), 1);
    assert_eq!(built.trades[0].source_rows, [2, 3, 4]);
}

#[test]
fn the_french_latin1_fixture_reads_decimal_commas_and_day_first_dates() {
    let (_, built) = run(FR_LATIN1);
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    let t = &built.trades[1].input;
    assert_eq!((t.ticker.as_str(), t.side.as_str()), ("MC.PA", "short"));
    assert_eq!(t.entry_price, Some(712.40));
    let at = t.entry_at.unwrap();
    assert_eq!((at.day(), u8::from(at.month())), (30, 3));
    assert_eq!(accounted(&built), [3, 4], "the preamble line is counted");
}

/// Without a quantity column no row is a trade: each one says so on its own line.
#[test]
fn import_rejects_a_book_with_a_missing_quantity_header_row_by_row() {
    let text = without_column(ROUNDTRIP, 2);
    let (_, built) = run(text.as_bytes());
    assert!(built.trades.is_empty());
    assert_eq!(built.errors.iter().map(|e| e.row).collect::<Vec<_>>(), [2, 3, 4]);
    assert!(built.errors.iter().all(|e| e.message.contains("quantity")), "{:?}", built.errors);
}

/// The same book with its columns in another order imports the same trades.
#[test]
fn reordered_headers_build_the_same_trades() {
    let reordered = concat!(
        "Close Time,Commission,Exit Price,Qty,Symbol,Open Time,Entry Price,Side\n",
        "2026-03-03 15:45:00,1.00,160.50,10,AAPL,2026-03-02 14:30:00,150.25,Buy\n",
        "2026-03-04 19:30:00,1.00,395.50,5,MSFT,2026-03-04 15:00:00,400.00,Sell\n",
    );
    let (_, a) = run(ROUNDTRIP.as_bytes());
    let (_, b) = run(reordered.as_bytes());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let key = |t: &build::BuiltTrade| (t.input.ticker.clone(), t.input.side.clone(), t.input.quantity, t.input.entry_price, t.input.exit_price, t.input.entry_at, t.input.exit_at, t.input.fees);
    assert_eq!(b.trades.iter().map(key).collect::<Vec<_>>(), a.trades[..2].iter().map(key).collect::<Vec<_>>());
}

#[test]
fn an_unreadable_price_is_an_error_on_its_line() {
    let text = ROUNDTRIP.replace("400.00", "n/a");
    let (_, built) = run(text.as_bytes());
    assert_eq!(built.trades.len(), 2);
    assert_eq!(built.errors.len(), 1);
    assert_eq!(built.errors[0].row, 3);
    assert!(built.errors[0].message.contains("entry price"), "{}", built.errors[0].message);
    assert!(built.errors[0].raw.contains("MSFT"));
}

#[test]
fn a_header_only_book_imports_nothing_and_reports_nothing() {
    let (_, built) = run(ROUNDTRIP.lines().next().unwrap().as_bytes());
    assert!(built.trades.is_empty() && built.errors.is_empty());
}

/// A file cut mid-line loses the rest of its last row: here the commission. Reading what
/// is left as a whole row books the trade with no fees, and nothing says the line was short.
#[test]
fn a_truncated_line_is_flagged_not_imported_short() {
    let cut = ROUNDTRIP.trim_end().strip_suffix(",4.20").expect("the fixture ends with the ES commission");
    let (_, built) = run(cut.as_bytes());
    let flagged = built.errors.iter().any(|e| e.row == 4) || built.trades.iter().any(|t| t.source_rows == [4] && !t.warnings.is_empty());
    assert!(flagged, "line 4 was cut short and nothing says so");
}

/// An exit date that is there but unreadable is not the same as no exit: dropping it files
/// a closed trade as still open.
#[test]
fn an_unreadable_exit_date_is_flagged() {
    let text = ROUNDTRIP.replace("2026-03-03 15:45:00", "2026-13-45 15:45:00");
    let (_, built) = run(text.as_bytes());
    let flagged = built.errors.iter().any(|e| e.row == 2) || built.trades.iter().any(|t| t.source_rows == [2] && t.warnings.iter().any(|w| w.code == "date"));
    assert!(flagged, "the unreadable exit date on line 2 was dropped silently");
}

/// The same file imported twice yields the same hashes (a re-import is a no-op), and a
/// repeated row stays two rows.
#[test]
fn duplicate_rows_stay_distinct_and_a_reimport_hashes_the_same() {
    let doubled = format!("{ROUNDTRIP}{}\n", ROUNDTRIP.lines().nth(1).unwrap());
    let (_, a) = run(doubled.as_bytes());
    let (_, b) = run(doubled.as_bytes());
    assert_eq!(a.trades.len(), 4);
    let hashes: std::collections::HashSet<_> = a.trades.iter().map(|t| t.row_hash.clone()).collect();
    assert_eq!(hashes.len(), 4, "the repeated row is its own trade");
    assert_eq!(a.trades.iter().map(|t| &t.row_hash).collect::<Vec<_>>(), b.trades.iter().map(|t| &t.row_hash).collect::<Vec<_>>());
}

/// A blank commission on a line that runs to its end is a trade without fees, not a cut.
#[test]
fn a_blank_commission_on_a_whole_line_is_not_flagged_as_cut() {
    let text = ROUNDTRIP.replace("2026-03-04 19:30:00,1.00", "2026-03-04 19:30:00,");
    let (_, built) = run(text.as_bytes());
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    assert!(built.trades.iter().all(|t| t.warnings.iter().all(|w| w.code != "truncated")), "{:?}", built.trades.iter().map(|t| &t.warnings).collect::<Vec<_>>());
}

/// The cut line says which column it lost and that its fees read as 0.
#[test]
fn a_truncated_line_names_the_lost_column() {
    let cut = ROUNDTRIP.trim_end().strip_suffix(",4.20").unwrap();
    let (_, built) = run(cut.as_bytes());
    let w = built.trades.iter().find(|t| t.source_rows == [4]).unwrap().warnings.iter().find(|w| w.code == "truncated").expect("a truncated warning");
    assert!(w.message.contains("Commission") && w.message.contains("fees read as 0"), "{}", w.message);
}

/// The unreadable exit date is an error on its own line, naming the value, and the
/// other lines still build.
#[test]
fn an_unreadable_exit_date_names_the_value() {
    let text = ROUNDTRIP.replace("2026-03-03 15:45:00", "2026-13-45 15:45:00");
    let (_, built) = run(text.as_bytes());
    assert_eq!(built.trades.len(), 2);
    assert_eq!(built.errors.len(), 1);
    assert_eq!(built.errors[0].row, 2);
    assert!(built.errors[0].message.contains("exit date") && built.errors[0].message.contains("2026-13-45"), "{}", built.errors[0].message);
}

/// A fill with an unreadable date cannot be placed in time: an error on its line.
#[test]
fn an_unreadable_fill_date_is_an_error() {
    let text = EXECUTIONS.lines().enumerate().map(|(i, l)| if i == 2 { l.replacen("2026", "2026x", 1) } else { l.to_string() }).collect::<Vec<_>>().join("\n");
    let (m, built) = run(text.as_bytes());
    assert_eq!(m.shape, "executions");
    assert!(built.errors.iter().any(|e| e.row == 3 && e.message.contains("could not be read")), "{:?}", built.errors);
}

// ── Named zones across the 2026 daylight saving changes ──────────────────────

/// One round trip per (open, close) pair, naive local times.
fn book(times: &[(&str, &str)]) -> String {
    let mut out = String::from("Symbol,Side,Qty,Entry Price,Exit Price,Open Time,Close Time,Commission\n");
    for (open, close) in times {
        out.push_str(&format!("AAPL,Buy,10,150.25,160.50,{open},{close},1.00\n"));
    }
    out
}

fn stamps_in(zone: &str, times: &[(&str, &str)]) -> build::BuildResult {
    let zone = zone.to_string();
    run_with(book(times).as_bytes(), |m| m.tz = Some(zone)).1
}

fn unix(t: Option<time::OffsetDateTime>) -> i64 {
    t.expect("a date").unix_timestamp()
}

/// New York: 09:30 is 14:30Z before 8 March and after 1 November, 13:30Z between.
#[test]
fn new_york_follows_its_march_8_and_november_1_changes() {
    let built = stamps_in("America/New_York", &[("2026-03-06 09:30:00", "2026-03-09 09:30:00"), ("2026-10-30 09:30:00", "2026-11-02 09:30:00")]);
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    let t = &built.trades;
    assert_eq!((unix(t[0].input.entry_at), unix(t[0].input.exit_at)), (1_772_807_400, 1_773_063_000));
    assert_eq!((unix(t[1].input.entry_at), unix(t[1].input.exit_at)), (1_793_367_000, 1_793_629_800));
}

/// Paris: 09:00 is 08:00Z before 29 March and after 25 October, 07:00Z between.
#[test]
fn paris_follows_its_march_29_and_october_25_changes() {
    let built = stamps_in("Europe/Paris", &[("2026-03-27 09:00:00", "2026-03-30 09:00:00"), ("2026-10-23 09:00:00", "2026-10-26 09:00:00")]);
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    let t = &built.trades;
    assert_eq!((unix(t[0].input.entry_at), unix(t[0].input.exit_at)), (1_774_598_400, 1_774_854_000));
    assert_eq!((unix(t[1].input.entry_at), unix(t[1].input.exit_at)), (1_792_738_800, 1_793_001_600));
}

/// The hour the clock jumps over never happened: an error on that line, naming the zone.
#[test]
fn a_wall_time_in_the_spring_gap_is_an_error_on_its_line() {
    for (zone, gap) in [("America/New_York", "2026-03-08 02:30:00"), ("Europe/Paris", "2026-03-29 02:30:00")] {
        let built = stamps_in(zone, &[("2026-03-02 10:00:00", "2026-03-02 11:00:00"), (gap, "2026-03-30 10:00:00")]);
        assert_eq!(built.trades.len(), 1, "{zone}");
        assert_eq!(built.errors.len(), 1, "{zone}");
        assert_eq!(built.errors[0].row, 3);
        assert!(built.errors[0].message.contains("does not exist") && built.errors[0].message.contains(zone), "{}", built.errors[0].message);
    }
}

/// The autumn hour shown twice reads as its first occurrence, still on summer time:
/// 01:30 EDT on 1 November is 05:30Z, 02:30 CEST on 25 October is 00:30Z.
#[test]
fn a_wall_time_in_the_autumn_overlap_reads_as_the_earlier_instant() {
    let ny = stamps_in("America/New_York", &[("2026-11-01 01:30:00", "2026-11-02 10:00:00")]);
    assert_eq!(unix(ny.trades[0].input.entry_at), 1_793_511_000);
    let paris = stamps_in("Europe/Paris", &[("2026-10-25 02:30:00", "2026-10-26 10:00:00")]);
    assert_eq!(unix(paris.trades[0].input.entry_at), 1_792_888_200);
}

/// A value carrying its own offset keeps it whatever zone the mapping names.
#[test]
fn an_explicit_offset_wins_over_the_zone() {
    let built = stamps_in("Europe/Paris", &[("2026-03-30T09:00:00Z", "2026-03-30T10:00:00+02:00")]);
    assert_eq!((unix(built.trades[0].input.entry_at), unix(built.trades[0].input.exit_at)), (1_774_861_200, 1_774_857_600));
}

/// Without a zone name the fixed offset is applied as before, on both sides of the change.
#[test]
fn the_fixed_offset_still_applies_without_a_zone() {
    let (_, built) = run_with(book(&[("2026-03-06 09:30:00", "2026-03-09 09:30:00")]).as_bytes(), |m| m.tz_offset = -300);
    assert_eq!((unix(built.trades[0].input.entry_at), unix(built.trades[0].input.exit_at)), (1_772_807_400, 1_773_066_600));
}

/// An unknown zone name is refused, never read as UTC.
#[test]
fn an_unknown_zone_is_refused() {
    let m = Mapping { tz: Some("Mars/Olympus".into()), ..Default::default() };
    assert!(m.zone().unwrap_err().to_string().contains("Mars/Olympus"));
    let built = stamps_in("Mars/Olympus", &[("2026-03-06 09:30:00", "2026-03-09 09:30:00")]);
    assert!(built.trades.is_empty());
    assert!(built.errors[0].message.contains("Mars/Olympus"), "{:?}", built.errors);
}
