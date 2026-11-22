//! The mark atlas and the palettes, made on the CPU at start-up (DESIGN.md §1.6.1).
//!
//! The atlas is RG8: R is how much of the mark covers a texel, G is its height (for relief). One row
//! per mark set, one column per brightness level (16), each tile 32 × 32 and seamless across its
//! edges, so marks rotated or tiled through `fract` never show a seam.
use crate::style::set;
use crate::text::Text;

pub const TILE: usize = 32;
pub const LEVELS: usize = 16;
pub const WIDTH: usize = TILE * LEVELS;
pub const HEIGHT: usize = TILE * set::COUNT;
/// Mip levels down to one texel per tile, so small cells read an average of the mark, not a few
/// aliased texels of it.
pub const MIPS: usize = 6;

/// Candidate characters for the character set: printable ASCII. They are ordered by measured ink,
/// never by how dense they look.
pub const CANDIDATES: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

/// 8 × 8 Bayer threshold, 0..63, by bit-interleaving (the same formula as the shader's `bayer8`).
pub fn bayer8(x: u32, y: u32) -> u32 {
    let (x, y) = (x & 7, y & 7);
    let a = x ^ y;
    ((a & 1) << 5) | ((y & 1) << 4) | ((a & 2) << 2) | ((y & 2) << 1) | ((a & 4) >> 1) | ((y & 4) >> 2)
}

pub struct Atlas {
    /// RG8, `WIDTH` × `HEIGHT`, level 0 only.
    pub pixels: Vec<u8>,
    /// The characters chosen for each level of the character set.
    pub chars: [char; LEVELS],
    /// Their measured ink (sum of coverage over the tile, 0..1 per texel).
    pub ink: [f32; LEVELS],
}

/// An atlas being made (`Atlas::begin`).
pub struct AtlasBuild { atlas: Atlas, next: u8 }
impl AtlasBuild {
    /// Make the next set; true when every set is made.
    pub fn step(&mut self) -> bool {
        if self.next < set::COUNT as u8 {
            let s = self.next;
            for level in 0..LEVELS {
                let t = level as f32 / (LEVELS - 1) as f32;
                put(&mut self.atlas.pixels, s as usize, level, |x, y| geometric(s, t, x, y));
            }
            self.next += 1;
        }
        self.next >= set::COUNT as u8
    }
    /// How many sets are still to make.
    pub fn left(&self) -> usize { (set::COUNT as u8).saturating_sub(self.next) as usize }
    pub fn finish(mut self) -> Atlas { while !self.step() {} self.atlas }
}

impl Atlas {
    /// Mark coverage and height at texel (x, y) of tile (set, level), 0..1.
    pub fn at(&self, s: usize, level: usize, x: usize, y: usize) -> (f32, f32) {
        let o = ((s * TILE + y) * WIDTH + level * TILE + x) * 2;
        (self.pixels[o] as f32 / 255.0, self.pixels[o + 1] as f32 / 255.0)
    }

    /// `face` is the face the character set is cut from (a monospaced pixel face reads best).
    pub fn build(text: &mut Text, face: u8) -> Self {
        let mut b = Self::begin(text, face);
        while !b.step() {}
        b.finish()
    }
    /// The atlas made a row at a time (DESIGN.md §1.23, the warm-up): the character set now (it needs the
    /// text system), then one geometric set per `AtlasBuild::step`. The finished atlas is the same as `build`'s.
    pub fn begin(text: &mut Text, face: u8) -> AtlasBuild {
        let mut pixels = vec![0u8; WIDTH * HEIGHT * 2];
        let (chars, ink, tiles) = characters(text, face);
        for (level, tile) in tiles.iter().enumerate() {
            put(&mut pixels, set::CHARS as usize, level, |x, y| { let c = tile[y * TILE + x]; (c, c) });
        }
        AtlasBuild { atlas: Self { pixels, chars, ink }, next: 1 }
    }

    /// Every mip level, largest first, each RG8 and half the size of the one before.
    pub fn mips(&self) -> Vec<(usize, usize, Vec<u8>)> {
        let mut out = vec![(WIDTH, HEIGHT, self.pixels.clone())];
        for _ in 1..MIPS {
            let (w, h, prev) = out.last().expect("level 0");
            let (nw, nh) = (w / 2, h / 2);
            let mut next = vec![0u8; nw * nh * 2];
            for y in 0..nh { for x in 0..nw { for c in 0..2 {
                let at = |dx: usize, dy: usize| prev[((y * 2 + dy) * w + x * 2 + dx) * 2 + c] as u32;
                next[(y * nw + x) * 2 + c] = ((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8;
            } } }
            out.push((nw, nh, next));
        }
        out
    }
}

fn put(pixels: &mut [u8], s: usize, level: usize, f: impl Fn(usize, usize) -> (f32, f32)) {
    for y in 0..TILE { for x in 0..TILE {
        let (m, h) = f(x, y);
        let o = ((s * TILE + y) * WIDTH + level * TILE + x) * 2;
        pixels[o] = (m.clamp(0.0, 1.0) * 255.0).round() as u8;
        pixels[o + 1] = (h.clamp(0.0, 1.0) * 255.0).round() as u8;
    } }
}

/// Rasterise every candidate into a tile, measure its ink, then pick the 16 whose ink lies nearest to
/// evenly spaced targets from none to the most, keeping the order strict.
fn characters(text: &mut Text, face: u8) -> ([char; LEVELS], [f32; LEVELS], Vec<Vec<f32>>) {
    let mut cands: Vec<(char, f32, Vec<f32>)> = CANDIDATES.chars().map(|ch| {
        let tile = char_tile(text, face, ch);
        let ink = tile.iter().sum::<f32>();
        (ch, ink, tile)
    }).collect();
    cands.sort_by(|a, b| a.1.total_cmp(&b.1));
    let max = cands.last().map(|c| c.1).unwrap_or(1.0);
    let mut chosen: Vec<usize> = vec![0];
    for j in 1..LEVELS {
        let target = max * j as f32 / (LEVELS - 1) as f32;
        let last = *chosen.last().expect("first");
        // Strictly more ink than the previous pick, and leave enough candidates for the levels after.
        let room = cands.len() - (LEVELS - 1 - j);
        let pick = (last + 1..room)
            .filter(|&i| cands[i].1 > cands[last].1)
            .min_by(|&a, &b| (cands[a].1 - target).abs().total_cmp(&(cands[b].1 - target).abs()))
            .unwrap_or((last + 1).min(cands.len() - 1));
        chosen.push(pick);
    }
    let mut chars = [' '; LEVELS];
    let mut ink = [0.0; LEVELS];
    let mut tiles = Vec::with_capacity(LEVELS);
    for (level, &i) in chosen.iter().enumerate() {
        chars[level] = cands[i].0;
        ink[level] = cands[i].1;
        tiles.push(cands[i].2.clone());
    }
    (chars, ink, tiles)
}

/// One character in a 32 × 32 tile. The face is drawn at twice its native size (each font pixel is
/// 2 × 2) and then widened twice by repeating columns, so its cell (7 × 14 font pixels for Departure
/// Mono) fills the square tile the way ASCII art fills its cells. Coverage at the tile edges is left
/// empty, so tiles join without seams.
fn char_tile(text: &mut Text, face: u8, ch: char) -> Vec<f32> {
    let mut tile = vec![0.0f32; TILE * TILE];
    let px = text.native(face).unwrap_or(16.0) * 2.0;
    let Some(g) = text.raster(face, ch, px) else { return tile };
    // The cell: advance wide, ascent + descent tall, centred in the tile after widening.
    let (cw, chh) = (g.advance * 2.0, g.ascent + g.descent);
    let (ox, oy) = ((TILE as f32 - cw) * 0.5, (TILE as f32 - chh) * 0.5);
    for y in 0..g.h as i32 { for x in 0..g.w as i32 {
        let c = g.data[(y as u32 * g.w + x as u32) as usize] as f32 / 255.0;
        if c <= 0.0 { continue; }
        let ty = (oy + g.ascent - g.top as f32 + y as f32).round() as i32;
        for dx in 0..2 {
            let tx = (ox + (g.left + x) as f32 * 2.0).round() as i32 + dx;
            if tx >= 1 && ty >= 1 && tx < TILE as i32 - 1 && ty < TILE as i32 - 1 { tile[ty as usize * TILE + tx as usize] = c; }
        }
    } }
    tile
}

/// Coverage and height of a geometric mark at texel (x, y), level fraction `t` (0 = none … 1 = most).
/// Each texel is averaged over 4 × 4 sub-samples so edges are smooth; positions are in tile units,
/// centre (0.5, 0.5).
fn geometric(s: u8, t: f32, x: usize, y: usize) -> (f32, f32) {
    let n = 4;
    let (mut m, mut h) = (0.0, 0.0);
    for sy in 0..n { for sx in 0..n {
        let u = (x as f32 + (sx as f32 + 0.5) / n as f32) / TILE as f32;
        let v = (y as f32 + (sy as f32 + 0.5) / n as f32) / TILE as f32;
        let (a, b) = sample(s, t, u, v, x, y);
        m += a; h += b;
    } }
    let k = (n * n) as f32;
    (m / k, h / k)
}

fn sample(s: u8, t: f32, u: f32, v: f32, x: usize, y: usize) -> (f32, f32) {
    let (dx, dy) = (u - 0.5, v - 0.5);
    let r = (dx * dx + dy * dy).sqrt();
    let on = |b: bool| if b { 1.0 } else { 0.0 };
    // Distance from the nearest tile edge, 0 at the edge, 0.5 at the centre.
    let edge = dx.abs().max(dy.abs());
    let inset = 0.5 - edge;
    match s {
        // The 8 × 8 Bayer matrix enlarged to the tile (4 texels per entry); level k lights the entries
        // whose threshold is below k / 15, so level 0 is empty and the last is solid.
        set::DITHER => { let b = (bayer8(x as u32 / 4, y as u32 / 4) as f32 + 0.5) / 64.0; let m = on(b < t); (m, m) }
        // Solid, with a very faint line along one edge: neighbouring cells then read as a grid of
        // pixels. The line is on one side only, so tiling gives one line per cell.
        set::PIXEL => (if x == 0 || y == 0 { 0.93 } else { 1.0 }, 1.0),
        // A tile: solid face, a thin dark grout and bevelled edges in the height channel.
        set::MOSAIC => (if inset < 1.0 / 32.0 { 0.55 } else { 1.0 }, (inset / 0.16).min(1.0).sqrt()),
        // A brick: solid with a round stud; the stud is in the height channel, ringed by a faint line.
        set::BRICK => {
            let stud = 0.27;
            let dome = if r < stud { 0.35 + 0.65 * (1.0 - (r / stud).powi(2)).sqrt() } else { 0.35 * (inset / 0.08).min(1.0) };
            (if (r - stud).abs() < 0.02 { 0.82 } else { 1.0 }, dome)
        }
        // Area grows with the level.
        set::DOTS => { let rr = (t / std::f32::consts::PI).sqrt(); let m = on(r < rr); (m, m * (1.0 - r / rr.max(1e-3)).max(0.0).sqrt()) }
        set::CROSS => {
            let w = 0.04 + 0.22 * t; let l = 0.18 + 0.3 * t;
            let m = on(t > 0.0 && ((dx.abs() < w && dy.abs() < l) || (dy.abs() < w && dx.abs() < l)));
            (m, m)
        }
        // One 45° line per tile, measured on the torus so the tile wraps: thickness grows with level.
        set::DIAGONAL => {
            let d = ((u - v).rem_euclid(1.0) - 0.5).abs(); // 0.5 on the line through the corners
            let dist = (0.5 - d) * std::f32::consts::FRAC_1_SQRT_2;
            let m = on(dist < t * 0.36);
            (m, m)
        }
        set::DIAMOND => { let rr = (t * 0.5).sqrt(); let m = on(dx.abs() + dy.abs() < rr); (m, m) }
        set::LINES => { let m = on(dy.abs() < t * 0.5); (m, m) }
        // A soft dot with a glow: brighter and wider with the level, nothing at level 0.
        set::GLOW => { let rr = 0.08 + 0.3 * t; let m = t.sqrt() * (-(r / rr).powi(2) * 1.6).exp(); (m, m) }
        // A horizontal stroke nearly the length of the tile, with round ends; the shader turns it, and
        // turned strokes wrap at the tile edge into their neighbours like hatching. Thicker with the level.
        set::STROKE => {
            let half = 0.44; let w = 0.03 + 0.2 * t;
            let px = (dx.abs() - half).max(0.0);
            let m = on(t > 0.0 && (px * px + dy * dy).sqrt() < w);
            (m, m)
        }
        // A lamp: a rounded square, lit by the level; an unlit lamp stays as a faint seat, so the
        // matrix reads even in the dark.
        set::LED => {
            let k = 0.36;
            let d = ((dx.abs() - k * 0.6).max(0.0).powi(2) + (dy.abs() - k * 0.6).max(0.0).powi(2)).sqrt();
            let lamp = on(d < k * 0.4);
            let m = lamp * (0.08 + 0.92 * t);
            (m, lamp * (1.0 - (r / 0.5).min(1.0)))
        }
        // Braille: up to eight dots in two columns of four, lit in a fixed order as the level rises.
        set::BRAILLE => {
            const ORDER: [(usize, usize); 8] = [(0, 0), (1, 1), (0, 2), (1, 3), (1, 0), (0, 1), (1, 2), (0, 3)];
            let n = (t * 8.0).round() as usize;
            let mut m = 0.0f32;
            for &(c, rr) in ORDER.iter().take(n) {
                let (cx, cy) = (0.32 + 0.36 * c as f32, 0.14 + 0.24 * rr as f32);
                if ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() < 0.085 { m = 1.0; }
            }
            (m, m)
        }
        // An isometric cube centred in the cell, larger with the level: the top face full, the left at
        // two thirds, the right at a third (the height channel keeps the faces apart for relief).
        set::CUBES => {
            let s = 0.08 + 0.36 * t;
            if t <= 0.0 { return (0.0, 0.0); }
            let (px, py) = (dx / s, dy / s);
            // Hexagon outline of the cube, then its three faces.
            let inside = py.abs() <= 1.0 && (py.abs() * 0.5 + px.abs() * 0.866) <= 1.0;
            if !inside { return (0.0, 0.0); }
            let top = py < 0.0 && (py + px.abs() * 0.577) < 0.0;
            let face = if top { 1.0 } else if px < 0.0 { 0.68 } else { 0.38 };
            (face, face)
        }
        // A hexagon inset in its cell, growing with the level; a fine seam all round.
        set::HEX => {
            let rr = 0.08 + 0.4 * t;
            let hx = dx.abs() * 0.866 + dy.abs() * 0.5;
            let m = on(t > 0.0 && hx.max(dy.abs()) < rr);
            (m, m * (1.0 - hx.max(dy.abs()) / rr.max(1e-3)).sqrt())
        }
        // Stippling: sixteen fixed scattered places, a dot at the first n of them.
        set::STIPPLE => {
            let n = (t * 16.0).round() as u32;
            let mut m = 0.0f32;
            for i in 0..n {
                let hh = |k: u32| ((i.wrapping_mul(2_654_435_761).wrapping_add(k.wrapping_mul(40_503)) >> 8) & 0xffff) as f32 / 65535.0;
                let (cx, cy) = (0.12 + 0.76 * hh(1), 0.12 + 0.76 * hh(7));
                if ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() < 0.07 { m = 1.0; }
            }
            (m, m)
        }
        // Cross-hatching: one to four directions of line as the level rises, like pen hatching.
        set::HATCH => {
            let k = (t * 4.0).round() as u32;
            let w = 0.07;
            let d1 = ((u + v).rem_euclid(0.5) - 0.25).abs();
            let lines = [dy.abs() < w, d1 < w * 0.8, dx.abs() < w, ((u - v).rem_euclid(0.5) - 0.25).abs() < w * 0.8];
            let m = on(lines.iter().take(k as usize).any(|b| *b));
            (m, m)
        }
        // Truchet: two quarter arcs round opposite corners; neighbouring cells join into curves.
        set::TRUCHET => {
            let w = 0.03 + 0.14 * t;
            let a = ((u * u + v * v).sqrt() - 0.5).abs();
            let b = (((1.0 - u).powi(2) + (1.0 - v).powi(2)).sqrt() - 0.5).abs();
            let m = on(t > 0.0 && a.min(b) < w);
            (m, m)
        }
        // Contour rings about the centre, thicker with the level.
        set::TOPO => {
            let w = 0.02 + 0.09 * t;
            let ring = ((r * 3.0).fract() - 0.5).abs() / 3.0;
            let m = on(t > 0.0 && ring < w);
            (m, m)
        }
        _ => (0.0, 0.0),
    }
}

// ── Characters at the size they are shown ────────────────────────────────────────────────────────
//
// Minifying 32 px character tiles blurs them below a pitch of about 16, and the picture then reads as
// grey noise. So the character set is also cut once per device pitch from `CHAR_MIN` to `CHAR_MAX`:
// each tile is exactly P × P texels, drawn from the pixel face at a whole multiple of its size, and the
// shader reads it texel for texel. Every pitch chooses its own 16 characters by measured ink, among
// the ones that fit its tile.

pub const CHAR_MIN: usize = 4;
pub const CHAR_MAX: usize = 40;
/// Row offset of the tiles of pitch `p` (rows of all smaller pitches above it).
pub const fn char_row(p: usize) -> usize { (p * (p - 1) - CHAR_MIN * (CHAR_MIN - 1)) / 2 }
pub const CHAR_W: usize = CHAR_MAX * LEVELS;
/// The row under the tiles: for each pitch `p`, at columns `4 (p − CHAR_MIN)` on, the ink box of its
/// lightest character that is not blank (level 1), in texels: left, top, right, bottom (the last two past
/// it). The shader centres that character's ink on a thin line (DESIGN.md §1.6).
pub const INK_BOX_ROW: usize = char_row(CHAR_MAX + 1);
pub const CHAR_H: usize = INK_BOX_ROW + 1;

pub struct CharTiles {
    /// R8, `CHAR_W` × `CHAR_H`.
    pub pixels: Vec<u8>,
    /// For each pitch from `CHAR_MIN`: the 16 characters chosen and their ink (texels covered). A
    /// character drawn bold (doubled one texel to the right) is shown as `(c, true)`.
    pub chosen: Vec<[(char, bool, f32); LEVELS]>,
}
impl CharTiles {
    pub fn at(&self, p: usize, level: usize, x: usize, y: usize) -> u8 { self.pixels[(char_row(p) + y) * CHAR_W + level * p + x] }
    pub fn chosen(&self, p: usize) -> &[(char, bool, f32); LEVELS] { &self.chosen[p - CHAR_MIN] }
}

/// The character tiles for every pitch. `face` should be a pixel face (Departure Mono): its glyphs are
/// already whole texels, so nothing is resampled at 8 px and up.
pub fn char_tiles(text: &mut Text, face: u8) -> CharTiles {
    let mut pixels = vec![0u8; CHAR_W * CHAR_H];
    let mut chosen = Vec::new();
    // Below 8 px no glyph fits whole; those tiles are the 8 px ones averaged down.
    let eight = char_set(text, face, 8);
    for p in CHAR_MIN..=CHAR_MAX {
        let set = if p < 8 { shrink(&eight, 8, p) } else { char_set(text, face, p) };
        for (level, (_, _, _, tile)) in set.iter().enumerate() {
            for y in 0..p { for x in 0..p { pixels[(char_row(p) + y) * CHAR_W + level * p + x] = tile[y * p + x]; } }
            if level == 1 {
                let inked = |x: usize, y: usize| tile[y * p + x] > 0;
                let b = (0..p).flat_map(|y| (0..p).map(move |x| (x, y))).filter(|&(x, y)| inked(x, y))
                    .fold([p, p, 0, 0], |b, (x, y)| [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)]);
                let b = if b[2] > b[0] { b } else { [0, 0, p, p] };
                for (k, v) in b.into_iter().enumerate() { pixels[INK_BOX_ROW * CHAR_W + 4 * (p - CHAR_MIN) + k] = v as u8; }
            }
        }
        let mut c = [(' ', false, 0.0); LEVELS];
        for (i, t) in set.iter().enumerate() { c[i] = (t.0, t.1, t.2); }
        chosen.push(c);
    }
    CharTiles { pixels, chosen }
}

type CharTile = (char, bool, f32, Vec<u8>);

/// Every candidate that fits a P × P tile (plus bold forms denser than any plain one), ordered by
/// ink, and the 16 picked from it.
fn char_set(text: &mut Text, face: u8, p: usize) -> Vec<CharTile> {
    let native = text.native(face).unwrap_or(11.0);
    let Some(h) = text.raster(face, 'H', native) else { return vec![(' ', false, 0.0, vec![0; p * p]); LEVELS] };
    let (cap, adv) = (h.top as usize, h.advance.round() as usize);
    // The largest whole multiple of the face whose capitals fit, keeping a texel of air per 9.
    let m = (p / (cap + 1)).max(1);
    let room = p as i32 - (cap * m) as i32;
    let base = room / 2 + (cap * m) as i32;
    let ox = (p as i32 - (adv * m) as i32) / 2;
    let mut plain: Vec<CharTile> = Vec::new();
    for ch in CANDIDATES.chars() {
        let Some(g) = text.raster(face, ch, native * m as f32) else { if ch == ' ' { plain.push((' ', false, 0.0, vec![0; p * p])); } continue };
        let mut tile = vec![0u8; p * p];
        let mut fits = true;
        for y in 0..g.h as i32 { for x in 0..g.w as i32 {
            if g.data[(y as u32 * g.w + x as u32) as usize] < 128 { continue; }
            let (tx, ty) = (ox + g.left + x, base - g.top + y);
            if tx < 0 || ty < 0 || tx >= p as i32 || ty >= p as i32 { fits = false; continue; }
            tile[ty as usize * p + tx as usize] = 255;
        } }
        if fits { let ink = tile.iter().filter(|&&v| v > 0).count() as f32; plain.push((ch, false, ink, tile)); }
    }
    let most = plain.iter().map(|t| t.2).fold(0.0, f32::max);
    // Bold: each glyph doubled one face pixel to the right. Only the forms denser than every plain
    // character are kept, so they extend the range at the top instead of replacing plain ones.
    let mut cands = plain.clone();
    for (ch, _, _, tile) in &plain {
        let mut b = tile.clone();
        for y in 0..p { for x in m..p { if tile[y * p + x - m] > 0 { b[y * p + x] = 255; } } }
        let ink = b.iter().filter(|&&v| v > 0).count() as f32;
        if ink > most { cands.push((*ch, true, ink, b)); }
    }
    cands.sort_by(|a, b| a.2.total_cmp(&b.2));
    let max = cands.last().map(|c| c.2).unwrap_or(1.0);
    let picks = pick_levels(&cands.iter().map(|c| c.2).collect::<Vec<_>>(), max);
    picks.into_iter().map(|i| cands[i].clone()).collect()
}

/// Indices of 16 entries of `inks` (sorted ascending, first is none) nearest to evenly spaced targets
/// from 0 to `max`, strictly increasing.
fn pick_levels(inks: &[f32], max: f32) -> Vec<usize> {
    let mut chosen: Vec<usize> = vec![0];
    for j in 1..LEVELS {
        let target = max * j as f32 / (LEVELS - 1) as f32;
        let last = *chosen.last().expect("first");
        let room = inks.len().saturating_sub(LEVELS - 1 - j).max(last + 1);
        let pick = (last + 1..room)
            .filter(|&i| inks[i] > inks[last])
            .min_by(|&a, &b| (inks[a] - target).abs().total_cmp(&(inks[b] - target).abs()))
            .unwrap_or((last + 1).min(inks.len() - 1));
        chosen.push(pick);
    }
    chosen
}

/// Average tiles of size `from` down to `to` (box filter).
fn shrink(set: &[CharTile], from: usize, to: usize) -> Vec<CharTile> {
    set.iter().map(|(c, b, ink, t)| {
        let mut out = vec![0u8; to * to];
        for y in 0..to { for x in 0..to {
            let (x0, x1, y0, y1) = (x * from / to, ((x + 1) * from).div_ceil(to), y * from / to, ((y + 1) * from).div_ceil(to));
            let mut acc = 0u32; let mut n = 0u32;
            for yy in y0..y1 { for xx in x0..x1 { acc += t[yy * from + xx] as u32; n += 1; } }
            out[y * to + x] = (acc / n.max(1)) as u8;
        } }
        (*c, *b, *ink, out)
    }).collect()
}

/// Palettes: one row of 16 colours each, with how many of them are meant to be used.
pub const PALETTES: [(&str, usize, [[u8; 3]; 16]); 7] = [
    ("黑白", 2, pad(&[[18, 18, 22], [238, 233, 216]])),
    ("孔版三色", 3, pad(&[[30, 34, 78], [255, 76, 126], [247, 238, 214]])),
    ("掌机绿", 4, pad(&[[15, 56, 15], [48, 98, 48], [139, 172, 15], [196, 216, 120]])),
    ("CGA", 4, pad(&[[0, 0, 0], [85, 255, 255], [255, 85, 255], [255, 255, 255]])),
    ("暖色五级", 5, pad(&[[28, 16, 18], [110, 40, 36], [204, 94, 52], [242, 170, 84], [252, 236, 196]])),
    ("PICO-8", 16, [
        [0, 0, 0], [29, 43, 83], [126, 37, 83], [0, 135, 81], [171, 82, 54], [95, 87, 79], [194, 195, 199], [255, 241, 232],
        [255, 0, 77], [255, 163, 0], [255, 236, 39], [0, 228, 54], [41, 173, 255], [131, 118, 156], [255, 119, 168], [255, 204, 170],
    ]),
    ("包豪斯", 5, pad(&[[16, 16, 16], [217, 34, 26], [26, 69, 163], [247, 194, 20], [236, 231, 218]])),
];

const fn pad(c: &[[u8; 3]]) -> [[u8; 3]; 16] {
    let mut out = [[0u8; 3]; 16];
    let mut i = 0;
    while i < 16 { out[i] = if i < c.len() { c[i] } else { c[c.len() - 1] }; i += 1; }
    out
}

/// RGBA8, 16 × `PALETTES.len()`.
pub fn palette_pixels() -> Vec<u8> {
    PALETTES.iter().flat_map(|(_, _, row)| row.iter().flat_map(|c| [c[0], c[1], c[2], 255])).collect()
}
