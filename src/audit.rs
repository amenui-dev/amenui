//! The audit (DESIGN.md §7): what is wrong with a page as it is drawn, measured, not guessed. Input is
//! one deterministic frame (its pixels, and the same frame with the glyphs left out, for the ground under
//! the text) and the runtime that drew it; output is a list of findings, most serious first, each with
//! what was measured, the limit, and one thing to do about it. Five kinds are measured: text cut off by a
//! clip or the window (or a word drawn in part), contrast against what is really under the text, glyphs
//! drawn over each other, text drawn too small, and a page that keeps asking for frames with nothing
//! going on; the view's structural problems (`Ui::problems`) join the list. `render --audit` prints it,
//! `--strict` fails on P0 and P1.
#![warn(missing_docs)]
use crate::form::ink_box;
use crate::ui::{Rect, Ui};
use serde::Serialize;

/// How serious a finding is: P0 breaks reading, P1 hurts it, P2 is worth a look, P3 a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Level {
    /// Breaks reading.
    P0,
    /// Hurts reading.
    P1,
    /// Worth a look.
    P2,
    /// A note.
    P3,
}

/// What a finding is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Text to read cut by a clip or the window, or a word drawn in part.
    Clipped,
    /// Text against the ground actually drawn under it (WCAG 2 contrast ratio).
    Contrast,
    /// Glyphs drawn over each other.
    Overlap,
    /// Text to read drawn under 9 device px.
    Small,
    /// Frames asked for with no input and nothing declared as moving (`El::ongoing`).
    Restless,
    /// Text to read whose strokes the material wears away at rest: less of its ink drawn than plain draws.
    Eroded,
    /// A problem in the view's structure (`Ui::problems`).
    Structure,
    /// Text to read nearer a window edge than its preset's rule allows (`preset::Rule::Margin`).
    Margin,
    /// A line of running text longer than its preset's rule allows (`preset::Rule::Measure`).
    Measure,
    /// More of the frame at rest in the accent than its preset's rule allows (`preset::Rule::Accent`).
    Accent,
}
impl Kind {
    /// Its name in the audit's output.
    pub fn name(self) -> &'static str {
        match self { Kind::Clipped => "clipped", Kind::Contrast => "contrast", Kind::Overlap => "overlap", Kind::Small => "small", Kind::Restless => "restless", Kind::Eroded => "eroded", Kind::Structure => "structure", Kind::Margin => "margin", Kind::Measure => "measure", Kind::Accent => "accent" }
    }
}

/// One finding: how serious, which element (its key), what kind, what was measured and the limit it
/// broke (in the kind's unit: px, a ratio, a fraction, seconds), and a sentence on what to do.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    /// How serious it is.
    pub level: Level,
    /// The element's key.
    pub key: String,
    /// What it is about.
    pub kind: Kind,
    /// What was measured.
    pub measured: f32,
    /// The limit it broke.
    pub limit: f32,
    /// What to do.
    pub fix: String,
}
impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {} `{}`: {}", self.level, self.kind.name(), self.key, self.fix)
    }
}

/// The smallest text to read, device px.
pub const MIN_TEXT_PX: f32 = 9.0;
/// The least contrast of text to read (WCAG 2 AA): under it is a P1, under 3 a P0.
pub const MIN_CONTRAST: f32 = 4.5;
/// The most of one glyph's ink another may cover in text to read.
pub const MAX_OVERLAP: f32 = 0.10;
/// How long a page may go on asking for frames after the last input (s).
pub const SETTLE_S: f32 = 2.0;
/// The least share of plain's ink pixels a material draws of text to read at rest.
pub const MIN_INK: f32 = 0.85;

/// Whether the findings fail a strict run (any P0 or P1).
pub fn fails(f: &[Finding]) -> bool { f.iter().any(|f| f.level <= Level::P1) }

/// Audit the frame the runtime last drew: `px` its RGBA8 pixels, `ground` the same frame drawn with
/// every glyph transparent (`Renderer::hide_glyphs`), both at the runtime's device size. The page's
/// restlessness is a separate measurement (`restless`), since it needs the clock to run.
pub fn audit<S, M: Clone>(ui: &mut Ui<S, M>, px: &[u8], ground: &[u8]) -> Vec<Finding> {
    let mut out: Vec<Finding> = ui.problems().into_iter().map(|p| Finding { level: Level::P1, key: String::new(), kind: Kind::Structure, measured: 1.0, limit: 0.0, fix: p.to_string() }).collect();
    clipped(ui, &mut out);
    contrast(ui, px, ground, &mut out);
    overlap(ui, &mut out);
    small(ui, &mut out);
    sort(&mut out);
    out
}

/// Most serious first, then by key.
pub fn sort(f: &mut [Finding]) { f.sort_by(|a, b| a.level.cmp(&b.level).then_with(|| a.key.cmp(&b.key)).then_with(|| a.kind.name().cmp(b.kind.name()))); }

/// After the last input, run the clock on with nothing happening (60 Hz, from `from` seconds): a page
/// still asking for frame after frame `SETTLE_S` later, with nothing on it declared `El::ongoing`, is
/// restless (a timed wake now and then, such as a caret's blink, is not).
#[cfg(not(target_arch = "wasm32"))]
pub fn restless(d: &mut dyn crate::drive::Driven, from: f32) -> Option<Finding> {
    // As a host would: each frame, ticked, and drawn when it asks (here only marked drawn).
    let end = from + SETTLE_S + 0.5;
    // A timed wake now and then (a caret's blink) is not restless; frames asked for one after another are.
    let (mut t, mut busy, mut run) = (from, from, 0);
    while t < end {
        t = (t + 1.0 / 60.0).min(end);
        d.tick(t);
        if d.needs_frame(t) { run += 1; d.frame_drawn(); } else { run = 0; }
        if run >= 3 { busy = t; }
    }
    if busy - from <= SETTLE_S || d.ongoing() { return None; }
    Some(Finding { level: Level::P1, key: String::new(), kind: Kind::Restless, measured: busy - from, limit: SETTLE_S,
        fix: format!("the page still asks for frames {:.1} s after the last input with nothing moving on purpose: let what animates settle (a field effect, a loop), or mark a live part with `.ongoing()`", busy - from) })
}

/// The box that the window and every clipping ancestor that is not a scroll view leave visible
/// (logical px), and for each side (left, top, right, bottom) the key of the element whose edge it is
/// (`None`: the window's). Scroll views are left out: what they hide can be scrolled to.
fn hard_clip<S, M: Clone>(ui: &Ui<S, M>, i: usize) -> (Rect, [Option<String>; 4]) {
    let (w, h) = ui.logical_size();
    let nodes = ui.nodes();
    let (mut e, mut by): ([f32; 4], [Option<String>; 4]) = ([0.0, 0.0, w, h], Default::default());
    let mut a = nodes[i].parent;
    while let Some(p) = a {
        let n = &nodes[p];
        if n.clips && !n.scroll {
            let r = [n.rect.x, n.rect.y, n.rect.right(), n.rect.bottom()];
            for k in 0..4 {
                let tighter = if k < 2 { r[k] > e[k] } else { r[k] < e[k] };
                if tighter { e[k] = r[k]; by[k] = Some(n.key.clone()); }
            }
        }
        a = n.parent;
    }
    (Rect::new(e[0], e[1], (e[2] - e[0]).max(0.0), (e[3] - e[1]).max(0.0)), by)
}

/// Text that is read (legible text, drawn, not disabled), as node indices.
fn reading<S, M: Clone>(ui: &Ui<S, M>) -> Vec<usize> {
    let under = ui.modal_at();
    ui.nodes().iter().enumerate().filter(|(i, n)| n.legible && n.text().is_some_and(|t| !t.trim().is_empty()) && !n.hidden && !n.disabled && !n.skeleton && under.is_none_or(|m| *i >= m)).map(|(i, _)| i).collect()
}

fn clipped<S, M: Clone>(ui: &mut Ui<S, M>, out: &mut Vec<Finding>) {
    let s = ui.scale();
    let glyphs = ui.glyphs().clone();
    const SIDES: [&str; 4] = ["left", "top", "right", "bottom"];
    for i in reading(ui) {
        let (hard, by) = hard_clip(ui, i);
        let h = [hard.x * s, hard.y * s, hard.right() * s, hard.bottom() * s];
        let key = ui.nodes()[i].key.clone();
        // Glyphs wholly outside, and on each side the farthest any ink goes past it (logical px).
        let (mut whole, mut past) = (0usize, [0.0f32; 4]);
        for g in glyphs.log.iter().filter(|g| g.node == i) {
            let Some(inst) = ui.scene().instances.get(g.inst) else { continue };
            if inst.paint[2] < 0.05 { continue; }
            let b = ink_box(inst);
            if b[2] <= b[0] || b[3] <= b[1] { continue; }
            let cut = [(h[0] - b[0]).max(0.0), (h[1] - b[1]).max(0.0), (b[2] - h[2]).max(0.0), (b[3] - h[3]).max(0.0)];
            let inside = (b[2].min(h[2]) - b[0].max(h[0])).max(0.0) * (b[3].min(h[3]) - b[1].max(h[1])).max(0.0);
            if inside <= 0.0 { whole += 1; }
            for k in 0..4 { if cut[k] > 0.5 { past[k] = past[k].max(cut[k] / s); } }
        }
        let (side, most) = past.iter().enumerate().fold((0, 0.0f32), |m, (k, c)| if *c > m.1 { (k, *c) } else { m });
        // Who cuts it there: an element's key, or the window.
        let cutter = by[side].as_deref().map_or_else(|| "the window".to_owned(), |k| format!("`{k}`"));
        // A word of which some glyphs are drawn and some are not (a pose, a clip by glyph).
        let mut halves = 0;
        if let Some((_, laid, drawn)) = glyphs.laid.iter().find(|(n, ..)| *n == i)
            && drawn.iter().any(|d| *d) && drawn.iter().any(|d| !*d) {
            let n = &ui.nodes()[i];
            let origin = (n.inner.x, n.inner.y);
            let groups = ui.reveal_groups(i, crate::el::Reveal::Word);
            for bs in &groups {
                let inw: Vec<bool> = laid.iter().zip(drawn).filter(|((x, y), _)| bs.iter().any(|r| r.inset(-0.5, -0.5, -0.5, -0.5).contains(x + origin.0, y + origin.1))).map(|(_, d)| *d).collect();
                if inw.iter().any(|d| *d) && inw.iter().any(|d| !*d) { halves += 1; }
            }
        }
        let reveal = ui.nodes()[i].reveal_by;
        let side = SIDES[side];
        if whole > 0 || halves > 0 {
            let what = if halves > 0 { format!("{halves} word(s) drawn in part") } else { format!("{whole} glyph(s) outside, past the {side} edge of {cutter} by up to {most:.0} px") };
            let room = if halves > 0 { "give it room".to_owned() } else { format!("give it {:.0} px more room on the {side}", most.ceil() + 2.0) };
            out.push(Finding { level: Level::P0, key: key.clone(), kind: Kind::Clipped, measured: (whole + halves) as f32, limit: 0.0,
                fix: format!("{what}: show it by whole words (`.reveal_by(Reveal::Word)`){}, or {room}", if reveal == crate::el::Reveal::Glyph { "" } else { " already set; check what hides it" }) });
        } else if most > 0.0 {
            out.push(Finding { level: Level::P1, key, kind: Kind::Clipped, measured: most, limit: 0.0,
                fix: format!("cut {most:.0} px at the {side} by {cutter}: show it by whole words (`.reveal_by(Reveal::Word)`), or give it {:.0} px more room on the {side}", most.ceil() + 2.0) });
        }
    }
}

/// How many pixels of a frame (`px`, RGBA, `w` px wide) are in no colour the theme can make: every
/// pixel outside `skip` (pictures, drawings with their own colours; device px boxes) should be the
/// ground (anywhere between `ground` and `ground2`) mixed toward the ink, the raised grey, the extreme or
/// one of the accents, or a colour on the line of the theme's greys (`Theme::grey`), as the materials
/// mix them, within `tol` (0..255 per channel). A field or a debug layer drawn in its own
/// colour onto the frame is counted.
pub fn off_palette(px: &[u8], w: u32, theme: &crate::style::Theme, skip: &[[f32; 4]], tol: f32) -> usize {
    let c = |v: [f32; 3]| v.map(|x| x * 255.0);
    let grounds: Vec<[f32; 3]> = (0..=8).map(|i| { let t = i as f32 / 8.0; std::array::from_fn(|k| c(theme.ground)[k] + (c(theme.ground2)[k] - c(theme.ground)[k]) * t) }).collect();
    let tops = [c(theme.ink), c(theme.accent), c(theme.second), c(theme.third), c(theme.extreme()), c(theme.greys.raised)];
    // The greys' own line, ground to ink and on to the extreme, as the tones run along it.
    let g = &theme.greys;
    let chain = [c(g.raised), c(g.hairline), c(g.dim), c(g.text), c(theme.ink), c(theme.extreme())];
    let seg = |p: [f32; 3], a: [f32; 3], b: [f32; 3]| {
        let ab: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
        let l2: f32 = ab.iter().map(|v| v * v).sum();
        let t = if l2 > 0.0 { ((0..3).map(|k| (p[k] - a[k]) * ab[k]).sum::<f32>() / l2).clamp(0.0, 1.0) } else { 0.0 };
        (0..3).map(|k| (p[k] - a[k] - ab[k] * t).abs()).fold(0.0, f32::max)
    };
    px.chunks_exact(4).enumerate().filter(|(i, q)| {
        let (x, y) = ((*i as u32 % w) as f32 + 0.5, (*i as u32 / w) as f32 + 0.5);
        if skip.iter().any(|b| x >= b[0] && y >= b[1] && x < b[0] + b[2] && y < b[1] + b[3]) { return false; }
        let p = [q[0] as f32, q[1] as f32, q[2] as f32];
        !grounds.iter().any(|g| tops.iter().any(|t| seg(p, *g, *t) <= tol)) && !chain.windows(2).any(|w| seg(p, w[0], w[1]) <= tol)
    }).count()
}

/// Relative luminance of an sRGB colour (WCAG 2).
pub fn luminance(c: [u8; 3]) -> f32 {
    let f = |v: u8| { let v = v as f32 / 255.0; if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } };
    0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}
/// The WCAG 2 contrast ratio of two colours, 1..21.
pub fn ratio(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Each text's weakest line: its node, the line (from 0) and the ratio of that line's strongest ink
/// against what is drawn under it (pixels that differ from the ground by 24 or more over the three
/// channels; the strongest of them is the colour the text is drawn in, where a stem covers a whole pixel).
fn line_contrast<S, M: Clone>(ui: &Ui<S, M>, px: &[u8], ground: &[u8]) -> Vec<(usize, usize, f32)> {
    let (dw, dh) = ui.device_size();
    if px.len() < (dw * dh * 4) as usize || ground.len() < px.len() { return vec![]; }
    let at = |buf: &[u8], x: u32, y: u32| { let o = ((y * dw + x) * 4) as usize; [buf[o], buf[o + 1], buf[o + 2]] };
    let under = ui.modal_at();
    let texts: Vec<usize> = ui.nodes().iter().enumerate().filter(|(i, n)| n.text().is_some_and(|t| !t.trim().is_empty()) && !n.hidden && !n.disabled && !n.skeleton && under.is_none_or(|m| *i >= m)).map(|(i, _)| i).collect();
    let glyphs = ui.glyphs();
    let mut out = vec![];
    for i in texts {
        // Posed text dimmed by depth on purpose (`FormFx::fog`): read the nearer half of it.
        let mut zs: Vec<f32> = glyphs.log.iter().filter(|g| g.node == i).map(|g| g.z).collect();
        zs.sort_by(f32::total_cmp);
        let z_mid = zs.get(zs.len() / 2).copied().unwrap_or(0.0);
        // Each line: the glyphs drawn on it (by where layout put them, within half the type size: a
        // comma's box sits lower than a letter's), their ink boxes.
        let tol = ui.nodes()[i].para().map_or(2.0, |p| p.size * 0.5);
        let mut lines: Vec<(f32, Vec<[f32; 4]>)> = vec![];
        for g in glyphs.log.iter().filter(|g| g.node == i) {
            let Some(inst) = ui.scene().instances.get(g.inst) else { continue };
            if inst.paint[2] < 0.3 || g.z < z_mid { continue; }
            let b = ink_box(inst);
            let c = inst.clip;
            let b = [b[0].max(c[0]).max(0.0), b[1].max(c[1]).max(0.0), b[2].min(c[2]).min(dw as f32), b[3].min(c[3]).min(dh as f32)];
            if b[2] - b[0] < 1.0 || b[3] - b[1] < 1.0 { continue; }
            match lines.iter_mut().find(|(y, _)| (y - g.at.1).abs() < tol) { Some(l) => l.1.push(b), None => lines.push((g.at.1, vec![b])) }
        }
        let mut worst: Option<(f32, usize)> = None;
        for (k, (_, boxes)) in lines.iter().enumerate() {
            let mut rs: Vec<f32> = vec![];
            for b in boxes {
                for y in b[1] as u32..(b[3] as u32).min(dh) {
                    for x in b[0] as u32..(b[2] as u32).min(dw) {
                        let (p, g) = (at(px, x, y), at(ground, x, y));
                        if (p[0] as i32 - g[0] as i32).abs() + (p[1] as i32 - g[1] as i32).abs() + (p[2] as i32 - g[2] as i32).abs() < 24 { continue; }
                        rs.push(ratio(p, g));
                    }
                }
            }
            if rs.len() < 4 { continue; }
            let r = rs.iter().copied().fold(0.0, f32::max);
            if worst.is_none_or(|(w, _)| r < w) { worst = Some((r, k)); }
        }
        if let Some((r, line)) = worst { out.push((i, line, r)); }
    }
    out
}
/// A lone mark (›, −, ×: no letter or digit in it) is a symbol, not text to read: it needs the 3:1 of a
/// graphic (WCAG 1.4.11), and a preset's rules are not about it.
fn is_mark<S, M: Clone>(ui: &Ui<S, M>, i: usize) -> bool { !ui.nodes()[i].text().unwrap_or_default().chars().any(char::is_alphanumeric) }

fn contrast<S, M: Clone>(ui: &mut Ui<S, M>, px: &[u8], ground: &[u8], out: &mut Vec<Finding>) {
    for (i, line, r) in line_contrast(ui, px, ground) {
        let legible = ui.nodes()[i].legible;
        let (level, limit) = if legible && is_mark(ui, i) { if r < 3.0 { (Level::P1, 3.0) } else { continue } }
            else if legible { if r < 3.0 { (Level::P0, 3.0) } else if r < MIN_CONTRAST { (Level::P1, MIN_CONTRAST) } else { continue } }
            else if r < 3.0 { (Level::P3, 3.0) } else { continue };
        out.push(Finding { level, key: ui.nodes()[i].key.clone(), kind: Kind::Contrast, measured: r, limit,
            fix: format!("line {} reads {r:.2}:1 against what is drawn under it (needs {limit}:1): use a stronger paint (`Paint::TEXT` or `Paint::INK`), or a calmer ground under it", line + 1) });
    }
}

fn overlap<S, M: Clone>(ui: &mut Ui<S, M>, out: &mut Vec<Finding>) {
    let glyphs = ui.glyphs().clone();
    let read: std::collections::HashSet<usize> = reading(ui).into_iter().collect();
    let posed = |ui: &Ui<S, M>, n: usize| matches!(&ui.nodes()[n].content, crate::el::Content::Text { pose: Some(_), .. });
    let atlas = ui.atlas().to_vec();
    let open = [-1e9, -1e9, 1e9, 1e9];
    let items: Vec<(usize, crate::scene::Instance, [f32; 4], f32)> = glyphs.log.iter().filter(|g| read.contains(&g.node)).filter_map(|g| {
        let mut inst = *ui.scene().instances.get(g.inst)?;
        if inst.paint[2] < 0.3 { return None; }
        inst.clip = open;
        let b = ink_box(&inst);
        (b[2] > b[0] && b[3] > b[1]).then_some((g.node, inst, b, g.z))
    }).collect();
    // A coarse grid over the boxes, so only neighbours are compared.
    const CELL: f32 = 48.0;
    let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
    for (k, it) in items.iter().enumerate() {
        let b = it.2;
        for gy in (b[1] / CELL) as i32..=(b[3] / CELL) as i32 { for gx in (b[0] / CELL) as i32..=(b[2] / CELL) as i32 { grid.entry((gx, gy)).or_default().push(k); } }
    }
    let mut worst: std::collections::HashMap<usize, f32> = Default::default();
    let mut seen = std::collections::HashSet::new();
    for cell in grid.values() {
        for (x, &a) in cell.iter().enumerate() {
            for &b in &cell[x + 1..] {
                if !seen.insert((a.min(b), a.max(b))) { continue; }
                let (ia, ib) = (&items[a], &items[b]);
                let (ba, bb) = (ia.2, ib.2);
                if ba[0] >= bb[2] || bb[0] >= ba[2] || ba[1] >= bb[3] || bb[1] >= ba[3] { continue; }
                // The smaller glyph's ink, sampled, under the other's.
                let (small, big) = if (ba[2] - ba[0]) * (ba[3] - ba[1]) <= (bb[2] - bb[0]) * (bb[3] - bb[1]) { (ia, ib) } else { (ib, ia) };
                let f = covered(&small.1, &small.2, &big.1, &atlas);
                if f <= 0.0 { continue; }
                for n in [ia.0, ib.0] { let e = worst.entry(n).or_insert(0.0); *e = e.max(f); }
            }
        }
    }
    let mut keys: Vec<(usize, f32)> = worst.into_iter().collect();
    keys.sort_by_key(|k| k.0);
    for (n, f) in keys {
        let key = ui.nodes()[n].key.clone();
        if posed(ui, n) {
            let limit = ui.nodes()[n].form.map_or(crate::form::FormFx::default().overlap, |fx| fx.overlap);
            if f > limit + 0.05 {
                out.push(Finding { level: Level::P2, key, kind: Kind::Overlap, measured: f, limit,
                    fix: format!("posed glyphs cover {:.0}% of each other's ink: lay it on a form (`.form(FormFx {{ overlap: {limit:.2}, .. }})`) so covered glyphs give way, or spread the poses", f * 100.0) });
            }
        } else if f > MAX_OVERLAP {
            out.push(Finding { level: Level::P1, key, kind: Kind::Overlap, measured: f, limit: MAX_OVERLAP,
                fix: format!("its glyphs and another text's cover {:.0}% of each other's ink: give the lines more height (`text_lh`) or move the elements apart", f * 100.0) });
        }
    }
}
/// The fraction of `a`'s ink that `b`'s ink covers, on a 7 × 7 grid over `a`'s box.
fn covered(a: &crate::scene::Instance, ab: &[f32; 4], b: &crate::scene::Instance, atlas: &[u8]) -> f32 {
    const N: usize = 7;
    let (w, h) = ((ab[2] - ab[0]) / N as f32, (ab[3] - ab[1]) / N as f32);
    let (mut ink, mut hid) = (0.0f32, 0.0f32);
    for iy in 0..N { for ix in 0..N {
        let (x, y) = (ab[0] + (ix as f32 + 0.5) * w, ab[1] + (iy as f32 + 0.5) * h);
        if crate::morph::cover_at(a, x, y, atlas) < 0.5 { continue; }
        ink += 1.0;
        if crate::morph::cover_at(b, x, y, atlas) >= 0.5 { hid += 1.0; }
    } }
    if ink < 3.0 { 0.0 } else { hid / ink }
}

/// How many pixels of `px` within `boxes` (device px, any overlap counted once) are drawn as ink: they
/// stand out from `ground` (the same frame with the glyphs hidden) by at least half the contrast of the
/// strongest of them (WCAG ratio), so an antialiased edge counts, a stroke left in a colour close to
/// what is under it does not.
pub fn ink_pixels(px: &[u8], ground: &[u8], (w, h): (u32, u32), boxes: &[[f32; 4]]) -> usize {
    if boxes.is_empty() || px.len() < (w * h * 4) as usize || ground.len() < px.len() { return 0; }
    let bb = boxes.iter().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
    let (x0, y0) = (bb[0].floor().max(0.0) as u32, bb[1].floor().max(0.0) as u32);
    let (x1, y1) = ((bb[2].ceil().max(0.0) as u32).min(w), (bb[3].ceil().max(0.0) as u32).min(h));
    if x1 <= x0 || y1 <= y0 { return 0; }
    let bw = (x1 - x0) as usize;
    let mut mask = vec![false; bw * (y1 - y0) as usize];
    for b in boxes {
        let (bx0, by0) = ((b[0].floor().max(x0 as f32) as u32), (b[1].floor().max(y0 as f32) as u32));
        let (bx1, by1) = ((b[2].ceil() as u32).min(x1), (b[3].ceil() as u32).min(y1));
        for y in by0..by1 { for x in bx0..bx1 { mask[(y - y0) as usize * bw + (x - x0) as usize] = true; } }
    }
    let mut rs = vec![];
    for y in y0..y1 {
        for x in x0..x1 {
            if !mask[(y - y0) as usize * bw + (x - x0) as usize] { continue; }
            let o = ((y * w + x) * 4) as usize;
            let (p, g) = ([px[o], px[o + 1], px[o + 2]], [ground[o], ground[o + 1], ground[o + 2]]);
            let d: i32 = (0..3).map(|c| (p[c] as i32 - g[c] as i32).abs()).sum();
            if d >= 24 { rs.push(ratio(p, g)); }
        }
    }
    let top = rs.iter().copied().fold(1.0f32, f32::max);
    let half = 1.0 + 0.5 * (top - 1.0);
    rs.iter().filter(|r| **r >= half).count()
}

/// The ink boxes (device px, within its clip and the window) of each text to read drawn at rest (no
/// glyph of it loose), by node.
fn resting_ink<S, M: Clone>(ui: &Ui<S, M>) -> Vec<(usize, Vec<[f32; 4]>)> {
    let (dw, dh) = ui.device_size();
    let glyphs = ui.glyphs();
    reading(ui).into_iter().filter_map(|i| {
        let mut boxes = vec![];
        for g in glyphs.log.iter().filter(|g| g.node == i) {
            let inst = ui.scene().instances.get(g.inst)?;
            if inst.extra[2] > 0.01 { return None; }
            if inst.paint[2] < 0.3 { continue; }
            let (b, c) = (ink_box(inst), inst.clip);
            let b = [b[0].max(c[0]).max(0.0), b[1].max(c[1]).max(0.0), b[2].min(c[2]).min(dw as f32), b[3].min(c[3]).min(dh as f32)];
            if b[2] > b[0] && b[3] > b[1] { boxes.push(b); }
        }
        (!boxes.is_empty()).then_some((i, boxes))
    }).collect()
}

/// Text to read that the material wears away at rest: `px` and `ground` the frame as drawn and with
/// the glyphs hidden, `plain` and `plain_ground` the same two in `plain` (the material swapped, nothing
/// else). Where a text to read at rest has fewer than `MIN_INK` of plain's ink pixels, it is a P1: its
/// strokes are broken or drawn in the colour of what is under them, which contrast (measured on the
/// strongest pixel) does not see.
pub fn eroded<S, M: Clone>(ui: &Ui<S, M>, (px, ground): (&[u8], &[u8]), (plain, plain_ground): (&[u8], &[u8]), material: &str) -> Vec<Finding> {
    let size = ui.device_size();
    let mut out = vec![];
    for (i, boxes) in resting_ink(ui) {
        let want = ink_pixels(plain, plain_ground, size, &boxes);
        if want < 20 { continue; }
        let got = ink_pixels(px, ground, size, &boxes);
        let share = got as f32 / want as f32;
        if share < MIN_INK {
            out.push(Finding { level: Level::P1, key: ui.nodes()[i].key.clone(), kind: Kind::Eroded, measured: share, limit: MIN_INK,
                fix: format!("`{material}` draws {:.0}% of the ink plain draws for this text at rest ({got} of {want} px): the material should keep text to read whole at rest, by the text's own paint (its instance), not by the pixels round it", share * 100.0) });
        }
    }
    out
}

/// Each text to read: its node, its size (logical px) and the height its clearly drawn glyphs have (device
/// px): its size as scaled by their poses. Flat text by its smallest glyph; posed text (on a path, on a
/// form, foreshortened at the rim on purpose) by the smallest tenth.
fn heights<S, M: Clone>(ui: &Ui<S, M>) -> Vec<(usize, f32, f32)> {
    let s = ui.scale();
    let glyphs = ui.glyphs();
    reading(ui).into_iter().filter_map(|i| {
        let size = ui.nodes()[i].para().map(|p| p.size)?;
        let mut hs: Vec<f32> = glyphs.log.iter().filter(|g| g.node == i).filter_map(|g| ui.scene().instances.get(g.inst)).filter(|inst| inst.paint[2] >= 0.5)
            .map(|inst| { let x = inst.xf; size * s * (x[1] * x[1] + (1.0 + x[3]) * (1.0 + x[3])).sqrt() }).collect();
        hs.sort_by(f32::total_cmp);
        let posed = matches!(&ui.nodes()[i].content, crate::el::Content::Text { pose: Some(_), .. });
        let least = if posed { hs.get(hs.len() / 10).copied() } else { hs.first().copied() }?;
        Some((i, size, least))
    }).collect()
}

fn small<S, M: Clone>(ui: &mut Ui<S, M>, out: &mut Vec<Finding>) {
    for (i, size, least) in heights(ui) {
        if least < MIN_TEXT_PX {
            out.push(Finding { level: Level::P1, key: ui.nodes()[i].key.clone(), kind: Kind::Small, measured: least, limit: MIN_TEXT_PX,
                fix: format!("drawn {least:.1} device px tall (needs {MIN_TEXT_PX}): set it at {:.0} px or more, or let it be decoration (`.legible(false)`)", (size * MIN_TEXT_PX / least).ceil().max(size + 1.0)) });
        }
    }
}

/// Running text (`preset::Of::Prose`): text to read no larger than the theme's body size, with more
/// than 40 characters in the element.
fn is_prose<S, M: Clone>(ui: &Ui<S, M>, i: usize, theme: &crate::style::Theme) -> bool {
    let n = &ui.nodes()[i];
    n.para().is_some_and(|p| p.size <= theme.sizes.body + 1e-3) && n.text().is_some_and(|t| t.chars().filter(|c| !c.is_whitespace()).count() > 40)
}
/// How many columns a character takes in a measure: 2 for wide ones (CJK, Hangul, full-width forms).
fn columns(c: char) -> usize {
    let u = c as u32;
    let wide = matches!(u, 0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD);
    if wide { 2 } else { 1 }
}

/// The audit (`audit`) of a page drawn in a preset: `by` its name, `rules` its rules, `theme` the theme it
/// was drawn in. Each rule is measured on the same frame; where a rule and the audit's own floor find the
/// same thing in the same element, one finding is kept: the rule's, a P1 (a P0 where the floor found a P0).
pub fn audit_under<S, M: Clone>(ui: &mut Ui<S, M>, px: &[u8], ground: &[u8], theme: &crate::style::Theme, by: &str, rules: &[crate::preset::Rule]) -> Vec<Finding> {
    let mut out = audit(ui, px, ground);
    let found = rules_found(ui, px, ground, theme, by, rules);
    for f in found {
        let level = match out.iter().position(|d| d.key == f.key && d.kind == f.kind) {
            Some(k) => { let l = out.remove(k).level; if l == Level::P0 { Level::P0 } else { Level::P1 } }
            None => Level::P1,
        };
        out.push(Finding { level, ..f });
    }
    sort(&mut out);
    out
}

fn rules_found<S, M: Clone>(ui: &mut Ui<S, M>, px: &[u8], ground: &[u8], theme: &crate::style::Theme, by: &str, rules: &[crate::preset::Rule]) -> Vec<Finding> {
    use crate::preset::{Of, Rule};
    let mut out = vec![];
    let s = ui.scale();
    let (prose_min, read_min) = rules.iter().fold((None::<f32>, None::<f32>), |(p, r), rule| match *rule {
        Rule::Contrast { of: Of::Prose, min } => (Some(p.map_or(min, |p: f32| p.max(min))), r),
        Rule::Contrast { of: Of::Read, min } => (p, Some(r.map_or(min, |r: f32| r.max(min)))),
        _ => (p, r),
    });
    if prose_min.is_some() || read_min.is_some() {
        for (i, line, r) in line_contrast(ui, px, ground) {
            if !ui.nodes()[i].legible || is_mark(ui, i) { continue; }
            let prose = prose_min.filter(|_| is_prose(ui, i, theme));
            let (limit, what) = match (prose, read_min) {
                (Some(p), Some(q)) if p >= q => (p, " for prose"),
                (Some(p), None) => (p, " for prose"),
                (_, Some(q)) => (q, ""),
                (None, None) => continue,
            };
            if r < limit {
                out.push(Finding { level: Level::P1, key: ui.nodes()[i].key.clone(), kind: Kind::Contrast, measured: r, limit,
                    fix: format!("line {} reads {r:.2}:1 against what is drawn under it ({by} needs {limit}:1{what}): use a stronger paint (`Paint::TEXT` or `Paint::INK`), or a calmer ground under it", line + 1) });
            }
        }
    }
    for rule in rules {
        match *rule {
            Rule::Contrast { .. } => {}
            Rule::TextSize { min } => for (i, size, least) in heights(ui) {
                let drawn = least / s;
                if drawn < min - 1e-3 {
                    out.push(Finding { level: Level::P1, key: ui.nodes()[i].key.clone(), kind: Kind::Small, measured: drawn, limit: min,
                        fix: format!("drawn {drawn:.1} px tall ({by} needs {min}): set it at {:.0} px or more, or let it be decoration (`.legible(false)`)", (size * min / drawn).ceil()) });
                }
            },
            Rule::Margin { min } => margin(ui, by, min, &mut out),
            Rule::Measure { max } => measure(ui, theme, by, max, &mut out),
            Rule::Accent { max } => {
                let n = px.len() / 4;
                if n == 0 { continue; }
                let a = theme.accent.map(|v| (v * 255.0).round() as i32);
                let hit = px.chunks_exact(4).filter(|q| (0..3).all(|k| (q[k] as i32 - a[k]).abs() <= 24)).count();
                let share = hit as f32 / n as f32;
                if share > max {
                    out.push(Finding { level: Level::P1, key: String::new(), kind: Kind::Accent, measured: share, limit: max,
                        fix: format!("the accent covers {:.1}% of the frame at rest ({by} allows {}%): keep the accent for what is held or dragged, and draw the rest in ink", share * 100.0, max * 100.0) });
                }
            }
        }
    }
    out
}

/// `Rule::Margin`: each text to read whose ink comes nearer than `min` logical px to an edge of the
/// window. Text in a scroll view is measured from the left and the right, and from the top while its
/// scroll view is at the top; never from the bottom.
fn margin<S, M: Clone>(ui: &Ui<S, M>, by: &str, min: f32, out: &mut Vec<Finding>) {
    const SIDES: [(&str, &str, &str); 4] = [("left", "right", "left"), ("top", "down", "top"), ("right", "left", "right"), ("bottom", "up", "bottom")];
    let s = ui.scale();
    let (w, h) = ui.logical_size();
    let glyphs = ui.glyphs();
    for i in reading(ui) {
        let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for g in glyphs.log.iter().filter(|g| g.node == i) {
            let Some(inst) = ui.scene().instances.get(g.inst) else { continue };
            if inst.paint[2] < 0.3 { continue; }
            let k = ink_box(inst);
            if k[2] <= k[0] || k[3] <= k[1] { continue; }
            b = [b[0].min(k[0]), b[1].min(k[1]), b[2].max(k[2]), b[3].max(k[3])];
        }
        if b[2] <= b[0] { continue; }
        let b = b.map(|v| v / s);
        // The nearest scroll view it is in, and whether that is scrolled away from its top.
        let mut a = ui.nodes()[i].parent;
        let mut scroll: Option<bool> = None;
        while let Some(p) = a { let n = &ui.nodes()[p]; if n.scroll { scroll = Some(ui.scroll_offset(&n.key) > 0.5); break; } a = n.parent; }
        let gaps = [b[0], b[1], w - b[2], h - b[3]];
        let measured = |k: usize| match (k, scroll) { (3, Some(_)) => false, (1, Some(away)) => !away, _ => true };
        let Some((k, gap)) = gaps.iter().copied().enumerate().filter(|(k, _)| measured(*k)).min_by(|x, y| x.1.total_cmp(&y.1)) else { continue };
        if gap < min - 0.5 {
            let more = (min - gap).ceil();
            let (side, way, pad) = SIDES[k];
            out.push(Finding { level: Level::P1, key: ui.nodes()[i].key.clone(), kind: Kind::Margin, measured: gap.max(0.0), limit: min,
                fix: format!("its ink ends {:.0} px from the window's {side} edge ({by} needs {min}): move it {way} {more:.0} px, or give the page {more:.0} px more padding at the {pad}", gap.max(0.0)) });
        }
    }
}

/// `Rule::Measure`: the longest line of each running text (`is_prose`), in characters (wide ones count 2).
fn measure<S, M: Clone>(ui: &mut Ui<S, M>, theme: &crate::style::Theme, by: &str, max: f32, out: &mut Vec<Finding>) {
    for i in reading(ui) {
        if !is_prose(ui, i, theme) { continue; }
        let lines = ui.line_texts(i);
        let Some((k, n)) = lines.iter().map(|l| l.trim_end().chars().map(columns).sum::<usize>()).enumerate().max_by_key(|x| x.1) else { continue };
        if n as f32 > max + 1e-3 {
            out.push(Finding { level: Level::P1, key: ui.nodes()[i].key.clone(), kind: Kind::Measure, measured: n as f32, limit: max,
                fix: format!("line {} runs {n} characters ({by} allows {max}): make the column about {:.0}% narrower, or set the text larger", k + 1, (1.0 - max / n as f32) * 100.0) });
        }
    }
}
