//! Markdown renderer for the shared report model.
//!
//! Deterministic plain-text output (diffable between runs). Charts become a
//! Unicode sparkline plus a first/last/min/max summary line, so the report
//! stays a single self-contained .md file.

use super::{Align, Block, Chart, Report, Section, Tone};

pub fn render(r: &Report) -> String {
    let mut o = String::new();

    if !r.meta.is_empty() {
        o.push_str("---\n");
        for (k, v) in &r.meta {
            o.push_str(&format!("{k}: {}\n", front_matter_value(v)));
        }
        o.push_str("---\n\n");
    }

    o.push_str(&format!("# {}\n\n", r.title));
    if let Some(s) = &r.subtitle {
        o.push_str(&format!("{s}\n\n"));
    }

    for s in &r.sections {
        render_section(&mut o, s);
    }

    if let Some(n) = &r.footer_note {
        o.push_str(&format!("---\n_{n}_\n"));
    }
    o
}

fn render_section(o: &mut String, s: &Section) {
    if !s.heading.is_empty() {
        let hashes = if s.level >= 3 { "###" } else { "##" };
        o.push_str(&format!("{hashes} {}\n\n", s.heading));
    }
    for b in &s.blocks {
        match b {
            Block::Paragraph(p) => o.push_str(&format!("{p}\n\n")),
            Block::Bullets(items) => {
                for it in items {
                    o.push_str(&format!("- {it}\n"));
                }
                o.push('\n');
            }
            Block::Stats(stats) => {
                o.push_str("| Metric | Value |\n|---|---|\n");
                for st in stats {
                    let hint = st
                        .hint
                        .as_deref()
                        .map(|h| format!(" _({h})_"))
                        .unwrap_or_default();
                    o.push_str(&format!("| {} | {}{hint} |\n", cell(&st.label), cell(&st.value)));
                }
                o.push('\n');
            }
            Block::Table(t) => {
                o.push_str(&format!(
                    "| {} |\n",
                    t.headers.iter().map(|h| cell(h)).collect::<Vec<_>>().join(" | ")
                ));
                let marks: Vec<&str> = (0..t.headers.len())
                    .map(|i| match t.aligns.get(i).copied().unwrap_or_default() {
                        Align::Right => "---:",
                        Align::Left => "---",
                    })
                    .collect();
                o.push_str(&format!("|{}|\n", marks.join("|")));
                for row in &t.rows {
                    o.push_str(&format!(
                        "| {} |\n",
                        row.iter().map(|c| cell(&c.text)).collect::<Vec<_>>().join(" | ")
                    ));
                }
                o.push('\n');
            }
            Block::Chart(c) => render_chart(o, c),
        }
    }
}

/// Sparkline over ≤60 buckets + a numeric summary line.
fn render_chart(o: &mut String, c: &Chart) {
    if c.points.len() < 2 {
        o.push_str("_Not enough data points to draw a curve._\n\n");
        return;
    }
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let ys: Vec<f64> = c.points.iter().map(|p| p.1).collect();
    let (min, max) = ys
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
    let span = (max - min).max(f64::EPSILON);

    // Downsample to at most 60 buckets (mean of each bucket).
    let buckets = ys.len().min(60);
    let mut line = String::new();
    for b in 0..buckets {
        let lo = b * ys.len() / buckets;
        let hi = (((b + 1) * ys.len()) / buckets).max(lo + 1);
        let mean = ys[lo..hi].iter().sum::<f64>() / (hi - lo) as f64;
        let idx = (((mean - min) / span) * 7.0).round().clamp(0.0, 7.0) as usize;
        line.push(BARS[idx]);
    }

    o.push_str(&format!("`{line}`\n\n"));
    o.push_str(&format!(
        "{}: start {} → end {} · min {} · max {} ({} points)\n\n",
        c.y_label,
        super::fmt_num(ys[0], 2),
        super::fmt_num(*ys.last().unwrap(), 2),
        super::fmt_num(min, 2),
        super::fmt_num(max, 2),
        c.points.len()
    ));
    let _ = Tone::Neutral; // tones intentionally not rendered in Markdown
}

/// Keep table cells one-line and pipe-safe.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn front_matter_value(v: &str) -> String {
    // Quote anything that could break simple YAML.
    if v.chars().all(|c| c.is_ascii_alphanumeric() || "._-:/ ".contains(c)) && !v.contains(": ") {
        v.to_string()
    } else {
        format!("\"{}\"", v.replace('"', "'"))
    }
}
