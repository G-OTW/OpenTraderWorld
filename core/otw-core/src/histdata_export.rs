//! A stored dataset, handed back as a file.
//!
//! Two formats, for two audiences. **CSV** is what opens in a spreadsheet and what every
//! tutorial reads; it is the lowest common denominator and stays the default. **Parquet**
//! is what the same history is kept as once there is enough of it: typed, columnar, and
//! roughly a tenth the size, so `pd.read_parquet` needs no `parse_dates` argument and no
//! dtype guessing.
//!
//! The Parquet file also carries **what it is**, in its own key-value metadata: the
//! instrument, the timeframe and where the bars came from. A CSV forgets all of that the
//! moment it is written, and re-importing one means typing it back in; a Parquet file
//! exported here and re-imported anywhere in the app fills that form by itself.
//!
//! Both build the whole file in memory before the first byte is sent. That is fine for the
//! datasets a person exports by hand and is the honest limit to raise the day an export
//! becomes a streaming one.

use std::sync::Arc;

use anyhow::{Context, Result};
use parquet::basic::{Compression, ZstdLevel};
use parquet::column::writer::ColumnWriter;
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;
use time::format_description::well_known::Rfc3339;

use otw_store::histdata::{Bar, Dataset};

/// Rows per row group. Big enough that the column statistics are worth something, small
/// enough that a reader can skip a range without holding the whole file.
const ROW_GROUP: usize = 100_000;

/// The keys an exported file carries so a re-import knows what it is holding. Prefixed:
/// a Parquet file's metadata is a shared namespace with whatever wrote it before us.
pub const META_PREFIX: &str = "otw.";

/// One bar per row, microseconds because that is what the store keeps and what pandas
/// reads back without rounding. Adjusted columns are optional: crypto and FX have none.
const SCHEMA: &str = "
    message bar {
        required int64  ts (TIMESTAMP(MICROS, true));
        required double open;
        required double high;
        required double low;
        required double close;
        required double volume;
        optional double adj_open;
        optional double adj_high;
        optional double adj_low;
        optional double adj_close;
    }
";

/// The bars as CSV text, one header row and one row per bar, timestamps in RFC3339.
pub fn csv_bytes(rows: &[Bar]) -> String {
    let mut csv = String::from("ts,open,high,low,close,volume,adj_open,adj_high,adj_low,adj_close\n");
    let opt = |v: Option<f64>| v.map(|n| n.to_string()).unwrap_or_default();
    for b in rows {
        let ts = b.ts.format(&Rfc3339).unwrap_or_default();
        csv.push_str(&format!(
            "{ts},{},{},{},{},{},{},{},{},{}\n",
            b.open,
            b.high,
            b.low,
            b.close,
            b.volume,
            opt(b.adj_open),
            opt(b.adj_high),
            opt(b.adj_low),
            opt(b.adj_close),
        ));
    }
    csv
}

/// Split one optional column into the values present and the definition level per row.
/// Parquet spells "this row has no value" as a level of 0 with nothing written for it.
fn optional(rows: &[Bar], pick: fn(&Bar) -> Option<f64>) -> (Vec<f64>, Vec<i16>) {
    let mut values = Vec::with_capacity(rows.len());
    let mut levels = Vec::with_capacity(rows.len());
    for b in rows {
        match pick(b) {
            Some(v) => {
                values.push(v);
                levels.push(1);
            }
            None => levels.push(0),
        }
    }
    (values, levels)
}

/// The bars as a Parquet file, carrying the dataset's identity in its metadata.
pub fn parquet_bytes(ds: &Dataset, rows: &[Bar]) -> Result<Vec<u8>> {
    let schema = Arc::new(parse_message_type(SCHEMA).context("building the Parquet schema")?);

    // What the file is, so re-importing it does not mean retyping it. `source` is where
    // the bars came from: the provider that served them, or the source an import recorded.
    let source = if ds.source.is_empty() { ds.provider.clone() } else { ds.source.clone() };
    let mut meta = vec![
        kv("provider", &ds.provider),
        kv("asset_type", &ds.asset_type),
        kv("ticker", &ds.ticker),
        kv("timeframe", &ds.timeframe),
        kv("source", &source),
    ];
    if let Some(label) = ds.label.as_deref().filter(|s| !s.is_empty()) {
        meta.push(kv("label", label));
    }

    let props = Arc::new(
        WriterProperties::builder()
            // zstd over snappy: a price column compresses far better and every reader in
            // the ecosystem has supported it for years.
            .set_compression(Compression::ZSTD(ZstdLevel::default()))
            .set_key_value_metadata(Some(meta))
            .build(),
    );

    let mut buf: Vec<u8> = Vec::new();
    let mut writer =
        SerializedFileWriter::new(&mut buf, schema, props).context("opening the Parquet writer")?;
    for chunk in rows.chunks(ROW_GROUP.max(1)) {
        let mut group = writer.next_row_group().context("starting a row group")?;
        let mut index = 0usize;
        while let Some(mut col) = group.next_column().context("opening a column")? {
            write_column(&mut col.untyped(), index, chunk)?;
            col.close().context("closing a column")?;
            index += 1;
        }
        group.close().context("closing a row group")?;
    }
    writer.close().context("finishing the Parquet file")?;
    Ok(buf)
}

/// One column of one row group. The order is the schema's own, so the index is the
/// column: a mismatch here would silently write prices into the wrong field, which is why
/// an unexpected index is an error rather than a skip.
fn write_column(col: &mut ColumnWriter<'_>, index: usize, rows: &[Bar]) -> Result<()> {
    let required = |pick: fn(&Bar) -> f64| rows.iter().map(pick).collect::<Vec<f64>>();
    match (index, col) {
        (0, ColumnWriter::Int64ColumnWriter(w)) => {
            let ts: Vec<i64> = rows
                .iter()
                .map(|b| (b.ts.unix_timestamp_nanos() / 1_000) as i64)
                .collect();
            w.write_batch(&ts, None, None).context("writing timestamps")?;
        }
        (1, ColumnWriter::DoubleColumnWriter(w)) => {
            w.write_batch(&required(|b| b.open), None, None)?;
        }
        (2, ColumnWriter::DoubleColumnWriter(w)) => {
            w.write_batch(&required(|b| b.high), None, None)?;
        }
        (3, ColumnWriter::DoubleColumnWriter(w)) => {
            w.write_batch(&required(|b| b.low), None, None)?;
        }
        (4, ColumnWriter::DoubleColumnWriter(w)) => {
            w.write_batch(&required(|b| b.close), None, None)?;
        }
        (5, ColumnWriter::DoubleColumnWriter(w)) => {
            w.write_batch(&required(|b| b.volume), None, None)?;
        }
        (6..=9, ColumnWriter::DoubleColumnWriter(w)) => {
            let pick: fn(&Bar) -> Option<f64> = match index {
                6 => |b| b.adj_open,
                7 => |b| b.adj_high,
                8 => |b| b.adj_low,
                _ => |b| b.adj_close,
            };
            let (values, levels) = optional(rows, pick);
            w.write_batch(&values, Some(&levels), None)?;
        }
        (i, _) => anyhow::bail!("unexpected Parquet column {i}"),
    }
    Ok(())
}

fn kv(key: &str, value: &str) -> parquet::file::metadata::KeyValue {
    parquet::file::metadata::KeyValue::new(format!("{META_PREFIX}{key}"), value.to_string())
}

/// `provider_ticker_timeframe.<ext>`, with the characters a filesystem refuses taken out
/// (a futures ticker carries `@` and `:`, an option symbol is fine but long).
pub fn filename(ds: &Dataset, ext: &str) -> String {
    let safe = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
            .collect::<String>()
    };
    format!(
        "{}_{}_{}.{ext}",
        safe(&ds.provider),
        safe(&ds.ticker),
        safe(&ds.timeframe)
    )
}
