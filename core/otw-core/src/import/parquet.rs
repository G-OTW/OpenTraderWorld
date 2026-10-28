//! Parquet in, through the same door as CSV.
//!
//! Parquet is what price history is actually stored as once there is enough of it: typed,
//! columnar, compressed, and the format `pandas.to_parquet` and every exchange archive
//! hand out. Reading one here does **not** get its own import pipeline. The file is turned
//! into the same [`Grid`] of strings a CSV becomes, and everything downstream (column
//! detection, the multilingual dictionary, the mapping step, the preview, the row
//! validation) works on it unchanged. One reader, three importers.
//!
//! Only the typed values need care, and the row API already resolves the annotations that
//! matter: a legacy INT96 timestamp (what Spark and older pandas write) arrives as
//! milliseconds, a `DATE` as a day count, a `DECIMAL` already scaled. What comes out is
//! spelled the way the text readers expect it: timestamps as RFC3339, everything else as
//! its plain value, a null as an empty cell.
//!
//! Arrow is deliberately not in the tree. It would bring forty crates to do what the row
//! API does in one pass, and this reader is not a query engine.

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::parse::Grid;

/// Rows one file may unfold into. A `Grid` holds every cell as an owned `String`, so a
/// file that is small on disk (Parquet compresses ~10:1) is not small in memory. The cap
/// is an honest refusal rather than an allocation failure halfway through.
const MAX_ROWS: usize = 2_000_000;

/// Every Parquet file starts and ends with this.
const MAGIC: &[u8] = b"PAR1";

/// Namespace of the metadata this app writes into a file it exports.
pub use crate::histdata_export::META_PREFIX;

/// Is this a Parquet file? The magic decides, not the extension: an upload arrives named
/// whatever the user's exporter chose, and a `.parquet` that is really a CSV would
/// otherwise fail deep inside the reader instead of being read correctly.
pub fn looks_parquet(filename: &str, bytes: &[u8]) -> bool {
    if bytes.len() > 8 && bytes.starts_with(MAGIC) && bytes.ends_with(MAGIC) {
        return true;
    }
    // A truncated or unusual file still gets tried when it says what it is.
    let lower = filename.to_lowercase();
    lower.ends_with(".parquet") || lower.ends_with(".pq")
}

/// One cell, spelled the way the text readers already understand.
fn cell(f: &Field) -> String {
    match f {
        Field::Null => String::new(),
        // `Display` quotes a string and prints `null` for a null; neither is a cell value.
        Field::Str(s) => s.clone(),
        // The three timestamp shapes become one: RFC3339, which `parse_datetime` reads
        // without a date-order guess and which shows as a real date in the preview.
        Field::TimestampMillis(ms) => iso_nanos(*ms as i128 * 1_000_000),
        Field::TimestampMicros(us) => iso_nanos(*us as i128 * 1_000),
        // Everything else: `Display` is already the plain value (a DECIMAL arrives scaled,
        // a DATE as YYYY-MM-DD, a TIMESTAMP(NANOS) as the raw epoch the unit detector
        // recognizes by magnitude).
        other => other.to_string(),
    }
}

fn iso_nanos(nanos: i128) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_default()
}

/// The `otw.*` key-value metadata a file exported by this app carries, with the prefix
/// taken off: what instrument the bars are of, at what timeframe, from where.
///
/// A CSV forgets all of that the moment it is written. A Parquet file does not, so a
/// round trip through pandas comes back knowing what it is instead of asking the user to
/// retype it. Anything else (a file from elsewhere, an unreadable one) is simply empty:
/// this is a convenience, never a requirement.
pub fn file_metadata(bytes: &[u8]) -> HashMap<String, String> {
    let Ok(reader) = SerializedFileReader::new(bytes::Bytes::copy_from_slice(bytes)) else {
        return HashMap::new();
    };
    reader
        .metadata()
        .file_metadata()
        .key_value_metadata()
        .map(|kvs| {
            kvs.iter()
                .filter_map(|kv| {
                    let key = kv.key.strip_prefix(META_PREFIX)?;
                    let value = kv.value.as_deref()?.trim();
                    (!value.is_empty()).then(|| (key.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read a Parquet file into the grid the rest of the import pipeline works on.
pub fn read_grid(bytes: &[u8]) -> Result<Grid> {
    let reader = SerializedFileReader::new(bytes::Bytes::copy_from_slice(bytes))
        .context("reading the Parquet file")?;

    let descr = reader.metadata().file_metadata().schema_descr();
    let headers: Vec<String> = (0..descr.num_columns())
        .map(|i| descr.column(i).name().to_string())
        .collect();
    if headers.is_empty() {
        return Err(anyhow!("the Parquet file has no columns"));
    }
    let declared = reader.metadata().file_metadata().num_rows();
    if declared as usize > MAX_ROWS {
        return Err(anyhow!(
            "this file holds {declared} rows, past the {MAX_ROWS} one import can read at once"
        ));
    }

    let width = headers.len();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row_lines: Vec<usize> = Vec::new();
    for (i, row) in reader.get_row_iter(None).context("walking the rows")?.enumerate() {
        if rows.len() >= MAX_ROWS {
            return Err(anyhow!(
                "this file holds more than the {MAX_ROWS} rows one import can read at once"
            ));
        }
        let row = row.with_context(|| format!("reading row {}", i + 1))?;
        // A nested or repeated column has no cell in a flat grid; its group is skipped by
        // taking the columns positionally, which is also how the header list was built.
        let mut cells: Vec<String> = row.get_column_iter().map(|(_, f)| cell(f)).collect();
        cells.resize(width, String::new());
        rows.push(cells);
        // Parquet has no header line, so a row's own number is what an error points at.
        row_lines.push(i + 1);
    }

    Ok(Grid {
        // No delimiter to speak of; the mapping round-trips this field and the Parquet
        // branch is chosen before it is ever consulted.
        delimiter: ',',
        header_row: 0,
        headers,
        rows,
        row_lines,
        preamble: Vec::new(),
        sections: Vec::new(),
        section: None,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parquet::basic::Compression;
    use parquet::column::writer::ColumnWriter;
    use parquet::data_type::{ByteArray, Int96};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::parser::parse_message_type;

    use super::*;

    /// 2024-01-02T00:00:00Z as the INT96 a Spark or old-pandas export writes: nanos into
    /// the day, then the Julian day number.
    const JULIAN_2024_01_02: u32 = 2_460_312;

    /// A file in the shapes that actually turn up in somebody else's export: a legacy
    /// INT96 stamp, a DATE, a DECIMAL price, a nullable column and a string.
    fn legacy_file() -> Vec<u8> {
        let schema = Arc::new(
            parse_message_type(
                "message bar {
                    required int96      legacy_ts;
                    required int32      d (DATE);
                    required int32      price (DECIMAL(9,4));
                    optional double     vol;
                    required byte_array note (STRING);
                }",
            )
            .expect("schema"),
        );
        let props = Arc::new(
            WriterProperties::builder()
                .set_compression(Compression::UNCOMPRESSED)
                .build(),
        );
        let mut buf = Vec::new();
        {
            let mut w = SerializedFileWriter::new(&mut buf, schema, props).expect("writer");
            let mut rg = w.next_row_group().expect("row group");
            let mut i = 0;
            while let Some(mut col) = rg.next_column().expect("column") {
                match (i, col.untyped()) {
                    (0, ColumnWriter::Int96ColumnWriter(c)) => {
                        let ts = Int96::from(vec![0, 0, JULIAN_2024_01_02]);
                        c.write_batch(&[ts.clone(), ts], None, None).unwrap();
                    }
                    (1, ColumnWriter::Int32ColumnWriter(c)) => {
                        // Days since the epoch: 2024-01-02 and 2024-01-03.
                        c.write_batch(&[19_724, 19_725], None, None).unwrap();
                    }
                    (2, ColumnWriter::Int32ColumnWriter(c)) => {
                        c.write_batch(&[1_856_400, 1_842_500], None, None).unwrap();
                    }
                    (3, ColumnWriter::DoubleColumnWriter(c)) => {
                        c.write_batch(&[1_000.0], Some(&[1, 0]), None).unwrap();
                    }
                    (4, ColumnWriter::ByteArrayColumnWriter(c)) => {
                        c.write_batch(&[ByteArray::from("a"), ByteArray::from("b")], None, None)
                            .unwrap();
                    }
                    (n, _) => panic!("unexpected column {n}"),
                }
                col.close().unwrap();
                i += 1;
            }
            rg.close().unwrap();
            w.close().unwrap();
        }
        buf
    }

    #[test]
    fn recognizes_a_parquet_file_by_its_magic() {
        let file = legacy_file();
        // The name is wrong on purpose: the bytes decide.
        assert!(looks_parquet("export.csv", &file));
        assert!(!looks_parquet("bars.csv", b"time,close\n2024-01-02,1\n"));
        // A name is still enough when the bytes are unreadable.
        assert!(looks_parquet("bars.parquet", b""));
    }

    /// Every typed shape comes out spelled the way the text readers expect: an INT96 and a
    /// DATE as dates, a DECIMAL already scaled, a null as an empty cell, a string unquoted.
    #[test]
    fn typed_columns_become_readable_cells() {
        let grid = read_grid(&legacy_file()).expect("grid");
        assert_eq!(grid.headers, ["legacy_ts", "d", "price", "vol", "note"]);
        assert_eq!(grid.rows.len(), 2);

        assert_eq!(grid.rows[0][0], "2024-01-02T00:00:00Z");
        assert_eq!(grid.rows[0][1], "2024-01-02");
        assert_eq!(grid.rows[0][2], "185.6400");
        assert_eq!(grid.rows[0][3], "1000.0");
        assert_eq!(grid.rows[0][4], "a", "a string cell carries no quotes");

        assert_eq!(grid.rows[1][3], "", "a null is an empty cell, not the word null");
    }

    /// A Parquet file has no header line, so row 1 is the first record and an error that
    /// names a line names one the user can count to.
    #[test]
    fn rows_are_numbered_from_one() {
        let grid = read_grid(&legacy_file()).expect("grid");
        assert_eq!(grid.row_lines, [1, 2]);
        assert_eq!(grid.header_row, 0);
    }

    #[test]
    fn a_file_that_is_not_parquet_is_refused_clearly() {
        let err = match read_grid(b"not a parquet file at all") {
            Err(e) => e,
            Ok(_) => panic!("a text file was read as Parquet"),
        };
        assert!(format!("{err:#}").contains("Parquet"), "{err:#}");
    }
}
