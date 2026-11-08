//! Reading a damaged or unusual file: the grid either comes out right or the read fails
//! with a sentence, and a cell either reads as what it says or not at all. Fixtures live in
//! `fixtures/imports/` (synthetic, see its README); each corruption is derived here, next to
//! its assertion.

use time::{OffsetDateTime, UtcOffset};

use super::parse::{self, DateOrder};

const ROUNDTRIP: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/imports/generic/journal_roundtrip.csv"));
const FR_LATIN1: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/imports/generic/journal_fr_latin1.csv"));

fn grid(bytes: &[u8]) -> parse::Grid {
    parse::read_grid("book.csv", bytes, None, None, None).expect("grid")
}

fn utc(raw: &str, order: DateOrder, tz_min: i32) -> Option<OffsetDateTime> {
    parse::parse_datetime(raw, order, tz_min).map(|d| d.to_offset(UtcOffset::UTC))
}

fn iso(d: OffsetDateTime) -> String {
    d.format(&time::format_description::well_known::Rfc3339).unwrap()
}

// ── Whole-file damage ────────────────────────────────────────────────────────

#[test]
fn import_rejects_an_empty_file_with_a_sentence() {
    for bytes in [&b""[..], b"   \n\n\t\n", b"\xEF\xBB\xBF", b"\xEF\xBB\xBF\r\n"] {
        let err = parse::read_grid("book.csv", bytes, None, None, None).err().expect("an empty file is refused");
        assert!(err.to_string().contains("empty"), "{err}");
    }
}

#[test]
fn a_header_only_file_is_a_grid_with_no_rows() {
    let g = grid(b"Symbol,Side,Qty,Entry Price\n");
    assert_eq!(g.headers, ["Symbol", "Side", "Qty", "Entry Price"]);
    assert!(g.rows.is_empty());
}

#[test]
fn a_utf8_bom_is_not_part_of_the_first_header() {
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend_from_slice(ROUNDTRIP);
    let with = grid(&bytes);
    let without = grid(ROUNDTRIP);
    assert_eq!(with.headers[0], "Symbol");
    assert_eq!(with.headers, without.headers);
    assert_eq!(with.rows, without.rows);
}

/// The same book written with each delimiter a spreadsheet offers reads as one grid.
#[test]
fn every_delimiter_reads_the_same_grid() {
    let text = String::from_utf8(ROUNDTRIP.to_vec()).unwrap();
    // The fixture quotes the cells holding a comma; once the delimiter changes, the quotes
    // stay and the cells read back unchanged.
    let reference = grid(ROUNDTRIP);
    for d in [';', '\t', '|'] {
        let mut out = String::new();
        for line in text.lines() {
            let mut cells = Vec::new();
            let mut rdr = csv::ReaderBuilder::new().has_headers(false).from_reader(line.as_bytes());
            for rec in rdr.records() {
                cells = rec.unwrap().iter().map(str::to_string).collect();
            }
            out.push_str(&cells.join(&d.to_string()));
            out.push('\n');
        }
        let g = grid(out.as_bytes());
        assert_eq!(g.delimiter, d, "delimiter {d:?} is sniffed");
        assert_eq!(g.headers, reference.headers, "delimiter {d:?}");
        assert_eq!(g.rows, reference.rows, "delimiter {d:?}");
    }
}

#[test]
fn a_latin1_export_is_decoded_not_mangled() {
    let g = grid(FR_LATIN1);
    assert_eq!(g.delimiter, ';');
    assert!(g.headers.iter().any(|h| h == "Quantité"), "{:?}", g.headers);
    assert!(g.headers.iter().any(|h| h == "Prix d'entrée"), "{:?}", g.headers);
    assert_eq!(g.preamble, ["Relevé de compte"]);
    assert_eq!(g.rows.len(), 2);
}

/// Excel's "Unicode text" export is UTF-16LE with a BOM and tabs. Read as Latin-1 every
/// other byte is a NUL and no header survives: it has to be decoded, or refused by name.
#[test]
fn a_utf16_export_is_decoded_or_refused_by_name() {
    let text = "Symbol\tSide\tQty\tEntry Price\r\nAAPL\tBuy\t10\t150.25\r\n";
    let mut bytes = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    match parse::read_grid("book.txt", &bytes, None, None, None) {
        Ok(g) => assert_eq!(g.headers, ["Symbol", "Side", "Qty", "Entry Price"]),
        Err(e) => assert!(e.to_string().to_lowercase().contains("utf-16"), "{e}"),
    }
}

/// Reordered columns are the same columns: the grid keeps them as the file has them.
#[test]
fn reordered_headers_keep_each_value_under_its_own_header() {
    let g = grid(b"Qty,Symbol,Entry Price,Side\n10,AAPL,150.25,Buy\n");
    let at = |h: &str| {
        let i = g.headers.iter().position(|x| x == h).unwrap();
        g.cell(&g.rows[0], i).to_string()
    };
    assert_eq!((at("Symbol"), at("Qty"), at("Side"), at("Entry Price")), ("AAPL".into(), "10".into(), "Buy".into(), "150.25".into()));
}

// ── Line numbers the user can find ───────────────────────────────────────────

#[test]
fn row_lines_count_the_preamble_and_the_header() {
    let g = grid(FR_LATIN1);
    assert_eq!(g.row_lines, [3, 4]);
}

/// A row cut short (a file truncated mid-line) is kept, padded, at its own line: the
/// builder then reports what is missing on that line instead of dropping it.
#[test]
fn a_truncated_last_line_is_kept_at_its_line() {
    let text = String::from_utf8(ROUNDTRIP.to_vec()).unwrap();
    let cut = &text[..text.rfind(",\"4,980.25\"").unwrap()]; // inside the last row
    let g = grid(cut.as_bytes());
    assert_eq!(g.rows.len(), 3);
    assert_eq!(*g.row_lines.last().unwrap(), 4);
    assert_eq!(g.rows[2].len(), g.width(), "padded to the header width");
}

/// A blank line between rows is a line the user sees in an editor. An error on the row after
/// it must name that row's own line.
#[test]
fn a_blank_line_does_not_shift_the_line_numbers() {
    let g = grid(b"Symbol,Side,Qty\nAAPL,Buy,10\n\nMSFT,Sell,5\n");
    assert_eq!(g.row_lines, [2, 4]);
}

/// A quoted cell spanning two lines moves every later row down one line.
#[test]
fn a_multiline_quoted_cell_does_not_shift_the_line_numbers() {
    let g = grid(b"Symbol,Note,Qty\nAAPL,\"first line\nsecond line\",10\nMSFT,ok,5\n");
    assert_eq!(g.row_lines, [2, 4]);
}

// ── Numbers ──────────────────────────────────────────────────────────────────

#[test]
fn numbers_read_in_the_column_s_own_convention() {
    // (column values, decimal the column is read with, the value of its first cell)
    let cases: &[(&[&str], char, f64)] = &[
        (&["1,5", "2,25"], ',', 1.5),
        (&["1.234,56", "10,00"], ',', 1234.56),
        (&["1 234,56", "3,5"], ',', 1234.56),
        (&["1\u{a0}234,56"], ',', 1234.56),
        (&["1,234", "12,500"], '.', 1234.0), // a comma followed by three digits groups thousands
        (&["1,234.5"], '.', 1234.5),
        (&["(12.50)"], '.', -12.5),
        (&["1 234,50-", "3,5"], ',', -1234.5),
        (&["−3.5"], '.', -3.5),
        (&["$1,234.00"], '.', 1234.0),
        (&["12.5%"], '.', 12.5),
        (&["31358.48(1)"], '.', 31358.48),
    ];
    for (values, decimal, want) in cases {
        let d = parse::column_decimal(values);
        assert_eq!(d, *decimal, "column {values:?}");
        assert_eq!(parse::parse_number(values[0], d), Some(*want), "{:?}", values[0]);
    }
}

/// A German fees column is negative throughout and writes its sign last ("1,50-"). With no
/// positive value to settle it, the column must still read the comma as the decimal: read
/// as a group separator, every fee comes out a hundred times too large.
#[test]
fn a_column_of_trailing_sign_values_keeps_its_decimal_comma() {
    let column = ["1,50-", "2,00-", "12,75-"];
    let d = parse::column_decimal(&column);
    assert_eq!(parse::parse_number(column[0], d), Some(-1.5));
}

/// IBKR's CSV statements write `yyyyMMdd;HHmmss`, as the parser's own doc says it reads.
#[test]
fn the_ibkr_compact_stamp_reads() {
    assert_eq!(utc("20260302;143000", DateOrder::Auto, 0).map(iso).as_deref(), Some("2026-03-02T14:30:00Z"));
    assert_eq!(utc("20260302", DateOrder::Auto, 0).map(iso).as_deref(), Some("2026-03-02T00:00:00Z"));
}

#[test]
fn text_is_not_a_number() {
    for raw in ["", "  ", "n/a", "-", "--", "abc", "€", "()"] {
        assert_eq!(parse::parse_number(raw, '.'), None, "{raw:?}");
    }
}

#[test]
fn a_number_with_two_decimal_points_is_not_read() {
    assert_eq!(parse::parse_number("1.2.3", '.'), None);
}

// ── Dates and zones ──────────────────────────────────────────────────────────

#[test]
fn dates_in_every_written_form_land_on_the_same_instant() {
    let want = "2026-03-02T14:30:00Z";
    for (raw, order) in [
        ("2026-03-02T14:30:00Z", DateOrder::Auto),
        ("2026-03-02T15:30:00+01:00", DateOrder::Auto),
        ("2026-03-02 09:30:00-05:00", DateOrder::Auto),
        ("2026-03-02 14:30:00", DateOrder::Auto),
        ("02/03/2026 14:30", DateOrder::Dmy),
        ("03/02/2026 14:30", DateOrder::Mdy),
        ("03/02/2026 02:30 PM", DateOrder::Mdy),
        ("2-Mar-2026 14:30", DateOrder::Auto),
        ("Mar 2, 2026 14:30", DateOrder::Auto),
        ("1772461800", DateOrder::Auto),
        ("1772461800000", DateOrder::Auto),
    ] {
        let got = utc(raw, order, 0).unwrap_or_else(|| panic!("{raw:?} reads"));
        assert_eq!(iso(got), want, "{raw:?}");
    }
}

/// A stamp that carries its own offset keeps it on both sides of a clock change, and the
/// mapping's offset only applies to naive stamps. US 2026: 8 March and 1 November, EU 2026:
/// 29 March and 25 October, so for a few weeks New York is five hours behind Paris, not six.
#[test]
fn explicit_offsets_hold_across_the_2026_clock_changes() {
    let cases = [
        // Paris, before and after 29 March 2026 (01:00 UTC).
        ("2026-03-28T09:00:00+01:00", "2026-03-28T08:00:00Z"),
        ("2026-03-30T09:00:00+02:00", "2026-03-30T07:00:00Z"),
        // New York, before and after 8 March 2026 (07:00 UTC), and after 1 November.
        ("2026-03-06T09:30:00-05:00", "2026-03-06T14:30:00Z"),
        ("2026-03-09T09:30:00-04:00", "2026-03-09T13:30:00Z"),
        ("2026-11-02T09:30:00-05:00", "2026-11-02T14:30:00Z"),
        // A compact offset as some exports write it.
        ("2026-10-26 09:00:00 +0100", "2026-10-26T08:00:00Z"),
    ];
    for (raw, want) in cases {
        assert_eq!(iso(utc(raw, DateOrder::Auto, 0).unwrap()), want, "{raw}");
        // The mapping's own offset never overrides one the stamp carries.
        assert_eq!(iso(utc(raw, DateOrder::Auto, 540).unwrap()), want, "{raw} with a +09:00 mapping");
    }
    // A naive stamp takes the mapping's offset.
    assert_eq!(iso(utc("2026-03-09 09:30:00", DateOrder::Auto, -240).unwrap()), "2026-03-09T13:30:00Z");
}

/// A trade at 23:30 New York is already the next day in UTC: the instant is kept, never
/// the wall-clock date.
#[test]
fn a_stamp_crossing_utc_midnight_keeps_its_instant() {
    let d = utc("2026-06-30 23:30:00", DateOrder::Auto, -240).unwrap();
    assert_eq!(iso(d), "2026-07-01T03:30:00Z");
}

#[test]
fn an_ambiguous_column_is_decided_by_its_one_telling_value() {
    assert_eq!(parse::column_date_order(&["01/02/2026", "13/02/2026"]), Some(DateOrder::Dmy));
    assert_eq!(parse::column_date_order(&["01/02/2026", "02/13/2026"]), Some(DateOrder::Mdy));
    assert_eq!(parse::column_date_order(&["01/02/2026", "03/04/2026"]), None);
}

#[test]
fn impossible_dates_do_not_read() {
    for raw in ["2026-02-30", "2026-13-01", "31/04/2026", "2026-00-10", "not a date", "29/02/2026"] {
        assert_eq!(utc(raw, DateOrder::Dmy, 0), None, "{raw:?}");
    }
    // 2028 is a leap year.
    assert!(utc("29/02/2028", DateOrder::Dmy, 0).is_some());
}

/// A day or month past 255 wraps through `as u8` and lands on a real date: `01/257/2026`
/// reads as 1 January. A value that cannot be a date must not read as one.
#[test]
fn an_out_of_range_day_is_not_wrapped_into_a_real_date() {
    assert_eq!(utc("01/257/2026", DateOrder::Mdy, 0), None);
    assert_eq!(utc("15/257/2026", DateOrder::Dmy, 0), None);
}

/// 25:00 is no time of day. Clamping it to 23:00 moves the trade silently.
#[test]
fn an_impossible_time_of_day_does_not_read() {
    assert_eq!(utc("2026-03-02 25:00:00", DateOrder::Auto, 0), None);
    assert_eq!(utc("2026-03-02 10:61:00", DateOrder::Auto, 0), None);
}

/// A malformed zone suffix must not crash the import. `-128` overflows the `i8` the offset
/// is multiplied in (a panic in a debug build, a wrong offset in release).
#[test]
fn a_malformed_zone_suffix_never_panics() {
    let got = std::panic::catch_unwind(|| utc("2026-03-02 10:00-128", DateOrder::Auto, 0));
    assert!(got.is_ok(), "parse_datetime panicked");
}
