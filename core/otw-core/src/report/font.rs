//! A 5×7 bitmap face for the PNG renderer.
//!
//! The PDF renderer gets its glyphs for free (base-14 Helvetica lives in every
//! reader), a bitmap has no such luxury: something has to carry the shapes. So
//! the face is written here as art rather than as a hex table, one line per
//! glyph, because a font you can read in the source is a font you can fix in the
//! source. Parsing it costs one pass, once, at first use.
//!
//! 5×7 is the smallest cell that still separates `8` from `B` and `1` from `l`
//! at one device pixel per cell, which is what an axis label needs. Anything
//! that is not ASCII is folded to its base letter when there is one (`é` → `e`,
//! `·` → `.`, an en dash to a hyphen) so a French caption degrades to readable
//! rather than to a row of boxes.

use std::sync::OnceLock;

/// Glyph cell, in pixels.
pub const W: usize = 5;
pub const H: usize = 7;
/// Pen movement per character: the cell plus one column of side bearing.
pub const ADVANCE: usize = W + 1;

const FIRST: char = ' ';
const LAST: char = '~';

/// One row of seven, columns left to right, `#` on and space off, rows joined by
/// `/`. Index = codepoint - 32.
#[rustfmt::skip]
const ART: [&str; 95] = [
    "     /     /     /     /     /     /     ", // space
    "  #  /  #  /  #  /  #  /  #  /     /  #  ", // !
    " # # / # # /     /     /     /     /     ", // "
    " # # / # # /#####/ # # /#####/ # # / # # ", // #
    "  #  / ####/# #  / ### /  # #/#### /  #  ", // $
    "##   /##  #/   # /  #  / #   /#  ##/   ##", // %
    " ##  /#  # / ##  / ##  /#  # /#   #/ ## #", // &
    "  #  /  #  /     /     /     /     /     ", // '
    "   # /  #  / #   / #   / #   /  #  /   # ", // (
    " #   /  #  /   # /   # /   # /  #  / #   ", // )
    "     /# # #/ ### /#####/ ### /# # #/     ", // *
    "     /  #  /  #  /#####/  #  /  #  /     ", // +
    "     /     /     /     /  ## /  ## / #   ", // ,
    "     /     /     /#####/     /     /     ", // -
    "     /     /     /     /     / ##  / ##  ", // .
    "    #/    #/   # /  #  / #   /#    /#    ", // /
    " ### /#   #/#  ##/# # #/##  #/#   #/ ### ", // 0
    "  #  / ##  /  #  /  #  /  #  /  #  / ### ", // 1
    " ### /#   #/    #/   # /  #  / #   /#####", // 2
    "#####/   # /  #  /   # /    #/#   #/ ### ", // 3
    "   # /  ## / # # /#  # /#####/   # /   # ", // 4
    "#####/#    /#### /    #/    #/#   #/ ### ", // 5
    "  ## / #   /#    /#### /#   #/#   #/ ### ", // 6
    "#####/    #/   # /  #  / #   / #   / #   ", // 7
    " ### /#   #/#   #/ ### /#   #/#   #/ ### ", // 8
    " ### /#   #/#   #/ ####/    #/   # / ##  ", // 9
    "     / ##  / ##  /     / ##  / ##  /     ", // :
    "     / ##  / ##  /     / ##  / ##  / #   ", // ;
    "   # /  #  / #   /#    / #   /  #  /   # ", // <
    "     /     /#####/     /#####/     /     ", // =
    "#    / #   /  #  /   # /  #  / #   /#    ", // >
    " ### /#   #/    #/   # /  #  /     /  #  ", // ?
    " ### /#   #/    #/ ## #/# # #/# # #/ ####", // @
    "  #  / # # /#   #/#   #/#####/#   #/#   #", // A
    "#### /#   #/#   #/#### /#   #/#   #/#### ", // B
    " ### /#   #/#    /#    /#    /#   #/ ### ", // C
    "###  /#  # /#   #/#   #/#   #/#  # /###  ", // D
    "#####/#    /#    /#### /#    /#    /#####", // E
    "#####/#    /#    /#### /#    /#    /#    ", // F
    " ### /#   #/#    /#  ##/#   #/#   #/ ####", // G
    "#   #/#   #/#   #/#####/#   #/#   #/#   #", // H
    " ### /  #  /  #  /  #  /  #  /  #  / ### ", // I
    "    #/    #/    #/    #/#   #/#   #/ ### ", // J
    "#   #/#  # /# #  /##   /# #  /#  # /#   #", // K
    "#    /#    /#    /#    /#    /#    /#####", // L
    "#   #/## ##/# # #/# # #/#   #/#   #/#   #", // M
    "#   #/##  #/##  #/# # #/#  ##/#  ##/#   #", // N
    " ### /#   #/#   #/#   #/#   #/#   #/ ### ", // O
    "#### /#   #/#   #/#### /#    /#    /#    ", // P
    " ### /#   #/#   #/#   #/# # #/#  # / ## #", // Q
    "#### /#   #/#   #/#### /# #  /#  # /#   #", // R
    " ####/#    /#    / ### /    #/    #/#### ", // S
    "#####/  #  /  #  /  #  /  #  /  #  /  #  ", // T
    "#   #/#   #/#   #/#   #/#   #/#   #/ ### ", // U
    "#   #/#   #/#   #/#   #/#   #/ # # /  #  ", // V
    "#   #/#   #/#   #/# # #/# # #/## ##/#   #", // W
    "#   #/#   #/ # # /  #  / # # /#   #/#   #", // X
    "#   #/#   #/ # # /  #  /  #  /  #  /  #  ", // Y
    "#####/    #/   # /  #  / #   /#    /#####", // Z
    " ### / #   / #   / #   / #   / #   / ### ", // [
    "#    /#    / #   /  #  /   # /    #/    #", // backslash
    " ### /   # /   # /   # /   # /   # / ### ", // ]
    "  #  / # # /#   #/     /     /     /     ", // ^
    "     /     /     /     /     /     /#####", // _
    " #   /  #  /     /     /     /     /     ", // `
    "     /     / ### /    #/ ####/#   #/ ####", // a
    "#    /#    /#### /#   #/#   #/#   #/#### ", // b
    "     /     / ####/#    /#    /#    / ####", // c
    "    #/    #/ ####/#   #/#   #/#   #/ ####", // d
    "     /     / ### /#   #/#####/#    / ### ", // e
    "  ## / #  #/ #   /###  / #   / #   / #   ", // f
    "     / ####/#   #/#   #/ ####/    #/ ### ", // g
    "#    /#    /#### /#   #/#   #/#   #/#   #", // h
    "  #  /     / ##  /  #  /  #  /  #  / ### ", // i
    "   # /     /  ## /   # /   # /#  # / ##  ", // j
    "#    /#    /#  # /# #  /##   /# #  /#  # ", // k
    " ##  /  #  /  #  /  #  /  #  /  #  / ### ", // l
    "     /     /## # /# # #/# # #/#   #/#   #", // m
    "     /     /#### /#   #/#   #/#   #/#   #", // n
    "     /     / ### /#   #/#   #/#   #/ ### ", // o
    "     /#### /#   #/#   #/#### /#    /#    ", // p
    "     / ####/#   #/#   #/ ####/    #/    #", // q
    "     /     /# ## /##   /#    /#    /#    ", // r
    "     /     / ####/#    / ### /    #/#### ", // s
    " #   / #   /###  / #   / #   / #  #/  ## ", // t
    "     /     /#   #/#   #/#   #/#   #/ ####", // u
    "     /     /#   #/#   #/#   #/ # # /  #  ", // v
    "     /     /#   #/#   #/# # #/# # #/ # # ", // w
    "     /     /#   #/ # # /  #  / # # /#   #", // x
    "     /#   #/#   #/#   #/ ####/    #/ ### ", // y
    "     /     /#####/   # /  #  / #   /#####", // z
    "   # /  #  /  #  / #   /  #  /  #  /   # ", // {
    "  #  /  #  /  #  /  #  /  #  /  #  /  #  ", // |
    " #   /  #  /  #  /   # /  #  /  #  / #   ", // }
    "     /     / #  #/# # #/#  # /     /     ", // ~
];

type Face = [[u8; H]; 95];

/// Rows as bitmasks, bit `W-1` leftmost. Parsed once; a malformed cell would be
/// a typo in [`ART`], so it is asserted rather than silently padded.
fn face() -> &'static Face {
    static FACE: OnceLock<Face> = OnceLock::new();
    FACE.get_or_init(|| {
        let mut f = [[0u8; H]; 95];
        for (g, art) in ART.iter().enumerate() {
            let mut rows = art.split('/');
            for row in f[g].iter_mut() {
                let src = rows.next().expect("glyph has 7 rows");
                assert_eq!(src.len(), W, "glyph {g} row is not {W} wide");
                let mut bits = 0u8;
                for (x, c) in src.bytes().enumerate() {
                    if c == b'#' {
                        bits |= 1 << (W - 1 - x);
                    }
                }
                *row = bits;
            }
            assert!(rows.next().is_none(), "glyph {g} has more than 7 rows");
        }
        f
    })
}

/// Fold a char onto the ASCII cell that carries its shape. Anything with no
/// sensible stand-in becomes `?`, which is honest: the label is still placed,
/// the reader can see a character is missing.
fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'ç' => 'c',
        'Ç' => 'C',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'ñ' => 'n',
        'Ñ' => 'N',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'O',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        // Typography the report builders actually emit.
        '·' | '•' => '.',
        '–' | '—' | '‑' | '−' => '-',
        '’' | '‘' => '\'',
        '“' | '”' => '"',
        '…' => '.',
        '€' => 'E',
        '\u{a0}' | '\u{202f}' | '\u{2009}' => ' ',
        c if (FIRST..=LAST).contains(&c) => c,
        _ => '?',
    }
}

/// Rows of one character's cell, top to bottom.
pub fn glyph(c: char) -> [u8; H] {
    let c = fold(c);
    face()[(c as usize) - (FIRST as usize)]
}

/// Rendered width of `s` at `scale`, without the trailing side bearing.
pub fn width(s: &str, scale: usize) -> usize {
    let n = s.chars().count();
    if n == 0 {
        0
    } else {
        (n * ADVANCE - 1) * scale
    }
}

/// Longest prefix of `s` that fits `max` pixels at `scale`, ellipsised when cut.
pub fn truncate(s: &str, scale: usize, max: usize) -> String {
    if width(s, scale) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if width(&format!("{out}{c}."), scale) > max {
            break;
        }
        out.push(c);
    }
    out.push('.');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_parses() {
        // The parse asserts shape; this pins that the table covers the range.
        assert_eq!(ART.len(), (LAST as usize) - (FIRST as usize) + 1);
        face();
    }

    #[test]
    fn distinct_shapes_stay_distinct() {
        // The whole point of 5x7 over anything smaller: these pairs must differ.
        for (a, b) in [('8', 'B'), ('1', 'l'), ('0', 'O'), ('5', 'S'), ('2', 'Z')] {
            assert_ne!(glyph(a), glyph(b), "{a} and {b} render identically");
        }
        // Only the space is blank.
        for c in ' '..='~' {
            let blank = glyph(c).iter().all(|r| *r == 0);
            assert_eq!(blank, c == ' ', "{c:?} blankness");
        }
    }

    #[test]
    fn folds_to_readable_ascii() {
        assert_eq!(glyph('é'), glyph('e'));
        assert_eq!(glyph('·'), glyph('.'));
        assert_eq!(glyph('日'), glyph('?'));
    }
}
