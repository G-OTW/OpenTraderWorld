//! PDF renderer for the shared report model.
//!
//! Hand-rolled PDF 1.4 writer — no external dependency, mirroring the approach
//! of the hand-rolled MCP server. Uses the base-14 Helvetica family (no font
//! embedding needed) with WinAnsi encoding, real vector charts, stat cards and
//! zebra-striped tables. Output is a complete, uncompressed PDF byte stream.

use super::{Align, Block, Chart, Report, Stat, Table, Tone};

const PAGE_W: f64 = 595.28; // A4 portrait, points
const PAGE_H: f64 = 841.89;
const MARGIN: f64 = 48.0;
const FOOTER_H: f64 = 26.0;

type Rgb = (f64, f64, f64);
const TEXT: Rgb = (0.10, 0.12, 0.16);
const MUTED: Rgb = (0.44, 0.47, 0.53);
const BORDER: Rgb = (0.84, 0.86, 0.89);
const STRIPE: Rgb = (0.955, 0.962, 0.972);
const CARD_BG: Rgb = (0.965, 0.970, 0.978);
const ACCENT: Rgb = (0.23, 0.47, 0.93);
const ACCENT_SOFT: Rgb = (0.875, 0.915, 0.985);
const GREEN: Rgb = (0.075, 0.55, 0.32);
const RED: Rgb = (0.83, 0.24, 0.24);

const REGULAR: u8 = 1; // /F1 Helvetica
const BOLD: u8 = 2; // /F2 Helvetica-Bold
const OBLIQUE: u8 = 3; // /F3 Helvetica-Oblique

pub fn render(r: &Report) -> Vec<u8> {
    let mut d = Doc::new();
    d.title(r);
    for s in &r.sections {
        d.section(s);
    }
    if let Some(n) = &r.footer_note {
        d.ensure(24.0);
        d.y -= 10.0;
        d.hline(MARGIN, PAGE_W - MARGIN, d.y, 0.6, BORDER);
        d.y -= 12.0;
        d.wrapped_text(n, MARGIN, PAGE_W - 2.0 * MARGIN, OBLIQUE, 7.5, MUTED, 10.0);
    }
    d.finish(&r.title)
}

// ── Layout engine ─────────────────────────────────────────────────────────────

struct Doc {
    pages: Vec<String>,
    cur: String,
    y: f64,
}

impl Doc {
    fn new() -> Self {
        Doc { pages: Vec::new(), cur: String::new(), y: PAGE_H - MARGIN }
    }

    fn new_page(&mut self) {
        self.pages.push(std::mem::take(&mut self.cur));
        self.y = PAGE_H - MARGIN;
    }

    /// Break to a new page unless `h` points still fit above the footer.
    fn ensure(&mut self, h: f64) {
        if self.y - h < MARGIN + FOOTER_H {
            self.new_page();
        }
    }

    // ── primitives ──
    fn text(&mut self, s: &str, x: f64, y: f64, font: u8, size: f64, color: Rgb) {
        let esc = escape(&encode(s));
        self.cur.push_str(&format!(
            "{} {} {} rg BT /F{font} {size:.1} Tf {x:.2} {y:.2} Td ({esc}) Tj ET\n",
            color.0, color.1, color.2
        ));
    }

    fn text_right(&mut self, s: &str, x_right: f64, y: f64, font: u8, size: f64, color: Rgb) {
        let w = measure(s, font, size);
        self.text(s, x_right - w, y, font, size, color);
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, fill: Rgb) {
        self.cur.push_str(&format!(
            "{} {} {} rg {x:.2} {y:.2} {w:.2} {h:.2} re f\n",
            fill.0, fill.1, fill.2
        ));
    }

    fn rect_stroke(&mut self, x: f64, y: f64, w: f64, h: f64, lw: f64, color: Rgb) {
        self.cur.push_str(&format!(
            "{} {} {} RG {lw} w {x:.2} {y:.2} {w:.2} {h:.2} re S\n",
            color.0, color.1, color.2
        ));
    }

    fn hline(&mut self, x1: f64, x2: f64, y: f64, lw: f64, color: Rgb) {
        self.cur.push_str(&format!(
            "{} {} {} RG {lw} w {x1:.2} {y:.2} m {x2:.2} {y:.2} l S\n",
            color.0, color.1, color.2
        ));
    }

    fn polyline(&mut self, pts: &[(f64, f64)], lw: f64, color: Rgb) {
        if pts.len() < 2 {
            return;
        }
        self.cur.push_str(&format!(
            "{} {} {} RG {lw} w 1 j 1 J {:.2} {:.2} m ",
            color.0, color.1, color.2, pts[0].0, pts[0].1
        ));
        for p in &pts[1..] {
            self.cur.push_str(&format!("{:.2} {:.2} l ", p.0, p.1));
        }
        self.cur.push_str("S\n");
    }

    fn polygon_fill(&mut self, pts: &[(f64, f64)], fill: Rgb) {
        if pts.len() < 3 {
            return;
        }
        self.cur.push_str(&format!(
            "{} {} {} rg {:.2} {:.2} m ",
            fill.0, fill.1, fill.2, pts[0].0, pts[0].1
        ));
        for p in &pts[1..] {
            self.cur.push_str(&format!("{:.2} {:.2} l ", p.0, p.1));
        }
        self.cur.push_str("h f\n");
    }

    /// Word-wrapped paragraph; advances the cursor. Returns nothing useful.
    fn wrapped_text(
        &mut self,
        s: &str,
        x: f64,
        max_w: f64,
        font: u8,
        size: f64,
        color: Rgb,
        line_h: f64,
    ) {
        for line in wrap(s, font, size, max_w) {
            self.ensure(line_h);
            self.y -= line_h;
            self.text(&line, x, self.y, font, size, color);
        }
    }

    // ── report pieces ──
    fn title(&mut self, r: &Report) {
        self.y -= 14.0;
        self.text(&r.title, MARGIN, self.y, BOLD, 19.0, TEXT);
        self.y -= 8.0;
        if let Some(sub) = &r.subtitle {
            self.y -= 13.0;
            self.text(sub, MARGIN, self.y, REGULAR, 10.5, MUTED);
        }
        if !r.meta.is_empty() {
            self.y -= 13.0;
            let line = r
                .meta
                .iter()
                .map(|(k, v)| format!("{}: {v}", k.replace('_', " ")))
                .collect::<Vec<_>>()
                .join("   ·   ");
            for l in wrap(&line, REGULAR, 8.0, PAGE_W - 2.0 * MARGIN) {
                self.text(&l, MARGIN, self.y, REGULAR, 8.0, MUTED);
                self.y -= 10.5;
            }
            self.y += 10.5;
        }
        self.y -= 14.0;
        self.hline(MARGIN, PAGE_W - MARGIN, self.y, 1.4, ACCENT);
        self.y -= 6.0;
    }

    fn section(&mut self, s: &super::Section) {
        if !s.heading.is_empty() {
            // Keep the heading attached to the first ~40pt of its content.
            self.ensure(64.0);
            let size = if s.level >= 3 { 10.5 } else { 12.5 };
            self.y -= size + 12.0;
            self.text(&s.heading, MARGIN, self.y, BOLD, size, TEXT);
            self.y -= 7.0;
            self.hline(MARGIN, PAGE_W - MARGIN, self.y, 0.6, BORDER);
            self.y -= 4.0;
        }
        for b in &s.blocks {
            match b {
                Block::Paragraph(p) => {
                    self.y -= 2.0;
                    self.wrapped_text(p, MARGIN, PAGE_W - 2.0 * MARGIN, REGULAR, 9.0, TEXT, 12.5);
                    self.y -= 3.0;
                }
                Block::Bullets(items) => {
                    self.y -= 2.0;
                    for it in items {
                        let lines = wrap(it, REGULAR, 9.0, PAGE_W - 2.0 * MARGIN - 12.0);
                        for (i, l) in lines.iter().enumerate() {
                            self.ensure(12.5);
                            self.y -= 12.5;
                            if i == 0 {
                                self.text("•", MARGIN + 2.0, self.y, REGULAR, 9.0, ACCENT);
                            }
                            self.text(l, MARGIN + 12.0, self.y, REGULAR, 9.0, TEXT);
                        }
                    }
                    self.y -= 3.0;
                }
                Block::Stats(stats) => self.stats(stats),
                Block::Table(t) => self.table(t),
                Block::Chart(c) => self.chart(c),
            }
        }
    }

    /// Stat cards, three per row.
    fn stats(&mut self, stats: &[Stat]) {
        const GAP: f64 = 8.0;
        const CARD_H: f64 = 46.0;
        let avail = PAGE_W - 2.0 * MARGIN;
        let card_w = (avail - 2.0 * GAP) / 3.0;
        self.y -= 4.0;
        for row in stats.chunks(3) {
            self.ensure(CARD_H + GAP);
            let top = self.y;
            for (i, st) in row.iter().enumerate() {
                let x = MARGIN + i as f64 * (card_w + GAP);
                self.rect(x, top - CARD_H, card_w, CARD_H, CARD_BG);
                self.rect_stroke(x, top - CARD_H, card_w, CARD_H, 0.7, BORDER);
                self.text(&st.label.to_uppercase(), x + 9.0, top - 15.0, BOLD, 6.6, MUTED);
                let vcolor = match st.tone {
                    Tone::Positive => GREEN,
                    Tone::Negative => RED,
                    Tone::Muted => MUTED,
                    Tone::Neutral => TEXT,
                };
                let mut vsize = 13.0;
                while measure(&st.value, BOLD, vsize) > card_w - 18.0 && vsize > 7.5 {
                    vsize -= 0.5;
                }
                self.text(&st.value, x + 9.0, top - 33.0, BOLD, vsize, vcolor);
                if let Some(h) = &st.hint {
                    let vw = measure(&st.value, BOLD, vsize);
                    let mut hint = h.clone();
                    truncate_to(&mut hint, REGULAR, 7.0, card_w - 18.0 - vw - 6.0);
                    self.text(&hint, x + 9.0 + vw + 6.0, top - 33.0, REGULAR, 7.0, MUTED);
                }
            }
            self.y = top - CARD_H - GAP;
        }
        self.y -= 2.0;
    }

    fn table(&mut self, t: &Table) {
        const PAD: f64 = 6.0;
        const ROW_H: f64 = 15.5;
        const HEAD_H: f64 = 16.0;
        let avail = PAGE_W - 2.0 * MARGIN;
        let ncols = t.headers.len().max(1);

        // Natural column widths from content, then scale to fill the page width.
        let mut widths: Vec<f64> = (0..ncols)
            .map(|i| {
                let mut w = measure(t.headers.get(i).map(String::as_str).unwrap_or(""), BOLD, 7.6);
                for row in &t.rows {
                    if let Some(c) = row.get(i) {
                        w = w.max(measure(&c.text, REGULAR, 8.4));
                    }
                }
                w + 2.0 * PAD
            })
            .collect();
        let total: f64 = widths.iter().sum();
        let scale = avail / total;
        for w in &mut widths {
            *w = (*w * scale).max(30.0);
        }
        // Renormalize in case the 30pt floor pushed us over.
        let total: f64 = widths.iter().sum();
        if total > avail {
            let k = avail / total;
            for w in &mut widths {
                *w *= k;
            }
        }

        let draw_header = |d: &mut Doc| {
            d.y -= HEAD_H;
            let mut x = MARGIN;
            for (i, h) in t.headers.iter().enumerate() {
                let mut txt = h.to_uppercase();
                truncate_to(&mut txt, BOLD, 7.6, widths[i] - 2.0 * PAD);
                match t.aligns.get(i).copied().unwrap_or_default() {
                    Align::Right => {
                        d.text_right(&txt, x + widths[i] - PAD, d.y + 4.5, BOLD, 7.6, MUTED)
                    }
                    Align::Left => d.text(&txt, x + PAD, d.y + 4.5, BOLD, 7.6, MUTED),
                }
                x += widths[i];
            }
            d.hline(MARGIN, PAGE_W - MARGIN, d.y, 0.9, (0.62, 0.66, 0.72));
        };

        self.y -= 4.0;
        self.ensure(HEAD_H + ROW_H * 2.0);
        draw_header(self);

        for (ri, row) in t.rows.iter().enumerate() {
            if self.y - ROW_H < MARGIN + FOOTER_H {
                self.new_page();
                self.y -= 6.0;
                draw_header(self);
            }
            self.y -= ROW_H;
            if ri % 2 == 1 {
                self.rect(MARGIN, self.y, avail, ROW_H, STRIPE);
            }
            let mut x = MARGIN;
            for (i, c) in row.iter().enumerate() {
                if i >= ncols {
                    break;
                }
                let color = match c.tone {
                    Tone::Positive => GREEN,
                    Tone::Negative => RED,
                    Tone::Muted => MUTED,
                    Tone::Neutral => TEXT,
                };
                let mut txt = c.text.clone();
                truncate_to(&mut txt, REGULAR, 8.4, widths[i] - 2.0 * PAD);
                match t.aligns.get(i).copied().unwrap_or_default() {
                    Align::Right => {
                        self.text_right(&txt, x + widths[i] - PAD, self.y + 4.5, REGULAR, 8.4, color)
                    }
                    Align::Left => self.text(&txt, x + PAD, self.y + 4.5, REGULAR, 8.4, color),
                }
                x += widths[i];
            }
            self.hline(MARGIN, PAGE_W - MARGIN, self.y, 0.4, BORDER);
        }
        self.y -= 8.0;
    }

    fn chart(&mut self, c: &Chart) {
        const CHART_H: f64 = 170.0;
        if c.points.len() < 2 {
            self.wrapped_text(
                "Not enough data points to draw a curve.",
                MARGIN,
                PAGE_W - 2.0 * MARGIN,
                OBLIQUE,
                8.5,
                MUTED,
                12.0,
            );
            return;
        }
        self.ensure(CHART_H + 34.0);
        self.y -= 8.0;
        let top = self.y;
        let x0 = MARGIN + 44.0;
        let x1 = PAGE_W - MARGIN - 4.0;
        let y1 = top; // chart top
        let y0 = top - CHART_H; // chart bottom

        let (mut ymin, mut ymax) = c.points.iter().fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(lo, hi), p| (lo.min(p.1), hi.max(p.1)),
        );
        if let Some(b) = c.baseline {
            ymin = ymin.min(b);
            ymax = ymax.max(b);
        }
        if (ymax - ymin).abs() < f64::EPSILON {
            ymin -= 1.0;
            ymax += 1.0;
        }
        // Pad the range a touch so the curve doesn't touch the frame.
        let pad = (ymax - ymin) * 0.06;
        ymin -= pad;
        ymax += pad;
        let (xmin, xmax) = (c.points[0].0, c.points[c.points.len() - 1].0);
        let xspan = (xmax - xmin).max(f64::EPSILON);

        let px = |x: f64| x0 + (x - xmin) / xspan * (x1 - x0);
        let py = |y: f64| y0 + (y - ymin) / (ymax - ymin) * (y1 - y0);

        // Frame + horizontal gridlines with y labels at "nice" steps.
        self.rect_stroke(x0, y0, x1 - x0, y1 - y0, 0.7, BORDER);
        let step = nice_step((ymax - ymin) / 4.5);
        let mut tick = (ymin / step).ceil() * step;
        while tick <= ymax {
            let yy = py(tick);
            if (yy - y0).abs() > 2.0 && (yy - y1).abs() > 2.0 {
                self.hline(x0, x1, yy, 0.4, BORDER);
            }
            self.text_right(&compact_num(tick), x0 - 5.0, yy - 2.4, REGULAR, 7.0, MUTED);
            tick += step;
        }

        // Baseline (e.g. zero line) emphasized.
        if let Some(b) = c.baseline {
            if b >= ymin && b <= ymax {
                self.hline(x0, x1, py(b), 0.9, (0.62, 0.66, 0.72));
            }
        }

        // X labels: 4 evenly spaced positions.
        for i in 0..4 {
            let fx = xmin + xspan * (i as f64 / 3.0);
            let label = if c.time_axis { fmt_ts(fx, xspan) } else { compact_num(fx) };
            let lx = px(fx);
            let w = measure(&label, REGULAR, 7.0);
            let tx = (lx - w / 2.0).clamp(x0, x1 - w);
            self.text(&label, tx, y0 - 11.0, REGULAR, 7.0, MUTED);
        }

        // Area fill down to the baseline (or the chart floor), then the line.
        let floor = py(c.baseline.unwrap_or(ymin).clamp(ymin, ymax));
        let mut poly: Vec<(f64, f64)> = vec![(px(c.points[0].0), floor)];
        let line: Vec<(f64, f64)> = c.points.iter().map(|p| (px(p.0), py(p.1))).collect();
        poly.extend(line.iter().copied());
        poly.push((px(xmax), floor));
        self.polygon_fill(&poly, ACCENT_SOFT);
        self.polyline(&line, 1.3, ACCENT);

        // End-value marker + label.
        let (ex, ey) = *line.last().unwrap();
        self.rect(ex - 1.6, ey - 1.6, 3.2, 3.2, ACCENT);
        let last = c.points[c.points.len() - 1].1;
        let lbl = super::fmt_num(last, 2);
        let ly = (ey + 4.0).min(y1 - 8.0).max(y0 + 3.0);
        self.text_right(&lbl, x1 - 2.0, ly, BOLD, 7.5, ACCENT);

        // Axis caption.
        self.text(&c.y_label, x0, y1 + 4.0, REGULAR, 7.5, MUTED);

        self.y = y0 - 24.0;
    }

    /// Assemble the final PDF: pages, fonts, footers, xref.
    fn finish(mut self, title: &str) -> Vec<u8> {
        self.pages.push(std::mem::take(&mut self.cur));
        let total = self.pages.len();

        // Page footers (need the total count, so added last).
        for (i, page) in self.pages.iter_mut().enumerate() {
            let mut f = String::new();
            f.push_str(&format!(
                "{} {} {} RG 0.6 w {MARGIN} {:.2} m {:.2} {:.2} l S\n",
                BORDER.0,
                BORDER.1,
                BORDER.2,
                MARGIN + FOOTER_H - 8.0,
                PAGE_W - MARGIN,
                MARGIN + FOOTER_H - 8.0
            ));
            let mut left = format!("OpenTraderWorld — {title}");
            truncate_to(&mut left, REGULAR, 7.0, PAGE_W - 2.0 * MARGIN - 70.0);
            f.push_str(&format!(
                "{} {} {} rg BT /F1 7 Tf {MARGIN} {:.2} Td ({}) Tj ET\n",
                MUTED.0,
                MUTED.1,
                MUTED.2,
                MARGIN + FOOTER_H - 19.0,
                escape(&encode(&left))
            ));
            let right = format!("Page {} / {total}", i + 1);
            let rw = measure(&right, REGULAR, 7.0);
            f.push_str(&format!(
                "{} {} {} rg BT /F1 7 Tf {:.2} {:.2} Td ({}) Tj ET\n",
                MUTED.0,
                MUTED.1,
                MUTED.2,
                PAGE_W - MARGIN - rw,
                MARGIN + FOOTER_H - 19.0,
                escape(&encode(&right))
            ));
            page.push_str(&f);
        }

        // Objects: 1 catalog, 2 pages, 3-5 fonts, then (page, content) pairs.
        let mut objs: Vec<(usize, Vec<u8>)> = Vec::new();
        let kids: Vec<String> =
            (0..total).map(|i| format!("{} 0 R", 6 + i * 2)).collect();
        objs.push((1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()));
        objs.push((
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {total} >>",
                kids.join(" ")
            )
            .into_bytes(),
        ));
        for (n, base) in [(3, "Helvetica"), (4, "Helvetica-Bold"), (5, "Helvetica-Oblique")] {
            objs.push((
                n,
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>"
                )
                .into_bytes(),
            ));
        }
        for (i, page) in self.pages.iter().enumerate() {
            let pnum = 6 + i * 2;
            objs.push((
                pnum,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] \
                     /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> \
                     /Contents {} 0 R >>",
                    pnum + 1
                )
                .into_bytes(),
            ));
            let stream = page.as_bytes();
            let mut content =
                format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
            content.extend_from_slice(stream);
            content.extend_from_slice(b"\nendstream");
            objs.push((pnum + 1, content));
        }

        let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0usize; objs.len() + 1];
        for (num, body) in &objs {
            offsets[*num] = out.len();
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objs.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for off in &offsets[1..] {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

// ── Text metrics & encoding ───────────────────────────────────────────────────

/// Helvetica advance widths (per mille) for chars 32..=126.
#[rustfmt::skip]
const W_REG: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Helvetica-Bold advance widths (per mille) for chars 32..=126.
#[rustfmt::skip]
const W_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Approximate string width in points.
fn measure(s: &str, font: u8, size: f64) -> f64 {
    let table = if font == BOLD { &W_BOLD } else { &W_REG };
    let mille: u32 = encode(s)
        .bytes()
        .map(|b| {
            if (32..=126).contains(&b) {
                table[(b - 32) as usize] as u32
            } else {
                // Latin-1 supplement ≈ lowercase width; good enough for layout.
                if font == BOLD { 585 } else { 546 }
            }
        })
        .sum();
    mille as f64 / 1000.0 * size
}

/// Map to WinAnsi (CP1252): ASCII + Latin-1 pass through, common typographic
/// chars mapped, everything else becomes '?'.
fn encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{20}'..='\u{7e}' => c,
            '\u{a0}'..='\u{ff}' => c, // Latin-1 == WinAnsi in this range
            '€' => '\u{80}',
            '’' | '‘' => '\'',
            '“' | '”' => '"',
            '–' | '—' | '−' => '-',
            '…' => '\u{85}',
            '•' => '\u{b7}',
            '→' => '>',
            _ => '?',
        })
        .collect()
}

/// Escape a (already WinAnsi-mapped) string for a PDF literal, byte-wise.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        let b = if (c as u32) < 256 { c as u32 as u8 } else { b'?' };
        match b {
            b'\\' => out.push_str("\\\\"),
            b'(' => out.push_str("\\("),
            b')' => out.push_str("\\)"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{:03o}", b)),
        }
    }
    out
}

/// Greedy word wrap by measured width.
fn wrap(s: &str, font: u8, size: f64, max_w: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if measure(&cand, font, size) <= max_w || cur.is_empty() {
            cur = cand;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Truncate `s` in place (with an ellipsis) so it fits `max_w`.
fn truncate_to(s: &mut String, font: u8, size: f64, max_w: f64) {
    if measure(s, font, size) <= max_w {
        return;
    }
    while !s.is_empty() && measure(&format!("{s}…"), font, size) > max_w {
        s.pop();
    }
    s.push('…');
}

/// A "nice" tick step (1/2/2.5/5 × 10^k) near `raw`.
fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    let nice = if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 2.5 {
        2.5
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// Compact axis number: 12.3k / 4.5M below/above thousand-scale.
fn compact_num(v: f64) -> String {
    let a = v.abs();
    if a >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if a >= 10_000.0 {
        format!("{:.1}k", v / 1_000.0)
    } else if a >= 100.0 {
        format!("{v:.0}")
    } else {
        super::fmt_num(v, 2)
    }
}

/// Format a unix-seconds x value as a date label; include the year on long spans.
fn fmt_ts(secs: f64, span_secs: f64) -> String {
    use time::macros::format_description;
    let short = format_description!("[month repr:short] [day padding:none]");
    let long = format_description!("[month repr:short] [day padding:none], [year]");
    match time::OffsetDateTime::from_unix_timestamp(secs as i64) {
        Ok(dt) => {
            let f = if span_secs > 300.0 * 86_400.0 { long } else { short };
            dt.format(f).unwrap_or_else(|_| String::from("?"))
        }
        Err(_) => String::from("?"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Block, Cell, Chart, Report, Section, Stat, Table, Tone};

    #[test]
    fn produces_wellformed_pdf() {
        let mut r = Report::new("Test report");
        r.subtitle = Some("Jan 1 – Jan 31, 2026 · All categories".into());
        r.meta.push(("period".into(), "2026-01".into()));
        let mut s = Section::new("Overview");
        s.blocks.push(Block::Stats(vec![
            Stat::new("Net PnL", "1,234.50 USD").signed(1234.5),
            Stat::new("Win rate", "58.3%"),
            Stat::new("Trades", "12").hint("10 closed · 2 open"),
        ]));
        s.blocks.push(Block::Chart(Chart {
            y_label: "Cumulative net PnL (USD)".into(),
            points: (0..50)
                .map(|i| (1_760_000_000.0 + i as f64 * 86_400.0, (i as f64 * 0.7).sin() * 500.0 + i as f64 * 20.0))
                .collect(),
            baseline: Some(0.0),
            time_axis: true,
        }));
        s.blocks.push(Block::Table(Table {
            headers: vec!["Strategy".into(), "Trades".into(), "Net PnL".into()],
            aligns: vec![crate::report::Align::Left, crate::report::Align::Right, crate::report::Align::Right],
            rows: (0..80)
                .map(|i| {
                    vec![
                        Cell::new(format!("Strategy {i} (éàü€)")),
                        Cell::new(format!("{i}")),
                        Cell::toned("-12.00", Tone::Negative),
                    ]
                })
                .collect(),
        }));
        r.sections.push(s);
        r.footer_note = Some("Generated by OpenTraderWorld.".into());

        let pdf = render(&r);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("Helvetica-Bold"));
        // The 80-row table must have forced at least a second page.
        assert!(text.matches("/Type /Page ").count() >= 2);
        // xref offset points at the xref table.
        let sx = text.rfind("startxref\n").unwrap();
        let off: usize = text[sx + 10..].lines().next().unwrap().trim().parse().unwrap();
        assert_eq!(&pdf[off..off + 4], b"xref");
    }
}
