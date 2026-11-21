//! The trade-book import (journal and portfolio share it): bytes to a grid, every cell to a
//! number, a date, a side, then column detection. Never panics.

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "import_src/mod.rs"]
mod import;

use import::{detect, dict, parse};

fuzz_target!(|data: &[u8]| {
    for name in ["book.csv", "book.json"] {
        let Ok(grid) = parse::read_grid(name, data, None, None, None) else { continue };
        let profiles: Vec<detect::Profile> = (0..grid.width()).map(|i| detect::profile_column(&grid, i)).collect();
        let _ = detect::suggest(&grid, &profiles, &Default::default(), &dict::JOURNAL);
        let _ = detect::suggest(&grid, &profiles, &Default::default(), &dict::PORTFOLIO);
        for idx in 0..grid.width() {
            let column = grid.column(idx);
            let decimal = parse::column_decimal(&column);
            let order = parse::column_date_order(&column).unwrap_or(parse::DateOrder::Auto);
            for cell in column {
                let _ = parse::parse_number(cell, decimal);
                for tz in [0, 60, -300, 840, -840] {
                    let _ = parse::parse_datetime(cell, order, tz);
                }
                let _ = parse::value_kind(cell, decimal);
                let _ = parse::side_of(cell, &[]);
                let _ = parse::kind_of(cell);
            }
        }
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = parse::decode_upload(text);
    }
});
