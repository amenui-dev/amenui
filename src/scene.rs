//! Scene: what the renderer draws. Everything on screen is an instance of one of three kinds, and every
//! instance carries the same small record, so they all go through the same two stages.

pub const GLYPH: f32 = 0.0;
pub const SHAPE: f32 = 1.0;
pub const PICTURE: f32 = 2.0;
/// A `scene3d` element's meshes, resolved by the renderer into ink and nearness (DESIGN.md §1.18).
/// `extra[1]` is its index in `Scene::meshes`.
pub const MESH: f32 = 3.0;
/// Batches of mesh instances are keyed by this plus their index (pictures by their id).
pub const MESH_BATCH: u32 = 0x4000_0000;
/// Batches that read a cached plate's ink (`el::cached_view`) are `CACHED_BATCH + slot`.
pub const CACHED_BATCH: u32 = 0x2000_0000;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// Device px. Glyphs include the spread padding; shapes and pictures are padded by the renderer.
    pub rect: [f32; 4],
    /// Glyphs and pictures: texture coordinates x0, y0, x1, y1 (below −1: a cached plate's, see
    /// `cached_slot`). Shapes: corner radius (device px) in x.
    pub uv: [f32; 4],
    /// tone, accent, alpha, kind
    pub paint: [f32; 4],
    /// Visible region, device px: x0, y0, x1, y1.
    pub clip: [f32; 4],
    /// Shapes: border width (device px, 0 = filled). Pictures: picture id in y. Glyphs: print pitch
    /// (logical px, 0 = off) in y. All: static looseness, time the instance first appeared.
    /// Pictures carry their `Markr` slot in `paint[1]`.
    pub extra: [f32; 4],
    /// Material override code (0 = the global material; else `128 + level × 16 + id`, see
    /// `style::override_code`), accent hue (0, 1, 2), whether it reads the field map (1; 0 for a
    /// stirring element's own subtree, what a morph shows, flying tiles, motes; DESIGN.md §1.29), and
    /// whether the map's flow leaves it in place (1 for pictures and 3D views, else 0) plus twice its clip's
    /// corner radius in quarter device px (`El::clip_round`; 0 for a square clip).
    pub look: [f32; 4],
    /// How the field may move it (DESIGN.md §1.15): the most looseness fronts may add (0 for `Ignore`
    /// and `Nudge`, `Look::legible_cap` for legible `Loosen`, else 1); how fronts push it (0: not at all,
    /// 1: by its own centre, 2: as one piece with the point in z, w — the centre of the control or text
    /// it belongs to, device px — so a word is never pulled apart).
    pub resp: [f32; 4],
    /// Its own linear transform about the centre of `rect` (rotation, scale, shear), stored as the
    /// difference from the identity so that zero is untransformed: screen offset from the centre =
    /// `[[1 + xf[0], xf[1]], [xf[2], 1 + xf[3]]]` × the offset in the box. Glyphs on a path or a sphere,
    /// strokes of a drawing (`el::canvas`).
    ///
    /// Blades (draw.rs `Blade`) are worked out by the vertex stage from the map at their root: a segment
    /// is a shape with `extra[1]` ≥ 1 (1 + its lean paint), its `rect` the box the blade can reach and
    /// its numbers in `uv`; a head is a glyph with `extra[1]` ≤ −1 (−1 − its own lean), the lean paint in
    /// `extra[0]` and the blade's root, length and stiffness in `xf` (shader.wgsl `blade_seg`, `blade_head`).
    pub xf: [f32; 4],
}
impl Instance {
    /// The transform for a turn by `angle` (radians, clockwise on screen) after scaling by `sx`, `sy`.
    pub fn turned(angle: f32, sx: f32, sy: f32) -> [f32; 4] {
        let (s, c) = angle.sin_cos();
        [c * sx - 1.0, -s * sy, s * sx, c * sy - 1.0]
    }
    /// A blade's segment or head (draw.rs `Blade`): its shape is worked out on the GPU from the field map.
    pub fn blade(&self) -> bool { (self.paint[3] == SHAPE && self.extra[1] >= 0.5) || self.blade_head() }
    pub(crate) fn blade_head(&self) -> bool { self.paint[3] == GLYPH && self.extra[1] < -0.5 }
    /// A view of a cached plate (`el::cached_view`): a glyph whose texture coordinates are packed as
    /// `−1 − 4 × slot − t` (t 0–1 in the window-sized plate), so they are below −1; the slot.
    pub fn cached_slot(&self) -> Option<u32> { (self.paint[3] == GLYPH && self.uv[0] < -0.5).then(|| ((-1.0 - self.uv[0]) / 4.0).floor().max(0.0) as u32) }
    /// The axis-aligned box (x0, y0, x1, y1) its transformed `rect` covers, grown by `pad` (device px).
    pub fn bounds(&self, pad: f32) -> [f32; 4] {
        let r = self.rect;
        // A blade's head: wherever on the blade's reach its tip takes it (`xf` holds the root and length).
        if self.blade_head() {
            let (x, d) = (self.xf, r[2].max(r[3]) + pad);
            return [x[0] - x[2] - d, x[1] - x[2] - d, x[0] + x[2] + d, x[1] + d];
        }
        let (hw, hh) = (r[2] * 0.5 + pad, r[3] * 0.5 + pad);
        let (cx, cy) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        let x = self.xf;
        let (ex, ey) = if x == [0.0; 4] { (hw, hh) } else { (((1.0 + x[0]) * hw).abs() + (x[1] * hh).abs(), (x[2] * hw).abs() + ((1.0 + x[3]) * hh).abs()) };
        [cx - ex, cy - ey, cx + ex, cy + ey]
    }
}

/// One `scene3d` element to draw: what it holds and its content box (device px: x, y, w, h).
#[derive(Clone, Debug)]
pub struct MeshDraw { pub scene: std::rc::Rc<crate::mesh::Scene3d>, pub rect: [f32; 4] }

/// A run of instances drawn with one picture bound. Glyphs and shapes do not care which.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Batch { pub start: u32, pub end: u32, pub picture: u32 }

/// Instances drawn as one group and then made see-through as one (`El::opacity`): `start..end`, each
/// at its own alpha within the group, the group's pixels then multiplied by `opacity`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Group { pub start: u32, pub end: u32, pub opacity: f32 }

#[derive(Clone, Default)]
pub struct Scene {
    pub instances: Vec<Instance>,
    pub batches: Vec<Batch>,
    /// Where the modal layer starts, if one is open. Everything from here on is drawn over the page
    /// and hides it in the near image too (see the renderer).
    pub layer: Option<u32>,
    /// Stage 3, the annotation layer (DESIGN.md §1.7): drawn after the material, exactly, never loose.
    pub annot: Vec<Instance>,
    /// The `scene3d` elements whose mesh instances are in this scene.
    pub meshes: Vec<MeshDraw>,
    /// The see-through groups, in order, never overlapping.
    pub groups: Vec<Group>,
    /// Text to read and controls drawn over others' ink: the instance, and how far round its true
    /// outline it holds the looseness beneath to the legibility cap, `r0` fully and fading out by `r1`
    /// (device px; DESIGN.md §1.15).
    pub holds: Vec<(u32, [f32; 2])>,
    /// The page's glyphs of text to read, by instance index: where the finishing pass keeps text clear
    /// (`PostSpec::clear_text`).
    pub legible: Vec<u32>,
    /// The islands of filled controls whose own fill is loosened (DESIGN.md §1.15): each a box held at
    /// rest, `rect` and its radius in `uv[0]`, within the control's outline at rest (`clip`, its radius in
    /// `look[0]`); drawn into the hold image as they are (`paint[0]` −3).
    pub islands: Vec<Instance>,
    /// The plates drawn this frame (`El::plate`, DESIGN.md §1.33), in order, never overlapping.
    pub plates: Vec<PlateSpan>,
    /// Plate slots emptied before this frame is drawn (`Cmd::ClearPlate`, a plate gone, a new size).
    pub plate_clear: Vec<u8>,
    /// The serial of `plate_clear`: a renderer empties the slots once for each serial it sees.
    pub plate_serial: u64,
    /// The paint that made this scene, unique across every `Ui`: a renderer that draws one scene twice
    /// (no paint between) puts its ink into the plates once.
    pub paint: u64,
    /// The plate clock, s: it stops while a `Hold` lasts and never runs back.
    pub plate_now: f32,
    /// Some plate's ink is ageing (`Plate::fade_s`): every plate is drawn again whole.
    pub plate_ageing: bool,
    /// The drawings' own colours this frame (`Drawing::palette`, sRGB 0–1): an instance's hue 3 + k is
    /// colour k (at most `draw::OWN_COLOURS`).
    pub own: Vec<[f32; 3]>,
    /// The stages on the page (`El::stage`, DESIGN.md §1.35): their instances and where they show.
    pub stages: Vec<StageBox>,
    /// The built-in materials the page's elements override the global one with, drawn or hidden (one
    /// waiting for a morph included), a bit by id (`style::override_code`): the passes a renderer makes
    /// ahead for them.
    pub mats: u16,
}
/// A stage's instances this frame (`start..end`) and its box as shown (within its clip, device px).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StageBox { pub start: u32, pub end: u32, pub rect: [f32; 4] }

/// How many plates a window keeps at most.
pub const PLATES: usize = 4;
/// A plate's instances this frame (`start..end`), its slot, and how long its ink lasts (s; 0: for good).
/// `replace`: they are all the plate holds (a cached subtree whose ink lasts): drawn, they replace what
/// it held where they are drawn, so a partial frame inks the plate only within its boxes; otherwise
/// they add to what it holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateSpan { pub slot: u8, pub start: u32, pub end: u32, pub fade_s: f32, pub replace: bool }

impl Scene {
    /// Where the instance list is cut into passes, in order: the modal layer's start (`(at, 0)`) and each
    /// plate's start and end (`(at, 1 + 2 slot)`, `(at, 2 + 2 slot)`). What is drawn differs where an
    /// instance crosses one, so the dirty region matches instances only between the same cuts.
    pub fn cuts(&self) -> Vec<(u32, u32)> { cuts(self.layer, self.plates.iter().map(|p| (p.slot, p.start, p.end))) }
    pub fn clear(&mut self) { self.instances.clear(); self.batches.clear(); self.layer = None; self.annot.clear(); self.meshes.clear(); self.groups.clear(); self.holds.clear(); self.legible.clear(); self.islands.clear(); self.plates.clear(); self.own.clear(); self.stages.clear(); }
    /// Everything pushed from now on is in the modal layer.
    pub fn begin_layer(&mut self) { self.layer = Some(self.instances.len() as u32); }
    pub fn push(&mut self, i: Instance) {
        let n = self.instances.len() as u32;
        let picture = if i.paint[3] == PICTURE { Some(i.extra[1] as u32) } else if i.paint[3] == MESH { Some(MESH_BATCH + i.extra[1] as u32) } else { i.cached_slot().map(|k| CACHED_BATCH + k) };
        // A batch never straddles the start of the layer, so the renderer can draw the two apart.
        match self.batches.last_mut() {
            // Glyphs and shapes join any batch whose group holds the glyph atlas: not a 3D element's.
            Some(b) if picture.map_or(b.picture < CACHED_BATCH, |p| p == b.picture) && self.layer != Some(n) => b.end = n + 1,
            _ => self.batches.push(Batch { start: n, end: n + 1, picture: picture.unwrap_or(0) }),
        }
        self.instances.push(i);
    }
    /// Drop everything from instance `n` on.
    pub fn truncate(&mut self, n: usize) {
        self.instances.truncate(n);
        if self.layer.is_some_and(|l| l as usize >= n) { self.layer = None; }
        self.batches.retain(|b| (b.start as usize) < n);
        if let Some(b) = self.batches.last_mut() { b.end = b.end.min(n as u32); }
        self.groups.retain(|g| (g.start as usize) < n);
        self.holds.retain(|h| (h.0 as usize) < n);
        self.legible.retain(|&k| (k as usize) < n);
        // An empty plate still lays on what it keeps.
        self.plates.retain(|p| (p.start as usize) <= n);
        for p in &mut self.plates { p.end = p.end.min(n as u32); }
        if let Some(g) = self.groups.last_mut() { g.end = g.end.min(n as u32); }
    }
}

/// `Scene::cuts` of a layer's start and plates' `(slot, start, end)`.
pub fn cuts(layer: Option<u32>, plates: impl Iterator<Item = (u8, u32, u32)>) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = layer.map(|l| (l, 0)).into_iter().collect();
    for (slot, start, end) in plates { v.push((start, 1 + 2 * slot as u32)); v.push((end, 2 + 2 * slot as u32)); }
    // A handful at most: put in order by hand (a sort here costs the web module more than it saves).
    for k in 1..v.len() { let mut j = k; while j > 0 && v[j - 1] > v[j] { v.swap(j - 1, j); j -= 1; } }
    v
}

/// What disturbs the page right now: fronts started by presses, and the pointer. Device px.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    /// x, y, start time, strength
    pub fronts: [[f32; 4]; 8],
    /// x, y, strength, _
    pub pointer: [f32; 4],
    /// A half-plane source (the compare wipe): x of the line, strength, _, _. Right of the line the
    /// disturbance is `strength`, left of it nothing. It acts only inside `wipe_rect` (x0, y0, x1, y1).
    pub wipe: [f32; 4],
    pub wipe_rect: [f32; 4],
    /// A short burst over a rect (x0, y0, x1, y1), and when it started, how strong, how long (s).
    /// Hover and press put one over the control; glitchr reads it.
    pub burst: [f32; 4],
    pub burst_t: [f32; 4],
    /// Up to four boxes (x0, y0, x1, y1, device px) that fronts pass by (`El::calm`).
    pub calm: [[f32; 4]; 4],
    /// Each front's scope (DESIGN.md §1.15): the box (x0, y0, x1, y1, device px) whose instances it
    /// reaches, by their centre; and the element that sent it (0: none), so a new press from the same
    /// element restarts its front instead of adding one.
    pub scopes: [[f32; 4]; 8],
    pub sources: [u64; 8],
    pub next: usize,
    /// Up to eight scroll nodes (DESIGN.md §1.8.3): each viewport (x0, y0, x1, y1, device px) and its
    /// content offset (x, y, whole device px), the strength of its trace (0..1) and the way its content
    /// moves (±1 down / up, ±2 right / left).
    pub scrolls: [[f32; 4]; 8],
    pub scrollo: [[f32; 4]; 8],
}
/// The scope box of a global front: everything.
pub const EVERYWHERE: [f32; 4] = [-1e6, -1e6, 1e6, 1e6];
impl Default for Field {
    fn default() -> Self { Self { fronts: [[0.0; 4]; 8], pointer: [-1e5, -1e5, 0.0, 0.0], wipe: [0.0; 4], wipe_rect: [0.0; 4], burst: [0.0; 4], burst_t: [0.0, 0.0, 0.0, 0.0], calm: [[0.0; 4]; 4], scopes: [EVERYWHERE; 8], sources: [0; 8], next: 0, scrolls: [[0.0; 4]; 8], scrollo: [[0.0; 4]; 8] } }
}
impl Field {
    /// A front over the whole window, from no element in particular.
    pub fn front(&mut self, x: f32, y: f32, t: f32, strength: f32) { self.front_in([x, y], t, strength, EVERYWHERE, 0, f32::INFINITY); }
    /// A front from `at` (device px) that reaches only instances whose centre is inside `scope` (device px box), sent by
    /// `source` (0: none). While a front from the same source is still alive (younger than `life`
    /// seconds) it is restarted from here, not added to: fronts from one element never stack.
    pub fn front_in(&mut self, at: [f32; 2], t: f32, strength: f32, scope: [f32; 4], source: u64, life: f32) {
        let [x, y] = at;
        let alive = (0..self.fronts.len()).find(|&j| source != 0 && self.sources[j] == source && self.fronts[j][3] > 0.0 && t - self.fronts[j][2] < life);
        let j = match alive { Some(j) => j, None => { let j = self.next; self.next = (self.next + 1) % self.fronts.len(); j } };
        self.fronts[j] = [x, y, t, strength];
        self.scopes[j] = scope;
        self.sources[j] = source;
    }
    /// How strongly the fronts disturb device point `p` at time `t`, for an instance centred at `c`
    /// (only fronts whose scope holds `c` count): the shader's `wave`, on the CPU, for tests and the tree
    /// export. `tail_gain` as in the shader (0.6 for text and shapes).
    pub fn wave_at(&self, p: [f32; 2], c: [f32; 2], t: f32, fx: &Fx, scale: f32, tail_gain: f32) -> f32 {
        if self.calm.iter().any(|r| r[2] > r[0] && p[0] >= r[0] && p[1] >= r[1] && p[0] < r[2] && p[1] < r[3]) { return 0.0; }
        let (w, tail) = (fx.wave_width.max(1e-3) * scale, fx.wave_tail.max(1e-3) * scale);
        let mut k: f32 = 0.0;
        for (j, f) in self.fronts.iter().enumerate() {
            let age = t - f[2];
            if f[3] <= 0.0 || age < 0.0 { continue; }
            let sc = self.scopes[j];
            if c[0] < sc[0] || c[1] < sc[1] || c[0] >= sc[2] || c[1] >= sc[3] { continue; }
            let d = ((p[0] - f[0]).powi(2) + (p[1] - f[1]).powi(2)).sqrt() - age * fx.wave_speed * scale;
            let life = fx.wave_life.max(1e-3);
            let x = (age / life).clamp(0.0, 1.0);
            let strength = (1.0 - x * x * (3.0 - 2.0 * x)) * f[3];
            let front = (-(d * d) / (w * w)).exp();
            let behind = if d < 0.0 { (d / tail).exp() * tail_gain } else { 0.0 };
            k = k.max(front.max(behind) * strength);
        }
        k
    }
    /// A burst over `rect` (device px, x0 y0 x1 y1, or the whole window when `None`) from time `t`,
    /// `strength` 0..1, lasting `secs`.
    pub fn burst(&mut self, rect: Option<[f32; 4]>, t: f32, strength: f32, secs: f32) {
        self.burst = rect.unwrap_or([-1e6, -1e6, 1e6, 1e6]);
        self.burst_t = [t, strength, secs.max(1e-3), 0.0];
    }
    /// How strong the burst is at time `t` (it fades out linearly).
    pub fn burst_at(&self, t: f32) -> f32 {
        let age = t - self.burst_t[0];
        if age < 0.0 || age > self.burst_t[2] { 0.0 } else { self.burst_t[1] * (1.0 - age / self.burst_t[2]) }
    }
}

/// Field and material parameters. Every one is a plain number so it can be set exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fx {
    /// Lattice pitch for text and shapes (logical px). Pictures have their own, in `Markr`.
    pub pitch: f32,
    /// How far each unit may sit off the lattice, 0..1 of the pitch.
    pub jitter: f32,
    /// Sideways slip of thin bands while a front passes, 0 = none.
    pub glitch: f32,
    /// Front speed (logical px per second), how long a front lasts (seconds), its width and the length
    /// of the loosened trail behind it (logical px).
    pub wave_speed: f32,
    pub wave_life: f32,
    pub wave_width: f32,
    pub wave_tail: f32,
    /// How far instances are pushed as a front passes under them (logical px).
    pub push: f32,
    /// Pointer lens over pictures: radius (logical px), and how hard its edge is (0: a soft bell, as
    /// loose as it is near the centre; 1: fully loose inside the radius, not at all outside).
    pub lens: f32,
    pub lens_hard: f32,
    /// How long a new instance takes to condense (seconds).
    pub enter: f32,
    pub square: bool,
    /// How much of the accent is in the hover and focus marks: 1 the accent, 0 the ink.
    pub marks_accent: f32,
}
impl Default for Fx {
    fn default() -> Self {
        Self { pitch: 5.0, jitter: 0.0, glitch: 0.2, wave_speed: 760.0, wave_life: 1.4, wave_width: 26.0, wave_tail: 150.0, push: 5.0, lens: 84.0, lens_hard: 0.35, enter: 0.45, square: false, marks_accent: 0.0 }
    }
}
