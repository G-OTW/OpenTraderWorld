//! An operations ledger that is damaged, reshuffled or partly unreadable: every row either
//! becomes an operation or an error naming its line. Fixture in `fixtures/imports/portfolio/`
//! (synthetic, see its README).

use super::*;

const LEDGER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/imports/portfolio/ledger.csv"));

fn run(file: &str) -> build::BuildResult {
    let grid = parse::read_grid("ledger.csv", file.as_bytes(), None, None, None).expect("grid");
    let profiles: Vec<Profile> = (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
    let s = detect::suggest(&grid, &profiles, &Default::default(), &dict::PORTFOLIO);
    let mut mapping = Mapping {
        decimal: s.decimal.to_string(),
        date_order: s.date_order.as_str().to_string(),
        columns: (0..grid.width()).map(|i| (i.to_string(), s.columns.get(&i).cloned().unwrap_or("ignore".into()))).collect(),
        ..Default::default()
    };
    mapping.conventions.qty_sign_is_side = s.qty_sign_is_side;
    build::run(&grid, &mapping, &profiles)
}

fn lines(b: &build::BuildResult) -> Vec<usize> {
    let mut l: Vec<usize> = b.ops.iter().map(|o| o.source_row).chain(b.errors.iter().map(|e| e.row)).collect();
    l.sort();
    l
}

#[test]
fn the_ledger_fixture_builds_every_operation() {
    let b = run(LEDGER);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.ops.iter().map(|o| o.side).collect::<Vec<_>>(), ["buy", "sell", "dividend"]);
    assert_eq!(b.ops[2].price, 5.86, "a cash row carries its amount");
    assert_eq!(lines(&b), [2, 3, 4]);
}

#[test]
fn an_unreadable_date_is_an_error_on_its_line() {
    let b = run(&LEDGER.replace("2026-04-11", "2026-04-31"));
    assert_eq!(b.ops.len(), 2);
    assert_eq!(b.errors.len(), 1);
    assert_eq!(b.errors[0].row, 3);
    assert!(b.errors[0].message.contains("date"), "{}", b.errors[0].message);
}

#[test]
fn a_row_cut_before_its_price_is_an_error_on_its_line() {
    let cut = &LEDGER[..LEDGER.find("124.05").unwrap()];
    let b = run(cut);
    assert_eq!(b.ops.len(), 1);
    assert_eq!(b.errors.iter().map(|e| e.row).collect::<Vec<_>>(), [3]);
    assert!(b.errors[0].message.contains("price"), "{}", b.errors[0].message);
}

#[test]
fn reordered_headers_build_the_same_operations() {
    let reordered = concat!(
        "Fees,Price,Symbol,Quantity,Type,Date\n",
        "1.20,118.42,VT,10,Buy,2026-03-04\n",
        "1.20,124.05,VT,4,Sell,2026-04-11\n",
    );
    let a = run(LEDGER);
    let b = run(reordered);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let key = |o: &build::BuiltOp| (o.symbol.clone(), o.side, o.op_date, o.quantity, o.price, o.fee);
    assert_eq!(b.ops.iter().map(key).collect::<Vec<_>>(), a.ops[..2].iter().map(key).collect::<Vec<_>>());
}

/// A ledger whose direction column is gone names nothing it can book: every row is an
/// error, none is guessed into a buy.
#[test]
fn import_rejects_a_ledger_with_a_missing_direction_header_row_by_row() {
    let text: String = LEDGER
        .lines()
        .map(|l| {
            let mut c: Vec<&str> = l.split(',').collect();
            c.remove(2);
            c.join(",") + "\n"
        })
        .collect();
    let b = run(&text);
    assert!(b.ops.is_empty(), "no row is guessed into a direction");
    assert_eq!(b.errors.iter().map(|e| e.row).collect::<Vec<_>>(), [2, 3, 4]);
}

#[test]
fn a_semicolon_ledger_with_decimal_commas_reads_the_same() {
    let fr = LEDGER.replace(',', ";").replace("118.42", "118,42").replace("124.05", "124,05").replace("1.20", "1,20").replace("5.86", "5,86");
    let a = run(LEDGER);
    let b = run(&fr);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.ops.iter().map(|o| (o.price, o.fee)).collect::<Vec<_>>(), a.ops.iter().map(|o| (o.price, o.fee)).collect::<Vec<_>>());
}
