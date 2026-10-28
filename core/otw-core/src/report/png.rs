//! PNG renderer for the shared report model's [`Chart`].
//!
//! Same picture as the PDF chart, at a resolution a chat client can show inline:
//! the geometry, the palette and the axis helpers are the ones in the parent
//! module, so a curve does not change shape depending on which file it landed
//! in. Where the PDF emits vector operators, this walks pixels.
//!
//! Hand-rolled end to end, like the PDF writer and the MCP server: a rasteriser
//! (coverage-based lines, a column-wise area fill, a 5×7 bitmap face) plus a
//! minimal PNG encoder (adaptive row filters, fixed-Huffman deflate, CRC32 and
//! Adler-32). The alternative was a headless browser in the `core` image to
//! screenshot the SPA's chart, which is two orders of magnitude more moving
//! parts for a picture this simple.

use super::font;
use super::{compact_num, fmt_num, fmt_ts, nice_step, Chart, Rgb};
use super::{ACCENT, ACCENT_SOFT, BORDER, MUTED, PAPER};

/// Default bitmap size: wide enough for four x labels at the small face, short
/// enough to stay under a chat client's inline-preview scaling.
pub const SIZE: (u32, u32) = (900, 420);

/// Clamp, so a caller-supplied size can never ask for a gigabyte of canvas.
const MIN_SIDE: u32 = 200;
const MAX_SIDE: u32 = 4000;

/// Render one chart as a complete PNG byte stream.
///
/// Always returns a valid image: a series too short to draw becomes a framed
/// panel carrying the reason, which is what a chat reply needs (an error the
/// user can see beats an attachment that failed to arrive).
pub fn render_chart(c: &Chart, w: u32, h: u32) -> Vec<u8> {
    let w = w.clamp(MIN_SIDE, MAX_SIDE) as usize;
    let h = h.clamp(MIN_SIDE, MAX_SIDE) as usize;
    let mut cv = Canvas::new(w, h, PAPER);
    draw(&mut cv, c);
    encode(&cv)
}

// ── Chart ───────────────────────────────────────────────────────────────────

/// Outer gutter, all four sides.
const PAD: f64 = 14.0;

fn draw(cv: &mut Canvas, c: &Chart) {
    // Small canvases get the one-pixel face; anything report-sized gets the
    // doubled one, which is the readable size on a phone screenshot.
    let s = if cv.h >= 300 && cv.w >= 520 { 2 } else { 1 };
    let cap_h = (font::H * s) as f64;

    if c.points.len() < 2 {
        let msg = "Not enough data points to draw a curve.";
        let x = (cv.w as f64 - font::width(msg, s) as f64) / 2.0;
        cv.rect_stroke(
            PAD,
            PAD,
            cv.w as f64 - 2.0 * PAD,
            cv.h as f64 - 2.0 * PAD,
            1.0,
            BORDER,
        );
        cv.text(msg, x, (cv.h as f64 - cap_h) / 2.0, s, MUTED);
        return;
    }

    // Y range: the series, widened to include the baseline, then padded so the
    // curve never rides the frame. Mirrors the PDF renderer exactly.
    let (mut ymin, mut ymax) = c
        .points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.1), hi.max(p.1))
        });
    if !ymin.is_finite() || !ymax.is_finite() {
        ymin = 0.0;
        ymax = 1.0;
    }
    if let Some(b) = c.baseline {
        ymin = ymin.min(b);
        ymax = ymax.max(b);
    }
    if (ymax - ymin).abs() < f64::EPSILON {
        ymin -= 1.0;
        ymax += 1.0;
    }
    let pad = (ymax - ymin) * 0.06;
    ymin -= pad;
    ymax += pad;

    // Ticks are measured before the plot box is placed: the y gutter is exactly
    // as wide as the widest label, so a chart of millions is not cropped and a
    // chart of percentages does not waste a third of its width.
    let step = nice_step((ymax - ymin) / 4.5);
    let mut ticks = Vec::new();
    let mut t = (ymin / step).ceil() * step;
    while t <= ymax && ticks.len() < 24 {
        ticks.push((t, compact_num(t)));
        t += step;
    }
    let gutter = ticks
        .iter()
        .map(|(_, l)| font::width(l, s))
        .max()
        .unwrap_or(0) as f64;

    let x0 = PAD + gutter + 6.0;
    let x1 = cv.w as f64 - PAD - 2.0;
    let y1 = PAD + cap_h + 6.0; // plot top
    let y0 = cv.h as f64 - PAD - cap_h - 6.0; // plot bottom
    if x1 - x0 < 20.0 || y0 - y1 < 20.0 {
        return; // nothing legible fits; a blank frame is the honest output
    }

    let (xmin, xmax) = (c.points[0].0, c.points[c.points.len() - 1].0);
    let xspan = (xmax - xmin).max(f64::EPSILON);
    let px = |x: f64| x0 + (x - xmin) / xspan * (x1 - x0);
    // Screen y grows downward, so the value axis is flipped here, not in the
    // shared range maths.
    let py = |y: f64| y0 - (y - ymin) / (ymax - ymin) * (y0 - y1);

    // Caption, top left, on the gutter line so it reads as the y axis title.
    let cap = font::truncate(&c.y_label, s, (x1 - PAD) as usize);
    cv.text(&cap, PAD, PAD, s, MUTED);

    cv.rect_stroke(x0, y1, x1 - x0, y0 - y1, 1.0, BORDER);
    for (v, label) in &ticks {
        let yy = py(*v);
        if (yy - y0).abs() > 2.0 && (yy - y1).abs() > 2.0 {
            cv.hline(x0, x1, yy, 1.0, BORDER);
        }
        let ty = yy - (font::H * s) as f64 / 2.0;
        cv.text(label, x0 - 5.0 - font::width(label, s) as f64, ty, s, MUTED);
    }

    if let Some(b) = c.baseline {
        if b >= ymin && b <= ymax {
            cv.hline(x0, x1, py(b), 1.4, (0.62, 0.66, 0.72));
        }
    }

    // X labels: four evenly spaced positions, clamped inside the plot box.
    for i in 0..4 {
        let fx = xmin + xspan * (i as f64 / 3.0);
        let label = if c.time_axis {
            fmt_ts(fx, xspan)
        } else {
            compact_num(fx)
        };
        let lw = font::width(&label, s) as f64;
        let tx = (px(fx) - lw / 2.0).clamp(PAD, cv.w as f64 - PAD - lw);
        cv.text(&label, tx, y0 + 6.0, s, MUTED);
    }

    let line: Vec<(f64, f64)> = c.points.iter().map(|p| (px(p.0), py(p.1))).collect();
    let floor = py(c.baseline.unwrap_or(ymin).clamp(ymin, ymax));
    cv.area(&line, floor, y1, y0, ACCENT_SOFT);
    cv.polyline(&line, 1.8, ACCENT);

    // End-value marker, then its label. The label sits on a small patch of
    // ground: at the right edge it lands wherever the curve happens to be, and
    // blue-on-blue is the one place this chart becomes unreadable.
    let (ex, ey) = *line.last().unwrap();
    cv.fill_rect(ex - 2.0, ey - 2.0, 4.0, 4.0, ACCENT);
    let last = fmt_num(c.points[c.points.len() - 1].1, 2);
    let lw = font::width(&last, s) as f64;
    let lh = (font::H * s) as f64;
    let lx = (ex - 6.0 - lw).max(x0 + 3.0);
    let ly = (ey - lh / 2.0).clamp(y1 + 3.0, y0 - lh - 3.0);
    cv.fill_rect(lx - 3.0, ly - 2.0, lw + 6.0, lh + 4.0, PAPER);
    cv.text(&last, lx, ly, s, ACCENT);
}

// ── Raster canvas ───────────────────────────────────────────────────────────

struct Canvas {
    w: usize,
    h: usize,
    /// Row-major RGB8, no alpha: everything composites onto an opaque ground.
    px: Vec<u8>,
}

fn quant(c: Rgb) -> [u8; 3] {
    [
        (c.0.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.1.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.2.clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

impl Canvas {
    fn new(w: usize, h: usize, bg: Rgb) -> Self {
        let c = quant(bg);
        let mut px = Vec::with_capacity(w * h * 3);
        for _ in 0..w * h {
            px.extend_from_slice(&c);
        }
        Canvas { w, h, px }
    }

    /// Source-over with a scalar coverage. Coverage is how the renderer gets its
    /// smooth edges: nothing is drawn as a hard pixel except glyphs.
    fn blend(&mut self, x: isize, y: isize, c: [u8; 3], a: f64) {
        if a <= 0.0 || x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize {
            return;
        }
        let a = a.min(1.0);
        let i = (y as usize * self.w + x as usize) * 3;
        for k in 0..3 {
            let dst = self.px[i + k] as f64;
            self.px[i + k] = (dst + (c[k] as f64 - dst) * a).round() as u8;
        }
    }

    /// Axis-aligned fill with exact edge coverage, so a 1.4px rule looks like
    /// 1.4px instead of snapping to 1 or 2.
    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Rgb) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let c = quant(color);
        let (x2, y2) = (x + w, y + h);
        for py in y.floor().max(0.0) as isize..=(y2.ceil() as isize).min(self.h as isize) {
            let cy = ((py as f64 + 1.0).min(y2) - (py as f64).max(y)).clamp(0.0, 1.0);
            if cy <= 0.0 {
                continue;
            }
            for pxx in x.floor().max(0.0) as isize..=(x2.ceil() as isize).min(self.w as isize) {
                let cx = ((pxx as f64 + 1.0).min(x2) - (pxx as f64).max(x)).clamp(0.0, 1.0);
                self.blend(pxx, py, c, cx * cy);
            }
        }
    }

    fn hline(&mut self, x1: f64, x2: f64, y: f64, lw: f64, color: Rgb) {
        self.fill_rect(x1, y - lw / 2.0, x2 - x1, lw, color);
    }

    fn rect_stroke(&mut self, x: f64, y: f64, w: f64, h: f64, lw: f64, color: Rgb) {
        self.fill_rect(x - lw / 2.0, y - lw / 2.0, w + lw, lw, color);
        self.fill_rect(x - lw / 2.0, y + h - lw / 2.0, w + lw, lw, color);
        self.fill_rect(x - lw / 2.0, y - lw / 2.0, lw, h + lw, color);
        self.fill_rect(x + w - lw / 2.0, y - lw / 2.0, lw, h + lw, color);
    }

    /// One segment, coverage from the distance to its axis: round caps, which is
    /// what keeps a polyline's joints from notching.
    fn segment(&mut self, p: (f64, f64), q: (f64, f64), lw: f64, c: [u8; 3]) {
        let hw = lw / 2.0;
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len2 = dx * dx + dy * dy;
        let lo_x = (p.0.min(q.0) - hw - 1.0).floor().max(0.0) as isize;
        let hi_x = (p.0.max(q.0) + hw + 1.0).ceil().min(self.w as f64) as isize;
        let lo_y = (p.1.min(q.1) - hw - 1.0).floor().max(0.0) as isize;
        let hi_y = (p.1.max(q.1) + hw + 1.0).ceil().min(self.h as f64) as isize;
        for y in lo_y..hi_y {
            for x in lo_x..hi_x {
                let (sx, sy) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = if len2 > f64::EPSILON {
                    (((sx - p.0) * dx + (sy - p.1) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (nx, ny) = (p.0 + t * dx - sx, p.1 + t * dy - sy);
                let d = (nx * nx + ny * ny).sqrt();
                self.blend(x, y, c, (hw + 0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    fn polyline(&mut self, pts: &[(f64, f64)], lw: f64, color: Rgb) {
        let c = quant(color);
        for w in pts.windows(2) {
            self.segment(w[0], w[1], lw, c);
        }
    }

    /// Fill between a single-valued curve and a horizontal floor.
    ///
    /// A general polygon scanline would work, but the series here is a function
    /// of x by construction, so one interpolated value per pixel column is both
    /// exact and an order of magnitude less code.
    fn area(&mut self, pts: &[(f64, f64)], floor: f64, top: f64, bottom: f64, color: Rgb) {
        let mut curve = vec![f64::NAN; self.w];
        for seg in pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
            let from = lo.0.floor().max(0.0) as usize;
            let to = (hi.0.ceil() as usize).min(self.w.saturating_sub(1));
            for x in from..=to {
                let cx = (x as f64 + 0.5).clamp(lo.0, hi.0);
                let t = if (hi.0 - lo.0).abs() > f64::EPSILON {
                    (cx - lo.0) / (hi.0 - lo.0)
                } else {
                    0.0
                };
                curve[x] = lo.1 + (hi.1 - lo.1) * t;
            }
        }
        let c = quant(color);
        for (x, v) in curve.iter().enumerate() {
            if v.is_nan() {
                continue;
            }
            let (mut a, mut b) = (v.min(floor), v.max(floor));
            a = a.max(top);
            b = b.min(bottom);
            if b <= a {
                continue;
            }
            for y in a.floor() as isize..=b.ceil() as isize {
                let cov = ((y as f64 + 1.0).min(b) - (y as f64).max(a)).clamp(0.0, 1.0);
                self.blend(x as isize, y, c, cov);
            }
        }
    }

    /// Text at `scale` device pixels per font pixel, `(x, y)` the cell's top left.
    /// Glyphs are drawn hard-edged on purpose: at this size, anti-aliasing a
    /// one-pixel stem turns it into a grey smear.
    fn text(&mut self, s: &str, x: f64, y: f64, scale: usize, color: Rgb) {
        let c = quant(color);
        let (ox, oy) = (x.round() as isize, y.round() as isize);
        for (i, ch) in s.chars().enumerate() {
            let gx = ox + (i * font::ADVANCE * scale) as isize;
            for (row, bits) in font::glyph(ch).iter().enumerate() {
                if *bits == 0 {
                    continue;
                }
                for col in 0..font::W {
                    if bits & (1 << (font::W - 1 - col)) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = gx + (col * scale + dx) as isize;
                            let y = oy + (row * scale + dy) as isize;
                            self.blend(x, y, c, 1.0);
                        }
                    }
                }
            }
        }
    }
}

// ── PNG container ───────────────────────────────────────────────────────────

fn encode(cv: &Canvas) -> Vec<u8> {
    let raw = filter_rows(cv);
    let z = zlib(&raw);

    let mut out = Vec::with_capacity(z.len() + 64);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(cv.w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(cv.h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, truecolour RGB, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = crc32(0xffff_ffff, kind);
    crc = crc32(crc, data);
    out.extend_from_slice(&(crc ^ 0xffff_ffff).to_be_bytes());
}

const BPP: usize = 3;

/// Per-row filter, picked by the usual minimum-sum-of-absolute-differences
/// heuristic. It is what turns a flat background into a run of zeroes, which is
/// where nearly all of the compression on a chart comes from.
fn filter_rows(cv: &Canvas) -> Vec<u8> {
    let stride = cv.w * BPP;
    let mut out = Vec::with_capacity(cv.h * (stride + 1));
    let mut cand: [Vec<u8>; 5] = std::array::from_fn(|_| Vec::with_capacity(stride));
    for y in 0..cv.h {
        let row = &cv.px[y * stride..(y + 1) * stride];
        let prev = if y == 0 {
            None
        } else {
            Some(&cv.px[(y - 1) * stride..y * stride])
        };
        for c in cand.iter_mut() {
            c.clear();
        }
        for i in 0..stride {
            let a = if i >= BPP { row[i - BPP] } else { 0 };
            let b = prev.map_or(0, |p| p[i]);
            let c = if i >= BPP {
                prev.map_or(0, |p| p[i - BPP])
            } else {
                0
            };
            cand[0].push(row[i]);
            cand[1].push(row[i].wrapping_sub(a));
            cand[2].push(row[i].wrapping_sub(b));
            cand[3].push(row[i].wrapping_sub(((a as u16 + b as u16) / 2) as u8));
            cand[4].push(row[i].wrapping_sub(paeth(a, b, c)));
        }
        let best = (0..5)
            .min_by_key(|&i| {
                cand[i]
                    .iter()
                    .map(|b| (*b as i8).unsigned_abs() as u64)
                    .sum::<u64>()
            })
            .unwrap();
        out.push(best as u8);
        out.extend_from_slice(&cand[best]);
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = (
        (p - a as i16).abs(),
        (p - b as i16).abs(),
        (p - c as i16).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01]; // deflate, 32K window, no preset dict
    out.extend_from_slice(&deflate(raw));
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

// Fixed-Huffman deflate. A dynamic tree would shave a few more percent off, at
// the cost of the whole code-length-code machinery; on an image that is mostly
// filtered-to-zero runs, the fixed tree is already within noise of it.

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const WINDOW: usize = 32768;
const HASH_BITS: usize = 15;
/// How far back a hash chain is followed. Deeper finds slightly longer matches
/// and costs linearly; a chart is drawn once and sent, so this stays modest.
const MAX_CHAIN: usize = 64;

struct BitW {
    out: Vec<u8>,
    cur: u32,
    n: u32,
}

impl BitW {
    /// Deflate packs bit fields low bit first within each byte.
    fn bits(&mut self, v: u32, n: u32) {
        self.cur |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push((self.cur & 0xff) as u8);
            self.cur >>= 8;
            self.n -= 8;
        }
    }
    /// Huffman codes are the exception: they go out most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        for i in (0..len).rev() {
            self.bits((code >> i) & 1, 1);
        }
    }
    fn literal(&mut self, v: u16) {
        match v {
            0..=143 => self.code(0x30 + v as u32, 8),
            144..=255 => self.code(0x190 + v as u32 - 144, 9),
            256..=279 => self.code(v as u32 - 256, 7),
            _ => self.code(0xc0 + v as u32 - 280, 8),
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push((self.cur & 0xff) as u8);
        }
        self.out
    }
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut bw = BitW {
        out: Vec::with_capacity(data.len() / 4 + 16),
        cur: 0,
        n: 0,
    };
    bw.bits(1, 1); // BFINAL
    bw.bits(1, 2); // BTYPE = fixed Huffman

    let mask = (1 << HASH_BITS) - 1;
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; data.len()];
    let hash = |d: &[u8], i: usize| -> usize {
        ((d[i] as usize) << 10 ^ (d[i + 1] as usize) << 5 ^ d[i + 2] as usize) & mask
    };

    let mut i = 0usize;
    while i < data.len() {
        let mut best_len = 0usize;
        let mut best_dist = 0usize;
        if i + MIN_MATCH <= data.len() {
            let h = hash(data, i);
            let mut cand = head[h];
            let mut chain = MAX_CHAIN;
            while cand != usize::MAX && chain > 0 && i - cand <= WINDOW {
                let max = MAX_MATCH.min(data.len() - i);
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == max {
                        break;
                    }
                }
                cand = prev[cand];
                chain -= 1;
            }
            prev[i] = head[h];
            head[h] = i;
        }

        if best_len >= MIN_MATCH {
            let li = LEN_BASE.partition_point(|&b| b as usize <= best_len) - 1;
            bw.literal(257 + li as u16);
            if LEN_EXTRA[li] > 0 {
                bw.bits(
                    (best_len - LEN_BASE[li] as usize) as u32,
                    LEN_EXTRA[li] as u32,
                );
            }
            let di = DIST_BASE.partition_point(|&b| b as usize <= best_dist) - 1;
            bw.code(di as u32, 5);
            if DIST_EXTRA[di] > 0 {
                bw.bits(
                    (best_dist - DIST_BASE[di] as usize) as u32,
                    DIST_EXTRA[di] as u32,
                );
            }
            // Index the bytes the match covered, so later matches can start there.
            for k in i + 1..i + best_len {
                if k + MIN_MATCH <= data.len() {
                    let h = hash(data, k);
                    prev[k] = head[h];
                    head[h] = k;
                }
            }
            i += best_len;
        } else {
            bw.literal(data[i] as u16);
            i += 1;
        }
    }
    bw.literal(256); // end of block
    bw.finish()
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for byte in chunk {
            a += *byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    });
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Chart {
        Chart {
            y_label: "Cumulative net PnL (USD)".into(),
            points: (0..120)
                .map(|i| {
                    (
                        1_760_000_000.0 + i as f64 * 86_400.0,
                        (i as f64 * 0.21).sin() * 4_800.0 + i as f64 * 95.0 - 2_000.0,
                    )
                })
                .collect(),
            baseline: Some(0.0),
            time_axis: true,
        }
    }

    #[test]
    fn produces_a_wellformed_png() {
        let png = render_chart(&sample(), SIZE.0, SIZE.1);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), SIZE.0);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), SIZE.1);
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        // Every chunk's CRC checks out, walking the file the way a decoder does.
        let mut p = 8;
        let mut seen = Vec::new();
        while p + 8 <= png.len() {
            let len = u32::from_be_bytes(png[p..p + 4].try_into().unwrap()) as usize;
            let kind = &png[p + 4..p + 8];
            let crc = crc32(crc32(0xffff_ffff, kind), &png[p + 8..p + 8 + len]) ^ 0xffff_ffff;
            assert_eq!(
                crc,
                u32::from_be_bytes(png[p + 8 + len..p + 12 + len].try_into().unwrap())
            );
            seen.push(String::from_utf8_lossy(kind).to_string());
            p += 12 + len;
        }
        assert_eq!(p, png.len());
        assert_eq!(seen, ["IHDR", "IDAT", "IEND"]);
    }

    #[test]
    fn compresses_rather_than_stores() {
        // A flat-ground chart must come out far smaller than its raw pixels;
        // if the LZ77 stage ever silently degrades, this catches it.
        let png = render_chart(&sample(), SIZE.0, SIZE.1);
        let raw = SIZE.0 as usize * SIZE.1 as usize * 3;
        assert!(png.len() < raw / 10, "{} bytes for {raw} raw", png.len());
    }

    #[test]
    fn degenerate_inputs_still_render() {
        for c in [
            Chart {
                y_label: "empty".into(),
                points: vec![],
                baseline: None,
                time_axis: false,
            },
            Chart {
                y_label: "one point".into(),
                points: vec![(0.0, 1.0)],
                baseline: None,
                time_axis: true,
            },
            Chart {
                y_label: "flat".into(),
                points: (0..10).map(|i| (i as f64, 7.0)).collect(),
                baseline: Some(7.0),
                time_axis: false,
            },
            Chart {
                y_label: "not finite".into(),
                points: vec![(0.0, f64::NAN), (1.0, f64::INFINITY)],
                baseline: None,
                time_axis: false,
            },
        ] {
            let png = render_chart(&c, 320, 200);
            assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
        }
    }

    /// Write the sample chart somewhere to look at it. A rasteriser cannot be
    /// reviewed by assertion alone: `OTW_PNG_DUMP=/tmp/c.png cargo test report::png`.
    #[test]
    fn dump_sample_when_asked() {
        let Ok(path) = std::env::var("OTW_PNG_DUMP") else {
            return;
        };
        std::fs::write(path, render_chart(&sample(), SIZE.0, SIZE.1)).unwrap();
    }

    #[test]
    fn size_is_clamped() {
        let png = render_chart(&sample(), 1, 999_999);
        assert_eq!(
            u32::from_be_bytes(png[16..20].try_into().unwrap()),
            MIN_SIDE
        );
        assert_eq!(
            u32::from_be_bytes(png[20..24].try_into().unwrap()),
            MAX_SIDE
        );
    }
}
