//! `transform`: reshape what the previous blocks produced, without leaving the trace.
//!
//! Five operations, on purpose. Anything more expressive would be a scripting block, and a
//! stored workflow that can run arbitrary code is a different security question than the
//! one this module answers.

use serde_json::{json, Map, Value};

use super::{NodeCtx, Outcome};

pub const OPS: &[&str] = &["pick", "set", "format", "csv_parse", "join"];

/// Rows a `csv_parse` may produce. Past this, the payload belongs in a blob, not in a
/// step output that every later node carries around.
const MAX_ROWS: usize = 5000;

/// What a `csv_parse` hands to the next block.
/// - `objects`: one object per row, keyed by the header line (the default).
/// - `arrays`: one array of cells per row, no header line.
/// - `map`: the whole file collapsed into **one** object, reading a key column and a value
///   column. This is the two-column settings file, not a table.
pub const CSV_SHAPES: &[&str] = &["objects", "arrays", "map"];

pub fn validate(config: &Value) -> Result<(), String> {
    let op = super::opt(config, "op", "");
    if !OPS.contains(&op) {
        return Err(format!("unknown operation \"{op}\""));
    }
    match op {
        "pick" | "join" => {
            super::field(config, "source")?;
        }
        "csv_parse" => {
            super::field(config, "source")?;
            let shape = csv_shape(config);
            if !CSV_SHAPES.contains(&shape.as_str()) {
                return Err(format!("unknown CSV output \"{shape}\""));
            }
        }
        "format" => {
            super::field(config, "template")?;
        }
        "set" => {
            let fields = config.get("fields").and_then(|v| v.as_object());
            if fields.is_none_or(|f| f.is_empty()) {
                return Err("add at least one field".into());
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn run(ctx: &NodeCtx<'_>) -> Result<Outcome, String> {
    let config = &ctx.node.config;
    let op = super::opt(config, "op", "pick");
    let request = json!({ "op": op });

    let output = match op {
        "pick" => ctx.resolver.render(super::field(config, "source")?, ctx.vars, false).await?,
        "format" => {
            ctx.resolver.render_str(super::field(config, "template")?, ctx.vars, false).await
                .map(Value::String)?
        }
        "set" => {
            let fields = config.get("fields").cloned().unwrap_or_else(|| json!({}));
            ctx.resolver.render_value(&fields, ctx.vars, false).await?
        }
        "csv_parse" => {
            let text = ctx.resolver.render_str(super::field(config, "source")?, ctx.vars, false).await?;
            let delimiter = super::raw(config, "delimiter", ",");
            parse_csv(&text, delimiter, &csv_shape(config), config)?
        }
        "join" => {
            let value = ctx.resolver.render(super::field(config, "source")?, ctx.vars, false).await?;
            let separator = super::raw(config, "separator", ", ");
            let field = config.get("field").and_then(|v| v.as_str());
            let items = match value {
                Value::Array(a) => a,
                other => vec![other],
            };
            let parts: Vec<String> = items
                .iter()
                .map(|item| match field {
                    Some(f) => crate::automator::expr::display(item.get(f).unwrap_or(&Value::Null)),
                    None => crate::automator::expr::display(item),
                })
                .collect();
            Value::String(parts.join(separator))
        }
        other => return Err(format!("unknown operation \"{other}\"")),
    };
    Ok(Outcome::new(request, output))
}

/// The chosen output shape, falling back to the `header` boolean the block used before
/// the shape existed, so a workflow saved back then still parses the same way.
fn csv_shape(config: &Value) -> String {
    match config.get("shape").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => match config.get("header").and_then(|v| v.as_bool()).unwrap_or(true) {
            true => "objects".into(),
            false => "arrays".into(),
        },
    }
}

/// A deliberately small CSV reader: one character delimiter, quoted fields with doubled
/// quotes, no embedded newline inside a quoted field. Anything richer is a real import,
/// and the app already has one.
fn parse_csv(text: &str, delimiter: &str, shape: &str, config: &Value) -> Result<Value, String> {
    let delim = delimiter.chars().next().unwrap_or(',');
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    if shape == "map" {
        return parse_map(lines, delim, config);
    }
    let with_header = shape == "objects";
    let headers: Vec<String> = if with_header {
        match lines.next() {
            Some(line) => split_row(line, delim),
            None => return Ok(json!([])),
        }
    } else {
        Vec::new()
    };
    let mut rows = Vec::new();
    for line in lines {
        if rows.len() >= MAX_ROWS {
            return Err(format!("CSV has more than {MAX_ROWS} rows; narrow it at the source"));
        }
        let cells = split_row(line, delim);
        if with_header {
            let mut obj = Map::new();
            for (i, cell) in cells.into_iter().enumerate() {
                let key = headers.get(i).cloned().unwrap_or_else(|| format!("col{}", i + 1));
                obj.insert(key, Value::String(cell));
            }
            rows.push(Value::Object(obj));
        } else {
            rows.push(Value::Array(cells.into_iter().map(Value::String).collect()));
        }
    }
    Ok(Value::Array(rows))
}

/// The whole file as one object. Naming the two columns says the file has a header line
/// and reads them by name; naming neither reads the first two columns of every line, which
/// is the plain `key,value` file. A repeated key keeps the last row, like an assignment.
fn parse_map<'a>(
    mut lines: impl Iterator<Item = &'a str>,
    delim: char,
    config: &Value,
) -> Result<Value, String> {
    let key_col = super::opt(config, "key_col", "");
    let value_col = super::opt(config, "value_col", "");
    let named = !key_col.is_empty() || !value_col.is_empty();
    let (mut key_at, mut value_at) = (0usize, 1usize);
    if named {
        let Some(head) = lines.next() else { return Ok(json!({})) };
        let headers = split_row(head, delim);
        let find = |name: &str, default: usize| -> Result<usize, String> {
            if name.is_empty() {
                return Ok(default);
            }
            headers
                .iter()
                .position(|h| h.trim().eq_ignore_ascii_case(name))
                .ok_or_else(|| format!("the CSV has no column \"{name}\""))
        };
        key_at = find(key_col, 0)?;
        value_at = find(value_col, 1)?;
    }
    let mut obj = Map::new();
    for line in lines {
        if obj.len() >= MAX_ROWS {
            return Err(format!("CSV has more than {MAX_ROWS} rows; narrow it at the source"));
        }
        let cells = split_row(line, delim);
        let Some(key) = cells.get(key_at).map(|c| c.trim()).filter(|c| !c.is_empty()) else {
            continue;
        };
        let value = cells.get(value_at).cloned().unwrap_or_default();
        obj.insert(key.to_string(), Value::String(value));
    }
    Ok(Value::Object(obj))
}

fn split_row(line: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            c if c == delim && !in_quotes => out.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    out.push(current.trim_end_matches('\r').to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str, shape: &str, config: Value) -> Result<Value, String> {
        parse_csv(text, ",", shape, &config)
    }

    #[test]
    fn a_whitespace_delimiter_survives_the_config_read() {
        // A tab is the value, not padding: trimming it would collapse a TSV into one column.
        let config = json!({ "op": "csv_parse", "delimiter": "\t", "shape": "objects" });
        let out = parse_csv("name\tqty\nAAPL\t3", super::super::raw(&config, "delimiter", ","),
                            "objects", &config)
            .unwrap();
        assert_eq!(out, json!([{ "name": "AAPL", "qty": "3" }]));
    }

    #[test]
    fn a_separator_keeps_its_spaces() {
        let config = json!({ "op": "join", "separator": " | " });
        assert_eq!(super::super::raw(&config, "separator", ", "), " | ");
    }

    #[test]
    fn objects_key_every_row_by_the_header() {
        let out = parse("name,qty\nAAPL,3\nMSFT,7", "objects", json!({})).unwrap();
        assert_eq!(out, json!([{ "name": "AAPL", "qty": "3" }, { "name": "MSFT", "qty": "7" }]));
    }

    #[test]
    fn arrays_keep_the_first_line_as_data() {
        let out = parse("AAPL,3\nMSFT,7", "arrays", json!({})).unwrap();
        assert_eq!(out, json!([["AAPL", "3"], ["MSFT", "7"]]));
    }

    #[test]
    fn map_without_columns_reads_a_plain_key_value_file() {
        let out = parse("api_url,https://x.test\ntimeout,30", "map", json!({})).unwrap();
        assert_eq!(out, json!({ "api_url": "https://x.test", "timeout": "30" }));
    }

    #[test]
    fn map_with_named_columns_skips_the_header_and_ignores_the_others() {
        let csv = "setting,note,value\napi_url,prod,https://x.test\ntimeout,seconds,30";
        let out =
            parse(csv, "map", json!({ "key_col": "setting", "value_col": "value" })).unwrap();
        assert_eq!(out, json!({ "api_url": "https://x.test", "timeout": "30" }));
    }

    #[test]
    fn map_names_a_missing_column_instead_of_returning_nonsense() {
        let err = parse("a,b\n1,2", "map", json!({ "key_col": "nope" })).unwrap_err();
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn map_keeps_the_last_row_of_a_repeated_key_and_skips_a_blank_one() {
        let out = parse("k,1\n,9\nk,2", "map", json!({})).unwrap();
        assert_eq!(out, json!({ "k": "2" }));
    }

    #[test]
    fn quoted_cells_survive_the_delimiter_and_doubled_quotes() {
        let out = parse("k,\"a,b \"\"c\"\"\"", "map", json!({})).unwrap();
        assert_eq!(out, json!({ "k": "a,b \"c\"" }));
    }

    #[test]
    fn shape_falls_back_to_the_old_header_boolean() {
        assert_eq!(csv_shape(&json!({})), "objects");
        assert_eq!(csv_shape(&json!({ "header": false })), "arrays");
        assert_eq!(csv_shape(&json!({ "header": false, "shape": "map" })), "map");
    }
}
