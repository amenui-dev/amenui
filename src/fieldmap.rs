//! The field map (DESIGN.md §1.29): one coarse grid over the whole window that keeps, at every node, an
//! energy (how much the things that read it loosen) and a flow (which way they are pushed). Moving
//! things stir it as they go (`El::stirr`), the state can stir it (`Cmd::Stir`), and it settles back to
//! exactly zero in a bounded time, so a page at rest is a still page (rule 3). Stepped at a fixed 1/60 s
//! from the runtime's tick, so the same inputs give the same bytes. No GPU here: the renderer uploads
//! `bytes()` as a small texture when they change; the CPU model (`energy_at`, `flow_at`) reads the same
//! quantised values the shaders read.
#![warn(missing_docs)]
use crate::field::noise;
use std::rc::Rc;

/// One step of the map, seconds.
pub const STEP: f32 = 1.0 / 60.0;
/// The linear part of the decay, per second: with it a disturbance reaches exactly zero in finite time
/// (from `e0` at most `τ ln(1 + e0 / (FLOOR τ))`; 1.24 s for the default energy life).
pub const FLOOR: f32 = 0.15;
/// The most nodes along either side (the texture is this square; a larger window takes a larger cell).
pub const MAX_NODES: usize = 512;

/// The field map's numbers (in `Look::map`, a preset's `interaction.map`), each a number control.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MapFx {
    /// Node spacing, logical px.
    pub cell: f32,
    /// How much energy a moving `stirr` element leaves (times its own `Stirr::energy`); 0 is no trail.
    pub stirr: f32,
    /// The trail's radius, logical px (plus half the element's shorter side).
    pub stirr_radius: f32,
    /// How much flow a stir puts in (0 none, 1 the direction it moved, more is stronger).
    pub flow: f32,
    /// How far the flow pushes what it reaches at most, logical px.
    pub drift: f32,
    /// How long energy lives (s): its exponential part.
    pub energy_life: f32,
    /// How long flow lives (s): its exponential part.
    pub flow_life: f32,
    /// How much of the 3 × 3 average every node takes each step.
    pub bleed: f32,
    /// The flow turns by a slow noise each step, at most `meander × 40°` a second: trails go in curves.
    pub meander: f32,
    /// Between nodes that have ink, spreading is this much stronger: energy creeps along type and shapes.
    pub creep: f32,
    /// A floor of energy at rest that rises and falls over `breath_s` seconds (0: none; a page with it
    /// never rests, as a flow field with speed does not).
    pub breath: f32,
    /// How long one breath takes (s).
    pub breath_s: f32,
}
impl Default for MapFx {
    fn default() -> Self { Self { cell: 16.0, stirr: 0.6, stirr_radius: 48.0, flow: 1.0, drift: 6.0, energy_life: 0.45, flow_life: 0.5, bleed: 0.2, meander: 0.35, creep: 0.3, breath: 0.0, breath_s: 6.0 } }
}
/// Each number's name, range, step and decimals, in the order of the fields (for number controls).
pub const MAP_PARAMS: [(&str, f32, f32, f32, usize, &str); 12] = [
    ("cell", 8.0, 32.0, 1.0, 0, "px"), ("stirr", 0.0, 1.0, 0.05, 2, ""), ("stirr_radius", 8.0, 160.0, 2.0, 0, "px"), ("flow", 0.0, 2.0, 0.05, 2, ""),
    ("drift", 0.0, 24.0, 0.5, 1, "px"), ("energy_life", 0.05, 3.0, 0.05, 2, "s"), ("flow_life", 0.05, 3.0, 0.05, 2, "s"), ("bleed", 0.0, 1.0, 0.05, 2, ""),
    ("meander", 0.0, 1.0, 0.05, 2, ""), ("creep", 0.0, 1.0, 0.05, 2, ""), ("breath", 0.0, 0.3, 0.01, 2, ""), ("breath_s", 2.0, 12.0, 0.5, 1, "s"),
];
impl MapFx {
    /// The number at place `i` of `MAP_PARAMS`.
    pub fn get(&self, i: usize) -> f32 {
        [self.cell, self.stirr, self.stirr_radius, self.flow, self.drift, self.energy_life, self.flow_life, self.bleed, self.meander, self.creep, self.breath, self.breath_s][i]
    }
    /// Set the number at place `i` of `MAP_PARAMS`, clamped to its range.
    pub fn set(&mut self, i: usize, v: f32) {
        let (_, lo, hi, ..) = MAP_PARAMS[i];
        let v = v.clamp(lo, hi);
        match i {
            0 => self.cell = v, 1 => self.stirr = v, 2 => self.stirr_radius = v, 3 => self.flow = v, 4 => self.drift = v, 5 => self.energy_life = v,
            6 => self.flow_life = v, 7 => self.bleed = v, 8 => self.meander = v, 9 => self.creep = v, 10 => self.breath = v, _ => self.breath_s = v,
        }
    }
}

/// One stir: at a point (logical px), in a direction (unit or zero), this much energy and flow, over a
/// radius, only on nodes inside `scope` (logical px box x0, y0, x1, y1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stir {
    /// Where, logical px.
    pub at: [f32; 2],
    /// Which way (unit, or zero for energy alone).
    pub dir: [f32; 2],
    /// How much energy.
    pub energy: f32,
    /// How much flow.
    pub flow: f32,
    /// How far it reaches, logical px.
    pub radius: f32,
    /// The box it is kept to (x0, y0, x1, y1, logical px).
    pub scope: [f32; 4],
}
/// A scope over everything.
pub const EVERYWHERE: [f32; 4] = [-1e6, -1e6, 1e6, 1e6];

/// A gust (`Cmd::Gust`, DESIGN.md §1.29): a band `width` logical px wide that travels along `dir` at
/// `speed` logical px a second from the line through `from` across its scope, flow along `dir` in it and
/// a little energy, and behind its trailing edge for `wake_s` seconds a wake of flow the other way. It is
/// worked out from the time at every node and laid over the map each step, never put into it: where it
/// has been the map is as it was, and once it has left its scope it asks for nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gust {
    /// A point on the line its leading edge starts from, logical px.
    pub from: [f32; 2],
    /// Which way it travels (made a unit).
    pub dir: [f32; 2],
    /// How fast, logical px a second.
    pub speed: f32,
    /// How wide the band is along `dir`, logical px.
    pub width: f32,
    /// How far the far side of its scope lags behind the near side, logical px: across `dir`, at a
    /// share `n` (0..1) of the scope's extent, the band is `slant × n^curve` behind.
    pub slant: f32,
    /// The power of that lag.
    pub curve: f32,
    /// How strong: the flow at the band's middle, times the look's `MapFx::flow`.
    pub strength: f32,
    /// The energy at the band's middle, times `strength`.
    pub energy: f32,
    /// The wake's flow at its middle, a share of `strength`, the other way.
    pub wake: f32,
    /// How long the wake lasts behind the trailing edge, s.
    pub wake_s: f32,
}
impl Default for Gust {
    fn default() -> Self { Self { from: [0.0, 0.0], dir: [1.0, 0.0], speed: 240.0, width: 330.0, slant: 0.0, curve: 1.0, strength: 1.0, energy: 0.2, wake: 0.105, wake_s: 0.5 } }
}
impl Gust {
    /// Its energy and flow at `p` (logical px), `t` s after it started, in a scope `b` (x0, y0, x1, y1):
    /// zero outside the scope, before the band and after the wake.
    pub fn at(&self, p: [f32; 2], t: f32, b: [f32; 4]) -> (f32, [f32; 2]) {
        if p[0] < b[0] || p[1] < b[1] || p[0] >= b[2] || p[1] >= b[3] { return (0.0, [0.0; 2]); }
        let d = self.unit();
        let perp = [-d[1], d[0]];
        let (lo, hi) = Self::extent(b, perp);
        let n = ((p[0] * perp[0] + p[1] * perp[1] - lo) / (hi - lo).max(1e-3)).clamp(0.0, 1.0);
        let lag = self.slant * n.powf(self.curve.max(1e-3));
        let a = (p[0] - self.from[0]) * d[0] + (p[1] - self.from[1]) * d[1];
        // How far behind the leading edge the point is.
        let s = self.speed * t - lag - a;
        let w = self.width.max(1e-3);
        let wl = (self.wake_s * self.speed).max(0.0);
        if (0.0..w).contains(&s) {
            let k = 0.5 * (1.0 - (std::f32::consts::TAU * s / w).cos());
            let f = self.strength * k;
            (self.energy * f, [d[0] * f, d[1] * f])
        } else if s >= w && s < w + wl {
            let u = (s - w) / wl;
            let f = -self.strength * self.wake * (std::f32::consts::PI * u).sin().powi(2);
            (0.0, [d[0] * f, d[1] * f])
        } else { (0.0, [0.0; 2]) }
    }
    /// Whether it has passed the whole of scope `b` (its wake too) `t` s after it started.
    pub fn gone(&self, t: f32, b: [f32; 4]) -> bool {
        let d = self.unit();
        let (_, far) = Self::extent([b[0] - self.from[0], b[1] - self.from[1], b[2] - self.from[0], b[3] - self.from[1]], d);
        self.speed * t - self.slant.max(0.0) - far >= self.width + (self.wake_s * self.speed).max(0.0)
    }
    fn unit(&self) -> [f32; 2] { let l = (self.dir[0] * self.dir[0] + self.dir[1] * self.dir[1]).sqrt(); if l > 0.0 { [self.dir[0] / l, self.dir[1] / l] } else { [1.0, 0.0] } }
    /// The least and most of `dot(corner, v)` over the box's corners.
    fn extent(b: [f32; 4], v: [f32; 2]) -> (f32, f32) {
        let c = [b[0] * v[0] + b[1] * v[1], b[2] * v[0] + b[1] * v[1], b[0] * v[0] + b[3] * v[1], b[2] * v[0] + b[3] * v[1]];
        (c.iter().copied().fold(f32::MAX, f32::min), c.iter().copied().fold(f32::MIN, f32::max))
    }
}

/// The map's bytes as the renderer and `dirty` see them: nodes across and down, the cell (logical px)
/// and RGBA8 per node (r = energy × 255, g and b = 128 + 127 × flow), shared so a frame's copy is free.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Nodes across.
    pub cols: usize,
    /// Nodes down.
    pub rows: usize,
    /// The distance between nodes, logical px.
    pub cell: f32,
    /// RGBA8 per node.
    pub bytes: Rc<Vec<u8>>,
    /// Whether any node is not at rest.
    pub nonzero: bool,
}
impl Default for Snapshot { fn default() -> Self { Self { cols: 0, rows: 0, cell: 16.0, bytes: Rc::new(vec![]), nonzero: false } } }
impl Snapshot {
    /// The box of nodes (logical px, grown by one cell for the bilinear reads) whose bytes differ from
    /// `other`'s, or `None` when none do. Different sizes differ everywhere.
    pub fn changed(&self, other: &Snapshot) -> Option<[f32; 4]> {
        if Rc::ptr_eq(&self.bytes, &other.bytes) { return None; }
        if (self.cols, self.rows, self.cell) != (other.cols, other.rows, other.cell) || self.bytes.len() != other.bytes.len() {
            return Some(EVERYWHERE);
        }
        self.nodes(|k| self.bytes[k * 4..k * 4 + 4] != other.bytes[k * 4..k * 4 + 4])
    }
    /// Where any node is not at rest (logical px, grown by a cell), or `None` when the map is at rest.
    pub fn lit(&self) -> Option<[f32; 4]> {
        if !self.nonzero { return None; }
        self.nodes(|k| { let p = &self.bytes[k * 4..k * 4 + 3]; p != [0, 128, 128] })
    }
    /// The box (logical px, grown by a cell) of the nodes where `f` holds.
    fn nodes(&self, f: impl Fn(usize) -> bool) -> Option<[f32; 4]> {
        let mut b = [usize::MAX, usize::MAX, 0, 0];
        let mut any = false;
        for k in (0..self.bytes.len() / 4).filter(|&k| f(k)) { let (i, j) = (k % self.cols, k / self.cols); b = [b[0].min(i), b[1].min(j), b[2].max(i), b[3].max(j)]; any = true; }
        any.then(|| { let c = self.cell; [(b[0] as f32 - 1.0) * c, (b[1] as f32 - 1.0) * c, (b[2] as f32 + 1.0) * c, (b[3] as f32 + 1.0) * c] })
    }
}

/// The map itself.
#[derive(Clone, Debug)]
pub struct FieldMap {
    /// Nodes across.
    pub cols: usize,
    /// Nodes down.
    pub rows: usize,
    /// The distance between nodes, logical px.
    pub cell: f32,
    e: Vec<f32>,
    u: Vec<f32>,
    v: Vec<f32>,
    /// How much ink each node has (0..1, for `creep`), and whether it was taken for this scene.
    ink: Vec<f32>,
    /// Each node's energy and flow length when something was last put in, and the time since (s): a node
    /// inside a box with lives of its own goes down from them along the look's curve (`FieldMap::step`).
    put_e: Vec<f32>,
    put_f: Vec<f32>,
    age: Vec<f32>,
    /// Time owed to steps not taken yet, and the map's own clock (s), which runs only while it moves.
    acc: f64,
    /// The map's own clock (s).
    pub clock: f32,
    /// Boxes nodes are kept at zero in (logical px; `El::calm`).
    pub calm: Vec<[f32; 4]>,
    /// The viewer asked for less motion: energy as usual, no flow, lives halved.
    pub reduced: bool,
    /// Whether anything was ever put in (tests: a preset with no trail writes nothing).
    pub written: bool,
    /// Boxes (logical px) whose nodes keep their energy and flow for lives of their own, s (`El::map_life`).
    pub lives: Vec<([f32; 4], f32, f32)>,
    /// The gusts going on: each, when it started on the map's clock, its scope (logical px), and how much
    /// of its flow the look takes (`MapFx::flow`).
    gusts: Vec<(Gust, f32, [f32; 4], f32)>,
    snap: Snapshot,
    /// Buffers kept between steps.
    scratch: [Vec<f32>; 4],
}
impl Default for FieldMap { fn default() -> Self { Self::new(0.0, 0.0, 16.0) } }

// Rounded to the nearest byte (halves up; `+ 0.5` and truncation, which needs no library call).
fn q8(x: f32) -> u8 { (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 }
fn qf(x: f32) -> u8 { (x.clamp(-1.0, 1.0) * 127.0 + 128.5) as u8 }

impl FieldMap {
    /// A map over a `w × h` window (logical px), nodes `cell` apart (larger when the window needs more
    /// than `MAX_NODES` along a side). Node (i, j) is at (i × cell, j × cell); one extra node past each
    /// far edge, so every point of the window has four nodes around it.
    pub fn new(w: f32, h: f32, cell: f32) -> Self {
        let (cols, rows, cell) = Self::grid(w, h, cell);
        let n = cols * rows;
        let mut m = Self { cols, rows, cell, e: vec![0.0; n], u: vec![0.0; n], v: vec![0.0; n], ink: vec![0.0; n], put_e: vec![0.0; n], put_f: vec![0.0; n], age: vec![0.0; n], acc: 0.0, clock: 0.0, calm: vec![], reduced: false, written: false, lives: vec![], gusts: vec![], snap: Snapshot::default(), scratch: Default::default() };
        m.encode();
        m
    }
    /// Fit the map to a window and a cell size: a new size starts over at zero (it is a picture of the
    /// moment, not data). Whether it changed.
    pub fn fit(&mut self, w: f32, h: f32, cell: f32) -> bool {
        if Self::grid(w, h, cell) == (self.cols, self.rows, self.cell) { return false; }
        let fresh = Self::new(w, h, cell);
        let (reduced, written) = (self.reduced, self.written);
        *self = fresh;
        self.reduced = reduced;
        self.written = written;
        true
    }
    /// The columns, rows and cell size of a map of a window.
    fn grid(w: f32, h: f32, cell: f32) -> (usize, usize, f32) {
        let cell = cell.max(1.0).max(w / (MAX_NODES - 2) as f32).max(h / (MAX_NODES - 2) as f32);
        ((w / cell).floor() as usize + 2, (h / cell).floor() as usize + 2, cell)
    }
    fn idx(&self, i: usize, j: usize) -> usize { j * self.cols + i }
    /// Nothing in the box `b` (logical px, x0, y0, x1, y1) any more: the energy and flow of every node
    /// within a cell of it set to zero (what was there went with the element that put it there).
    pub fn clear_in(&mut self, b: [f32; 4]) {
        let mut any = false;
        for j in 0..self.rows { for i in 0..self.cols {
            let (x, y) = (i as f32 * self.cell, j as f32 * self.cell);
            if x < b[0] - self.cell || y < b[1] - self.cell || x > b[2] + self.cell || y > b[3] + self.cell { continue; }
            let k = self.idx(i, j);
            if self.e[k] != 0.0 || self.u[k] != 0.0 || self.v[k] != 0.0 { (self.e[k], self.u[k], self.v[k]) = (0.0, 0.0, 0.0); any = true; }
        } }
        if any { self.encode(); }
    }
    /// Whether anything is in the map now (or it breathes): the runtime keeps asking for frames.
    pub fn moving(&self, fx: &MapFx) -> bool { self.snap.nonzero || self.breathes(fx) || !self.gusts.is_empty() }
    /// Start a gust now, over scope `b` (logical px), its flow times `flow` (the look's `MapFx::flow`).
    pub fn gust(&mut self, g: Gust, b: [f32; 4], flow: f32) { self.gusts.push((g, self.clock, b, flow)); self.written = true; self.encode(); }
    /// How many gusts are going on.
    pub fn gusts(&self) -> usize { self.gusts.len() }
    fn breathes(&self, fx: &MapFx) -> bool { fx.breath > 0.0 && !self.reduced }
    /// How many nodes are not zero (the readout's line).
    pub fn nonzero(&self) -> usize { self.snap.bytes.chunks_exact(4).filter(|p| p[0] != 0 || p[1] != 128 || p[2] != 128).count() }
    /// The mean energy over the nodes (the `LEVEL` readout).
    pub fn level(&self) -> f32 { let n = self.snap.bytes.len() / 4; if n == 0 { 0.0 } else { self.snap.bytes.chunks_exact(4).map(|p| p[0] as f32 / 255.0).sum::<f32>() / n as f32 } }
    /// The bytes as uploaded, with their size.
    pub fn snapshot(&self) -> &Snapshot { &self.snap }
    /// Raw node values (energy, flow), for tests.
    pub fn node(&self, i: usize, j: usize) -> (f32, [f32; 2]) { let k = self.idx(i.min(self.cols - 1), j.min(self.rows - 1)); (self.e[k], [self.u[k], self.v[k]]) }
    /// Where the most energy is now: the row of nodes (for the `OSC` readout) and its energy.
    pub fn strongest_row(&self) -> (usize, f32) {
        let mut best = (0, 0.0);
        for j in 0..self.rows { let s = (0..self.cols).map(|i| self.byte_e(i, j)).fold(0.0, f32::max); if s > best.1 { best = (j, s); } }
        best
    }
    /// The quantised energy of a node, as the shader reads it.
    pub fn byte_e(&self, i: usize, j: usize) -> f32 { self.snap.bytes.get(self.idx(i.min(self.cols - 1), j.min(self.rows - 1)) * 4).map_or(0.0, |b| *b as f32 / 255.0) }
    fn bilinear(&self, x: f32, y: f32, c: usize) -> f32 {
        if self.snap.bytes.is_empty() { return 0.0; }
        let (qx, qy) = ((x / self.cell).max(0.0), (y / self.cell).max(0.0));
        let (i, j) = (qx.floor() as usize, qy.floor() as usize);
        let (fx, fy) = (qx - qx.floor(), qy - qy.floor());
        let b = &self.snap.bytes;
        let at = |i: usize, j: usize| { let k = (j.min(self.rows - 1) * self.cols + i.min(self.cols - 1)) * 4 + c; b[k] as f32 / 255.0 };
        let (a0, a1, b0, b1) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
        let top = a0 + (a1 - a0) * fx;
        let bot = b0 + (b1 - b0) * fx;
        top + (bot - top) * fy
    }
    /// The energy at a point (logical px), bilinear between the quantised nodes: what the shaders read.
    pub fn energy_at(&self, x: f32, y: f32) -> f32 { if !self.snap.nonzero { 0.0 } else { self.bilinear(x, y, 0) } }
    /// The flow at a point (logical px; length at most about 1), as the shaders read it.
    pub fn flow_at(&self, x: f32, y: f32) -> [f32; 2] {
        if !self.snap.nonzero { return [0.0; 2]; }
        let d = |c: usize| { let f = (self.bilinear(x, y, c) * 255.0 - 128.0) / 127.0; if f.abs() < 0.5 / 127.0 { 0.0 } else { f } };
        [d(1), d(2)]
    }

    /// Put a stir in (nodes inside its radius and its scope: energy and flow by `(1 − (d / R)²)²`).
    pub fn stir(&mut self, s: Stir) { self.put(s); self.encode(); }
    fn put(&mut self, s: Stir) {
        if s.energy <= 0.0 && s.flow <= 0.0 { return; }
        // The nodes are a cell apart: what is narrower is put in a cell wide.
        let c = self.cell;
        let r = s.radius.max(c);
        let (i0, i1) = (((s.at[0] - r) / c).floor().max(0.0) as usize, (((s.at[0] + r) / c).ceil().max(0.0) as usize).min(self.cols - 1));
        let (j0, j1) = (((s.at[1] - r) / c).floor().max(0.0) as usize, (((s.at[1] + r) / c).ceil().max(0.0) as usize).min(self.rows - 1));
        let flow = if self.reduced { 0.0 } else { s.flow };
        let (sc, rr) = (s.scope, 1.0 / (r * r));
        for j in j0..=j1 {
            let y = j as f32 * c;
            if y < sc[1] || y >= sc[3] { continue; }
            let dy = y - s.at[1];
            for i in i0..=i1 {
                let x = i as f32 * c;
                if x < sc[0] || x >= sc[2] { continue; }
                if !self.calm.is_empty() && self.calm.iter().any(|b| x >= b[0] && y >= b[1] && x < b[2] && y < b[3]) { continue; }
                let dx = x - s.at[0];
                let d2 = (dx * dx + dy * dy) * rr;
                if d2 >= 1.0 { continue; }
                let w = (1.0 - d2) * (1.0 - d2);
                let k = self.idx(i, j);
                self.e[k] = (self.e[k] + s.energy * w).min(1.0);
                if flow > 0.0 {
                    let (u, v) = (self.u[k] + s.dir[0] * flow * w, self.v[k] + s.dir[1] * flow * w);
                    let l = (u * u + v * v).sqrt();
                    let g = if l > 1.0 { 1.0 / l } else { 1.0 };
                    self.u[k] = u * g;
                    self.v[k] = v * g;
                }
                (self.put_e[k], self.put_f[k], self.age[k]) = (self.e[k], (self.u[k] * self.u[k] + self.v[k] * self.v[k]).sqrt(), 0.0);
                self.written = true;
            }
        }
    }
    /// Stir along a segment from `a` to `b` (logical px): one stir every half cell, in its direction.
    pub fn stir_along(&mut self, a: [f32; 2], b: [f32; 2], energy: f32, flow: f32, radius: f32, scope: [f32; 4]) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        let dir = if len > 1e-6 { [dx / len, dy / len] } else { [0.0; 2] };
        let n = ((len / (self.cell * 0.5)).ceil() as usize).max(1);
        for k in 1..=n {
            let t = k as f32 / n as f32;
            self.put(Stir { at: [a[0] + dx * t, a[1] + dy * t], dir, energy, flow, radius, scope });
        }
        self.encode();
    }
    /// Which nodes have ink (`creep`): the coverage of the scene at each node, 0..1.
    pub fn set_ink(&mut self, ink: Vec<f32>) { if ink.len() == self.e.len() { self.ink = ink; } }
    /// How much ink the node nearest (x, y) (logical px) has, 0..1 (`set_ink`).
    pub fn ink_at(&self, x: f32, y: f32) -> f32 {
        if self.cols == 0 || self.rows == 0 || self.ink.is_empty() { return 0.0; }
        let (i, j) = (((x / self.cell).round().max(0.0) as usize).min(self.cols - 1), ((y / self.cell).round().max(0.0) as usize).min(self.rows - 1));
        self.ink[j * self.cols + i]
    }

    /// Let `dt` seconds pass: as many fixed steps as are owed. Whether the bytes changed.
    pub fn advance(&mut self, dt: f32, fx: &MapFx) -> bool {
        if !self.snap.nonzero && !self.breathes(fx) && self.gusts.is_empty() && self.e.iter().all(|x| *x == 0.0) { self.acc = 0.0; return false; }
        self.acc += dt.max(0.0) as f64;
        let before = self.snap.bytes.clone();
        while self.acc >= STEP as f64 - 1e-7 {
            self.acc -= STEP as f64;
            self.step(fx);
        }
        let now = self.clock;
        self.gusts.retain(|(g, t0, b, _)| !g.gone(now - t0, *b));
        self.encode();
        *before != *self.snap.bytes
    }
    /// One step of 1/60 s: spread (with `creep` along ink), decay to exactly zero, turn the flow by the
    /// meander, the breath's floor, nothing in calm boxes.
    pub fn step(&mut self, fx: &MapFx) {
        let dt = STEP;
        let life = |l: f32| if self.reduced { l * 0.5 } else { l }.max(1e-3);
        let (le, lf) = (life(fx.energy_life), life(fx.flow_life));
        let (cols, rows) = (self.cols, self.rows);
        let bleed = fx.bleed.clamp(0.0, 1.0);
        let creep = fx.creep.clamp(0.0, 1.0);
        let breathing = self.breathes(fx);
        // Only the nodes that hold something, and one ring around them, can change (all of them while
        // the map breathes).
        let (i0, j0, i1, j1) = if breathing { (0, 0, cols - 1, rows - 1) } else {
            let mut bx = (usize::MAX, usize::MAX, 0, 0);
            for j in 0..rows { for i in 0..cols { let k = j * cols + i; if self.e[k] != 0.0 || self.u[k] != 0.0 || self.v[k] != 0.0 { bx = (bx.0.min(i), bx.1.min(j), bx.2.max(i), bx.3.max(j)); } } }
            if bx.0 == usize::MAX { self.clock += dt; return; }
            (bx.0.saturating_sub(1), bx.1.saturating_sub(1), (bx.2 + 1).min(cols - 1), (bx.3 + 1).min(rows - 1))
        };
        // Boxes with lives of their own (`El::map_life`): a node takes the first box it is in. A life is
        // when what is put in there is back at rest, however much and however wide: nothing spreads there
        // (no bleed, no creep), and each node goes down from what was last put in it along the look's
        // curve from 1 (`curve`) slowed to end at the life, for energy and flow each (the meander turning
        // the flow as many times slower). Every node of a stroke put in at once keeps its share of the
        // others, so a path keeps its width and closes all at once, the part put in first first.
        let own: Vec<Own> = self.lives.iter().map(|&(b, e, f)| {
            let (e, f) = (life(e.clamp(0.1, 30.0)), life(f.clamp(0.1, 30.0)));
            Own { b, e, f, slow: f / settle_time(1.0, lf) }
        }).collect();
        let own_at = |i: usize, j: usize| { let (x, y) = (i as f32 * self.cell, j as f32 * self.cell); own.iter().find(|o| x >= o.b[0] && y >= o.b[1] && x < o.b[2] && y < o.b[3]) };
        if bleed > 0.0 {
            // Last step's values (kept buffers), and each box row's sums over three columns (the rows
            // and columns outside the box hold zero), so a bare node's 3 × 3 mean is three additions.
            let [mut e0, mut u0, mut v0, mut h] = std::mem::take(&mut self.scratch);
            for (dst, src) in [(&mut e0, &self.e), (&mut u0, &self.u), (&mut v0, &self.v)] { dst.clear(); dst.extend_from_slice(src); }
            let bw = i1 - i0 + 1;
            h.clear();
            h.resize(bw * (j1 - j0 + 1) * 3, 0.0);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let o = ((j - j0) * bw + i - i0) * 3;
                    for n in i.saturating_sub(1)..=(i + 1).min(cols - 1) { let k = j * cols + n; h[o] += e0[k]; h[o + 1] += u0[k]; h[o + 2] += v0[k]; }
                }
            }
            for j in j0..=j1 {
                for i in i0..=i1 {
                    if own_at(i, j).is_some() { continue; }
                    let k = j * cols + i;
                    let ik = self.ink[k];
                    let (mut se, mut su, mut sv, mut sw) = (0.0, 0.0, 0.0, 0.0);
                    if creep == 0.0 || ik == 0.0 {
                        for nj in j.saturating_sub(1).max(j0)..=(j + 1).min(j1) { let o = ((nj - j0) * bw + i - i0) * 3; se += h[o]; su += h[o + 1]; sv += h[o + 2]; }
                        sw = (((i + 1).min(cols - 1) - i.saturating_sub(1) + 1) * ((j + 1).min(rows - 1) - j.saturating_sub(1) + 1)) as f32;
                    } else {
                        for nj in j.saturating_sub(1)..=(j + 1).min(rows - 1) {
                            for ni in i.saturating_sub(1)..=(i + 1).min(cols - 1) {
                                let n = nj * cols + ni;
                                // Inked neighbours of an inked node count for more and bare ones for
                                // less: energy creeps along ink.
                                let w = if n == k { 1.0 } else { let inn = self.ink[n]; (1.0 + 4.0 * creep * ik * inn) * (1.0 - creep * ik * (1.0 - inn)) };
                                se += w * e0[n]; su += w * u0[n]; sv += w * v0[n]; sw += w;
                            }
                        }
                    }
                    let b = (bleed + creep * (1.0 - bleed) * ik).min(1.0);
                    self.e[k] = e0[k] + (se / sw - e0[k]) * b;
                    self.u[k] = u0[k] + (su / sw - u0[k]) * b;
                    self.v[k] = v0[k] + (sv / sw - v0[k]) * b;
                }
            }
            self.scratch = [e0, u0, v0, h];
        }
        let ke = (-dt / le).exp();
        let kf = (-dt / lf).exp();
        let turn = fx.meander.clamp(0.0, 1.0) * 40f32.to_radians() * dt;
        let t = self.clock;
        let c = self.cell;
        let floor = move |x: f32, y: f32| if breathing {
            let ph = t / fx.breath_s.max(0.5) + 0.35 * noise(x * 0.004, y * 0.004);
            fx.breath * (0.5 - 0.5 * (std::f32::consts::TAU * ph).cos())
        } else { 0.0 };
        // The meander's slow field, a product of two travelling waves (one per column and one per row, so
        // a step costs a few sines per side, not one noise per node): −1..1, about 160 px across.
        let wx: Vec<f32> = if turn > 0.0 { (i0..=i1).map(|i| (i as f32 * c * 0.04 + t * 0.5 + 1.3).sin()).collect() } else { vec![] };
        let wy: Vec<f32> = if turn > 0.0 { (j0..=j1).map(|j| (j as f32 * c * 0.033 - t * 0.4).cos()).collect() } else { vec![] };
        for j in j0..=j1 {
            for i in i0..=i1 {
                let k = j * cols + i;
                let (x, y) = (i as f32 * c, j as f32 * c);
                let o = own_at(i, j);
                let l0 = (self.u[k] * self.u[k] + self.v[k] * self.v[k]).sqrt();
                // In a box with lives of its own: what was put in, down the curve, never more than now (a
                // node cleared stays clear).
                let (ne, nl, ft) = match o {
                    Some(o) => {
                        self.age[k] += dt;
                        let a = self.age[k];
                        ((self.put_e[k] * curve(a / o.e, le, 0.5 / 255.0)).min(self.e[k]), (self.put_f[k] * curve(a / o.f, lf, 0.5 / 127.0)).min(l0), o.slow)
                    }
                    None => ((self.e[k] * ke - FLOOR * dt).max(0.0), (l0 * kf - FLOOR * dt).max(0.0), 1.0),
                };
                self.e[k] = ne.max(floor(x, y));
                let (u, v) = (self.u[k], self.v[k]);
                let l = l0;
                if l > 0.0 {
                    // The meander turns the flow by a slow noise; turning leaves its length alone.
                    // (At most 0.7° a step: sin a ≈ a and cos a ≈ 1 − a²/2 change the length by under
                    // a⁴/8, and the length is set again just below.)
                    let a = if turn > 0.0 { wx[i - i0] * wy[j - j0] * turn / ft } else { 0.0 };
                    let (s, co) = (a, 1.0 - 0.5 * a * a);
                    let g = nl / l;
                    self.u[k] = (u * co - v * s) * g;
                    self.v[k] = (u * s + v * co) * g;
                }
            }
        }
        // The nodes inside a calm box (node (i, j) is at (i, j) × cell; x ≥ b[0] and x < b[2]).
        let span = |lo: f32, hi: f32, n: usize| ((lo / c).ceil().max(0.0) as usize, (((hi / c).ceil().max(0.0)) as usize).min(n));
        for b in &self.calm {
            let ((ci0, ci1), (cj0, cj1)) = (span(b[0], b[2], cols), span(b[1], b[3], rows));
            for j in cj0.max(j0)..cj1.min(j1 + 1) {
                for i in ci0.max(i0)..ci1.min(i1 + 1) { let k = j * cols + i; self.e[k] = 0.0; self.u[k] = 0.0; self.v[k] = 0.0; }
            }
        }
        self.clock += dt;
    }
    /// Quantise into the bytes the renderer uploads (a new shared copy only when they change).
    fn encode(&mut self) {
        let n = self.cols * self.rows;
        let mut b = Vec::with_capacity(n * 4);
        let mut nonzero = false;
        // The gusts going on, laid over what the map holds: the larger energy, and of each flow component
        // the larger in size, its sign kept (a wake's is the other way).
        let (cols, c, now, flow) = (self.cols, self.cell, self.clock, if self.reduced { 0.0 } else { 1.0 });
        let over = |k: usize, e: f32, u: f32, v: f32| -> (f32, f32, f32) {
            let p = [(k % cols) as f32 * c, (k / cols) as f32 * c];
            let (mut e, mut u, mut v) = (e, u, v);
            for (g, t0, b, fl) in &self.gusts {
                let (ge, gf) = g.at(p, now - t0, *b);
                let (gu, gv) = (gf[0] * flow * fl, gf[1] * flow * fl);
                e = e.max(ge);
                if gu.abs() > u.abs() { u = gu; }
                if gv.abs() > v.abs() { v = gv; }
            }
            (e, u, v)
        };
        for k in 0..n {
            let (e, u, v) = if self.gusts.is_empty() { (self.e[k], self.u[k], self.v[k]) } else { over(k, self.e[k], self.u[k], self.v[k]) };
            let px = [q8(e), qf(u), qf(v), 0];
            nonzero |= px[0] != 0 || px[1] != 128 || px[2] != 128;
            b.extend_from_slice(&px);
        }
        // Values below a byte are gone: what no byte shows is let go, so the map ends exactly at zero.
        if !nonzero && self.gusts.is_empty() { self.e.iter_mut().for_each(|x| *x = 0.0); self.u.iter_mut().for_each(|x| *x = 0.0); self.v.iter_mut().for_each(|x| *x = 0.0); }
        if *self.snap.bytes != b || self.snap.cols != self.cols || self.snap.rows != self.rows || self.snap.cell != self.cell {
            self.snap = Snapshot { cols: self.cols, rows: self.rows, cell: self.cell, bytes: Rc::new(b), nonzero };
        }
    }
}

/// A box with lives of its own, worked out for a step (`FieldMap::step`): its energy's and flow's lives
/// (s) and how many times slower its flow runs than the look's map.
struct Own { b: [f32; 4], e: f32, f: f32, slow: f32 }

/// The look's curve from 1 for a life of `tau` (exponential, less the floor), a share `x` of the way to
/// rest: 1 at 0, and at 1 it is `rest`, under which no byte shows it (half a byte step), so the bytes are
/// back at rest there; 0 from 1 on.
fn curve(x: f32, tau: f32, rest: f32) -> f32 {
    if x >= 1.0 { return 0.0; }
    let a = FLOOR * tau;
    // (1 + a) e^(−t/τ) − a, with t/τ running from 0 to where it comes down to `rest`.
    let end = ((1.0 + a) / (a + rest)).ln();
    ((1.0 + a) * (-x * end).exp() - a).clamp(0.0, 1.0)
}

/// How long a disturbance of `e0` takes to reach zero at most, for an energy life of `tau` seconds.
pub fn settle_time(e0: f32, tau: f32) -> f32 { tau * (1.0 + e0 / (FLOOR * tau)).ln() }
/// A point's place along a polyline at `t` of its arc length (0..1), and the tangent there.
pub fn along(points: &[[f32; 2]], t: f32) -> ([f32; 2], [f32; 2]) {
    let lens: Vec<f32> = points.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).collect();
    let total: f32 = lens.iter().sum();
    let Some(first) = points.first() else { return ([0.0; 2], [0.0; 2]) };
    if total <= 0.0 { return (*first, [0.0; 2]); }
    let mut want = t.clamp(0.0, 1.0) * total;
    for (k, l) in lens.iter().enumerate() {
        if want <= *l || k == lens.len() - 1 {
            let f = if *l > 0.0 { (want / l).min(1.0) } else { 0.0 };
            let (a, b) = (points[k], points[k + 1]);
            let d = [(b[0] - a[0]) / l.max(1e-6), (b[1] - a[1]) / l.max(1e-6)];
            return ([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], d);
        }
        want -= l;
    }
    (*first, [0.0; 2])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quiet() -> MapFx { MapFx { meander: 0.0, creep: 0.0, ..MapFx::default() } }
    fn one(m: &mut FieldMap) { m.stir(Stir { at: [200.0, 160.0], dir: [1.0, 0.0], energy: 1.0, flow: 1.0, radius: 48.0, scope: EVERYWHERE }); }

    #[test]
    fn energy_falls_and_reaches_zero_within_the_bound() {
        let fx = quiet();
        let mut m = FieldMap::new(640.0, 400.0, 16.0);
        one(&mut m);
        let bound = settle_time(1.0, fx.energy_life.max(fx.flow_life));
        assert!((settle_time(1.0, 0.45) - 1.24).abs() < 0.01);
        let mut last = m.e.iter().copied().fold(0.0, f32::max);
        let mut t = 0.0;
        while m.snapshot().nonzero {
            m.step(&fx);
            m.encode();
            t += STEP;
            let now = m.e.iter().copied().fold(0.0, f32::max);
            assert!(now <= last + 1e-6, "the most energy rose: {last} → {now}");
            last = now;
            assert!(t <= bound + STEP, "still not zero after {t} s (bound {bound})");
        }
        assert!(!m.moving(&fx));
        assert_eq!(m.nonzero(), 0);
    }

    #[test]
    fn ten_steps_are_one_advance_of_ten_steps() {
        let fx = MapFx::default();
        let (mut a, mut b) = (FieldMap::new(640.0, 400.0, 16.0), FieldMap::new(640.0, 400.0, 16.0));
        one(&mut a); one(&mut b);
        for _ in 0..10 { a.advance(STEP, &fx); }
        b.advance(STEP * 10.0, &fx);
        assert_eq!(a.e, b.e);
        assert_eq!(a.u, b.u);
        assert_eq!(a.snapshot().bytes, b.snapshot().bytes);
        // The same inputs give the same bytes.
        let mut c = FieldMap::new(640.0, 400.0, 16.0);
        one(&mut c);
        for _ in 0..10 { c.advance(STEP, &fx); }
        assert_eq!(*c.snapshot().bytes, *a.snapshot().bytes);
    }

    #[test]
    fn a_stir_writes_only_inside_its_scope_and_never_in_calm() {
        let fx = MapFx::default();
        let mut m = FieldMap::new(640.0, 400.0, 16.0);
        m.calm = vec![[240.0, 0.0, 400.0, 400.0]];
        m.stir(Stir { at: [200.0, 160.0], dir: [1.0, 0.0], energy: 1.0, flow: 1.0, radius: 80.0, scope: [100.0, 100.0, 300.0, 220.0] });
        for j in 0..m.rows { for i in 0..m.cols {
            let (x, y) = (i as f32 * 16.0, j as f32 * 16.0);
            let (e, f) = m.node(i, j);
            let inside = x >= 100.0 && y >= 100.0 && x < 300.0 && y < 220.0 && x < 240.0;
            if !inside { assert_eq!((e, f), (0.0, [0.0, 0.0]), "written at {x}, {y}"); }
            assert!((f[0] * f[0] + f[1] * f[1]).sqrt() <= 1.0 + 1e-5);
        } }
        for _ in 0..30 {
            m.advance(STEP, &fx);
            for j in 0..m.rows { for i in 15..m.cols { assert_eq!(m.node(i, j).0, 0.0, "calm node {i}, {j}"); } }
            for k in 0..m.e.len() { assert!((m.u[k].powi(2) + m.v[k].powi(2)).sqrt() <= 1.0 + 1e-5); }
        }
    }

    #[test]
    fn meander_keeps_lengths_and_creep_gives_bare_nodes_no_more() {
        let mut a = FieldMap::new(320.0, 320.0, 16.0);
        one(&mut a);
        let lens = |m: &FieldMap| m.u.iter().zip(&m.v).map(|(u, v)| (u * u + v * v).sqrt()).collect::<Vec<f32>>();
        let fx = MapFx { bleed: 0.0, creep: 0.0, meander: 1.0, ..MapFx::default() };
        let before = lens(&a);
        let kf = (-STEP / fx.flow_life).exp();
        a.step(&fx);
        for (l0, l1) in before.iter().zip(lens(&a)) { assert!(l1 <= l0 * kf + 1e-5, "{l0} → {l1}"); }
        // Ink along a row: an inked node away from the stir gets more than spreading alone gives it.
        let mut ink = vec![0.0; a.e.len()];
        for i in 0..a.cols { ink[10 * a.cols + i] = 1.0; }
        let (mut plain, mut crept) = (FieldMap::new(320.0, 320.0, 16.0), FieldMap::new(320.0, 320.0, 16.0));
        crept.set_ink(ink.clone());
        for m in [&mut plain, &mut crept] { m.stir(Stir { at: [160.0, 160.0], dir: [0.0; 2], energy: 1.0, flow: 0.0, radius: 20.0, scope: EVERYWHERE }); }
        let fp = MapFx { creep: 0.0, meander: 0.0, ..MapFx::default() };
        let fc = MapFx { creep: 0.8, meander: 0.0, ..MapFx::default() };
        for _ in 0..12 { plain.step(&fp); crept.step(&fc); }
        // Bare nodes with no ink beside them get no more than spreading gives them.
        for k in 0..ink.len() { if (k / a.cols).abs_diff(10) >= 2 { assert!(crept.e[k] <= plain.e[k] + 1e-6, "bare node {k}: {} > {}", crept.e[k], plain.e[k]); } }
        let far = 10 * plain.cols + 13;
        assert!(crept.e[far] > plain.e[far], "energy did not creep along the ink: {} vs {}", crept.e[far], plain.e[far]);
    }

    #[test]
    fn reduced_motion_puts_no_flow_and_zero_is_exactly_zero() {
        let mut m = FieldMap::new(640.0, 400.0, 16.0);
        m.reduced = true;
        one(&mut m);
        assert!(m.u.iter().chain(&m.v).all(|x| *x == 0.0));
        assert!(m.e.iter().any(|x| *x > 0.5));
        let fresh = FieldMap::new(640.0, 400.0, 16.0);
        assert_eq!(fresh.energy_at(100.0, 100.0), 0.0);
        assert_eq!(fresh.flow_at(100.0, 100.0), [0.0, 0.0]);
        assert!(fresh.snapshot().bytes.chunks_exact(4).all(|p| p == [0, 128, 128, 0]));
    }
}
