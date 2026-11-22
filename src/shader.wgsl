// Two stages.
//   Stage 1 draws every instance exactly, into two intermediate images (same geometry, drawn twice):
//     ink  (rgba8): what is there. Glyphs and shapes: r = tone × (1 − accent), g = accent, a = coverage
//                   (premultiplied); the colour is grey(tone) × (1 − g) + hue × g (`paint_rgb`: the
//                   tone through the theme's greys, straight between them), so an anti-aliased edge over
//                   another paint decodes to a colour between the two.
//                   Pictures: rgb = their colour, a = coverage.
//                   b = a code (straight, b / a): hue × 0.15 + is-shape × 0.05 (hue 3 the extreme; so a
//                   glyph's anti-aliased edge over a shape of the same hue keeps its hue).
//     near (rgba8, max-blended): r = signed nearness to the outline, by true outline: 0.5 on it, 1 at
//                   SPREAD px inside, 0 at SPREAD px outside; g = where a picture is, (PicFx slot + 1) / 8;
//                   b = the instance's own looseness (held, born, or set at rest); a = a material
//                   override (≥ 128/255: 128 + level × 16 + id) or else the print pitch of glyphs × 2 / 255.
//   Stage 2 (the material) reads both plus the field and decides the final pixel. Pictures turn into
//   marks by their own PicFx slot (DESIGN.md §1.6.1); text and shapes follow the material.

struct U {
  res: vec4f,      // width, height, scale, time (time terms of materials read the material clock, fa.y)
  fa: vec4f,       // pitch, material clock (s), jitter, glitch   (lengths in logical px)
  fb: vec4f,       // wave speed, wave life, wave width, wave tail
  fc: vec4f,       // push, lens radius, enter time, square (0 dot, 1 square)
  fd: vec4f,       // material (0 plain, 1 dottr), srgb surface, spread px, palette rows
  bg: vec4f,
  fg: vec4f,
  accent: vec4f,
  pointer: vec4f,  // x, y, strength (device px), how hard the lens's edge is
  wipe: vec4f,     // half-plane source: line x, strength; z: the ground's own marks off (Off::ground), _
  wipe_rect: vec4f,// where it acts: x0, y0, x1, y1
  split: vec4f,    // a commit: origin x, y (device px), start time, on
  fronts: array<vec4f, 8>, // x, y, start time, strength
  pics: array<vec4f, 56>,  // 8 PicFx slots × 7 (see style.rs PicFx::pack)
  ground2: vec4f,  // the ground at the bottom of the window; w: looseness scale
  second: vec4f,   // accent hues 1 and 2
  third: vec4f,
  burst: vec4f,    // a burst's rect, device px (x0, y0, x1, y1)
  burst_t: vec4f,  // its start time, strength, length (s); w: the legibility cap (Look::legible_cap)
  mats: array<vec4f, 32>, // every material's numbers, two vec4s each (style.rs MAT_PARAMS, then those added)
  calm: array<vec4f, 4>,  // boxes fronts pass by (x0, y0, x1, y1, device px)
  qual: vec4f,     // the material resolution: 1 / q for the reduced pass (1 otherwise), q, its size w, h
  scopes: array<vec4f, 8>, // each front's scope box (x0, y0, x1, y1, device px): it reaches instances centred inside
  rip: vec4f,      // looseness per pixel is clamped to x (Look::loose_max) after summing; y: everything loosened by a transition; z: its last step; w: the ground clock (s)
  tr: array<vec4f, 9>,     // a transition (preset.rs Transition::pack): duration (s), origin, start; two vec4s per step
  boxes: array<vec4f, 16>, // the elements a transition's boxes find (x0, y0, x1, y1, device px)
  scrolls: array<vec4f, 8>, // scroll nodes' viewports (x0, y0, x1, y1, device px), DESIGN.md §1.8.3
  scrollo: array<vec4f, 8>, // their content offsets (x, y, whole device px), trace strength, the way the content moves (±1 down/up, ±2 right/left)
  scrp: vec4f,     // the trace's lengths (device px): trail, misregistration, glitch slip, glitch band
  scrq: vec4f,     // the least time between two glitch slips (s); the beat clock off the grid (condensing); how much ink plain keeps at the legibility cap (Theme::press_keep); w: with less motion, how long new things fade in (s; 0 otherwise)
  post: array<vec4f, 8>, // the finishing pass (post.rs Post::pack): dither kind, levels, palette size, on; bloom strength, threshold, radius, dither strength; chroma offset, scanline strength, period, grain; vignette strength, radius, curvature, grain cell; invert, binary, the grain's region on, the bloom's; the grain's region; the bloom's; text kept clear, its scanlines
  pal: array<vec4f, 16>, // the palette, sRGB 0–1
  fmap: vec4f,     // the field map (DESIGN.md §1.29): cell (device px), drift (device px), nodes across, down (0: all zero, not read)
  sbox: vec4f,     // a commit's scope (El::commit_scope): x0, y0, x1, y1 device px; outside it the old frame stays (x1 <= x0: none)
  plate: vec4f,    // x: the plate clock in ticks (render.rs PLATE_TICK)
  plates: array<vec4f, 4>, // each plate slot: x its fade in ticks (0: kept for good)
  own: array<vec4f, 32>,   // the drawings' own colours (Drawing::palette), sRGB 0–1: hue 3 + k
  stages: array<vec4f, 16>, // the stages shaded at their own scale in the reduced pass (x0, y0, x1, y1 device px)
  sharp: array<vec4f, 16>,  // what is drawn over them, kept at the device's scale (the same)
  stagen: vec4f,   // how many stages, how many boxes over them; z 1: the reduced pass is for the stages alone
  mv: vec4f,       // a scroll move with its trace showing: the stronger trace of the two frames, the way it points (render.rs)
  greys: array<vec4f, 5>, // the theme's greys, ground to ink (raised, hairline, dim, text: rgb, w the tone each is at), then the extreme
  lop: vec4f,      // the reduced grid: its anchor's offset (device px, modulo GRID_WRAP) and the texel it starts one before (render.rs)
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var tex0: texture_2d<f32>;
@group(0) @binding(3) var tex1: texture_2d<f32>;
@group(0) @binding(4) var marks: texture_2d<f32>;   // rg: mark coverage, height; rows = sets, columns = levels
@group(0) @binding(5) var palettes: texture_2d<f32>; // 16 × rows
@group(0) @binding(6) var chars: texture_2d<f32>;    // character tiles cut per device pitch (marks.rs char_tiles)
@group(0) @binding(7) var before: texture_2d<f32>;   // the frame before the last commit
@group(0) @binding(8) var fmap_t: texture_2d<f32>;   // the field map: r energy, g b 128 + 127 × flow (fieldmap.rs)
@group(0) @binding(9) var lo: texture_2d<f32>;       // the material at reduced resolution (DESIGN.md §1.12)
@group(0) @binding(10) var hold: texture_2d<f32>;    // r: how firmly text to read holds the looseness here to the cap (DESIGN.md §1.15)

const PLAIN: i32 = 0; const DOTTR: i32 = 1; const HALFTONR: i32 = 2; const CHARR: i32 = 3; const SMUDGR: i32 = 4;
const CARVR: i32 = 5; const GLITCHR: i32 = 6; const NEAR: i32 = 7; const SCRIBBLR: i32 = 8;
fn mp(m: i32, i: i32) -> vec4f { return u.mats[m * 2 + i]; }
/// The material every pixel is drawn with, in a material pass made for one (`Renderer::specialise`: the
/// global material, nothing overriding it); −1 in the pass for any.
override MAT: i32 = -1;
/// A material pass made for `MAT` on a page of several materials (`Renderer`'s passes per material): every
/// pixel finds its material as the pass for any does, and the pass draws only those of `MAT` (`shade_new`);
/// the others are drawn by the passes for theirs.
override ONLY: bool = false;
/// The filter up done by the sampler (`Renderer::checks`): the check that the filter weighing its four
/// texels itself differs from the sampler's by a step of 8 bits at most.
override SAMPLED_UP: bool = false;
/// No least ink in loose cells (`INK_FLOOR`, `Renderer::checks`): the check that only cells sampled below
/// it change.
override NO_INK_FLOOR: bool = false;
/// The material an instance (or a pixel) is drawn with: its override, or the global one.
fn eff_mat(code: f32) -> i32 { if (MAT >= 0 && !ONLY) { return MAT; } if (code >= 127.5) { return i32(round(code)) % 16; } return i32(u.fd.x); }
/// The extreme's hue code (`Paint::EXTREME_CODE`: after the theme's three and the drawings' 32).
const XHUE = 35.0;
/// A hue's colour: the theme's three accents, then the drawings' own colours (`Drawing::palette`), then
/// the scheme's extreme.
fn hue_col(h: f32) -> vec3f { if (h > XHUE - 0.5) { return u.greys[4].rgb; } if (h > 2.5) { return u.own[clamp(i32(round(h)) - 3, 0, 31)].rgb; } if (h > 1.5) { return u.third.rgb; } if (h > 0.5) { return u.second.rgb; } return u.accent.rgb; }
/// A hue as the coverage buffer codes it, off a picture: one of the theme's three (a drawing's own colour
/// is hue 0 there).
fn theme_hue(h: f32) -> f32 { return select(select(round(h), 0.0, h > 2.5), 3.0, h > XHUE - 0.5); }
/// A hue in the ink image's code (0–3) as `hue_col` takes it.
fn code_hue(h: f32) -> f32 { return select(h, XHUE, h > 2.5); }
/// A tone's colour (`Theme::grey`): the ground here (0) through the theme's greys, each exactly at its
/// tone, to the ink (1), straight between them.
fn grey(t: f32, at: vec2f) -> vec3f {
  let tc = clamp(t, 0.0, 1.0);
  var a = gnd(at); var ta = 0.0;
  for (var k = 0; k < 4; k++) {
    let b = u.greys[k];
    if (tc <= b.w) { return mix(a, b.rgb, clamp((tc - ta) / max(b.w - ta, 1e-6), 0.0, 1.0)); }
    a = b.rgb; ta = b.w;
  }
  return mix(a, u.fg.rgb, clamp((tc - ta) / max(1.0 - ta, 1e-6), 0.0, 1.0));
}
/// A paint's colour from its ink code: r = tone × (1 − accent), g = accent, the hue `hue_col` takes.
fn paint_rgb(r: f32, g: f32, hue: f32, at: vec2f) -> vec3f {
  let t = select(r / max(1.0 - g, 1e-6), 0.0, g > 0.999);
  return grey(t, at) * max(1.0 - g, 0.0) + hue_col(hue) * g;
}
fn loose_scale() -> f32 { return u.ground2.w; }
/// The ground's own marks are drawn (`Off::ground` not set).
fn ground_marks() -> bool { return u.wipe.z < 0.5; }

// (strength now, signed distance to the front in device px; negative = already passed)
fn front_at(p: vec2f, r: vec4f) -> vec2f {
  let age = u.res.w - r.z;
  if (r.w <= 0.0 || age < 0.0) { return vec2f(0.0, 1e6); }
  let d = distance(p, r.xy) - age * u.fb.x * u.res.z;
  return vec2f((1.0 - smoothstep(0.0, max(u.fb.y, 1e-3), age)) * r.w, d);
}
/// Whether front `j` reaches a point (an instance's centre, or a pixel of the ground).
fn in_scope(c: vec2f, j: i32) -> bool {
  let r = u.scopes[j];
  return c.x >= r.x && c.y >= r.y && c.x < r.z && c.y < r.w;
}

// ── The field map (DESIGN.md §1.29) ───────────────────────────────────────────────────────────────
// Node (i, j) is at (i, j) × cell; between nodes, bilinear by hand (the vertex stage reads it too, and the
// CPU's model reads the same bytes the same way). When the map is all zero it is not read at all.
fn fmap_raw(p: vec2f) -> vec4f {
  let q = max(p / u.fmap.x, vec2f(0.0));
  let i = vec2i(floor(q)); let f = q - floor(q);
  let lim = vec2i(i32(u.fmap.z) - 1, i32(u.fmap.w) - 1);
  let a = textureLoad(fmap_t, min(i, lim), 0);
  let b = textureLoad(fmap_t, min(i + vec2i(1, 0), lim), 0);
  let c = textureLoad(fmap_t, min(i + vec2i(0, 1), lim), 0);
  let d = textureLoad(fmap_t, min(i + vec2i(1, 1), lim), 0);
  return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
fn fmap_on() -> bool { return u.fmap.z > 0.5; }
fn fmap_e(p: vec2f) -> f32 { if (!fmap_on()) { return 0.0; } return fmap_raw(p).r; }
fn fmap_flow(p: vec2f) -> vec2f {
  if (!fmap_on()) { return vec2f(0.0); }
  let f = (fmap_raw(p).gb * 255.0 - 128.0) / 127.0;
  return select(f, vec2f(0.0), abs(f) < vec2f(0.5 / 127.0));
}

// ── Stage 1 ───────────────────────────────────────────────────────────────────────────────────────
struct VIn {
  @builtin(vertex_index) vi: u32,
  @location(0) rect: vec4f, @location(1) uv: vec4f, @location(2) paint: vec4f, @location(3) clip: vec4f, @location(4) extra: vec4f, @location(5) look: vec4f,
  @location(6) resp: vec4f, @location(7) xf: vec4f,
};
struct VOut {
  @builtin(position) pos: vec4f,
  @location(0) uv: vec2f,
  @location(1) local: vec2f,
  @location(2) @interpolate(flat) size: vec2f,
  @location(3) @interpolate(flat) paint: vec4f,
  @location(4) @interpolate(flat) clip: vec4f,
  @location(5) @interpolate(flat) extra: vec4f,
  // Corner radius, and whether the lens may touch it (1: a picture or 3D element that responds `Loosen`
  // and that the pointer does not operate, DESIGN.md §1.15 "the lens").
  @location(6) @interpolate(flat) radius: vec2f,
  @location(7) @interpolate(flat) look: vec4f,
  // How the field may move it (scene.rs Instance::resp): looseness cap, push gain; and its centre.
  @location(8) @interpolate(flat) resp: vec4f,
  // Where the instance's box starts on screen (device px, after any push): its local coordinates are
  // the pixel's position less this, exactly, wherever it is (DESIGN.md §1.8.3: a box moved by a pixel
  // draws the same pixels).
  @location(9) @interpolate(flat) org: vec2f,
  @location(10) @interpolate(flat) uvr: vec4f,
  // The instance's own transform about its box's centre, less the identity (scene.rs Instance::xf).
  @location(11) @interpolate(flat) xf: vec4f,
};
/// The fragment's local coordinates and texture coordinates from its exact position and the flat box,
/// not from interpolation (which rounds differently at every place on screen).
fn exact_i(iv: VOut) -> VOut {
  var i = iv;
  i.local = i.pos.xy - i.org;
  if (any(i.xf != vec4f(0.0))) {
    // A turned or scaled instance: the pixel's offset from the centre, taken back into the box.
    let a = 1.0 + i.xf.x; let b = i.xf.y; let c = i.xf.z; let d = 1.0 + i.xf.w;
    let q = i.local - i.size * 0.5;
    i.local = vec2f(d * q.x - b * q.y, a * q.y - c * q.x) / (a * d - b * c) + i.size * 0.5;
  }
  if (i.paint.w < 0.5 || i.paint.w > 1.5) { i.uv = i.uvr.xy + i.local / i.size * (i.uvr.zw - i.uvr.xy); }
  return i;
}

// ── Blades (draw.rs Blade, DESIGN.md §1.26) ───────────────────────────────────────────────────────
// A blade's segments and its head are instances whose shape is worked out here from the map's flow at the
// root, every frame, the same way as draw.rs `Blade::shape`.
const BLADE_M: i32 = 16;
const BLADE_MOST: f32 = 1.4835299; // 85°
fn blade_bend(s: f32, st: f32) -> f32 {
  if (st <= 0.0) { return clamp(s, 0.0, 1.0); }
  let t = clamp((s - 0.6 * st) / (1.0 - 0.6 * st), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}
fn blade_tangent(s: f32, th: f32, st: f32) -> f32 { return th * (0.35 + 0.65 * blade_bend(s, st)); }
/// Where the `k`th of `n` joints is, as a share of the length (`Blade::joint`).
fn blade_joint(k: f32, n: f32, st: f32) -> f32 {
  let want = k / n;
  var lo = 0.0; var hi = 1.0;
  for (var j = 0; j < 20; j++) {
    let m = (lo + hi) * 0.5;
    if ((m + 3.0 * blade_bend(m, st)) * 0.25 < want) { lo = m; } else { hi = m; }
  }
  return (lo + hi) * 0.5;
}
/// The point a share `s` along it (root at 0, up −y), `len` device px long (`Blade::at`).
fn blade_at(s: f32, th: f32, st: f32, len: f32) -> vec2f {
  let d = s * len / f32(BLADE_M);
  var p = vec2f(0.0);
  for (var j = 0; j < BLADE_M; j++) {
    let a = blade_tangent((f32(j) + 0.5) / f32(BLADE_M) * s, th, st);
    p += vec2f(sin(a), -cos(a)) * d;
  }
  return p;
}
fn blade_point(k: f32, n: f32, th: f32, st: f32, len: f32) -> vec2f { return blade_at(blade_joint(k, n, st), th, st, len); }
/// A blade's lean now (its tip's angle): its own and the map's flow at its root (max_lean × flow.x)
/// added, all of it within ±max_lean (and ±85°).
fn blade_lean(root: vec2f, own: f32, most: f32) -> f32 {
  let m = min(most, BLADE_MOST);
  return clamp(own + most * clamp(fmap_flow(root).x, -1.0, 1.0), -m, m);
}
/// A paint going to a blade's lean paint (`1 +` its tone + 256 × its accent, 255ths) as it leans `th`
/// (draw.rs `Blade::mix`): with `range` (lean_from, lean_full; resp.zw) rising, a smoothstep over it;
/// otherwise `|th|` of the most `most`.
fn blade_paint(p: vec4f, code: f32, th: f32, most: f32, range: vec2f) -> vec4f {
  var f = clamp(abs(th) / max(most, 1e-3), 0.0, 1.0);
  if (range.y > range.x) { f = smoothstep(range.x, range.y, abs(th)); }
  let lp = code - 1.0;
  let la = floor(lp / 256.0); let lt = lp - la * 256.0;
  return vec4f(mix(p.x, lt / 255.0, f), mix(p.y, la / 255.0, f), p.z, p.w);
}
/// A blade's segment as a stroke with round ends (a canvas line): uv.x stiffness, uv.y the most lean,
/// uv.z its own lean, uv.w which segment + 16 × how many + 256 × its width × 16; extra.y 1 + the lean
/// paint's tone + 256 × its accent (255ths); rect the box it can reach, the root at its bottom middle
/// less the width.
fn blade_seg(iv: VIn) -> VIn {
  var i = iv;
  let code = i.uv.w;
  let wq = floor(code / 256.0); let rem = code - wq * 256.0;
  let segs = floor(rem / 16.0); let g = rem - segs * 16.0;
  let w = wq / 16.0;
  let len = i.rect.z * 0.5 - w;
  let root = vec2f(i.rect.x + i.rect.z * 0.5, i.rect.y + i.rect.w - w);
  let th = blade_lean(root, i.uv.z, i.uv.y);
  let st = i.uv.x;
  var a = blade_point(g, segs, th, st, len);
  var b = blade_point(g + 1.0, segs, th, st, len);
  let dir = normalize(b - a + vec2f(0.0, -1e-6));
  let across = vec2f(-dir.y, dir.x);
  // Where it meets the segments before and after it, it is cut along the joint's bisector (`inst`), so
  // the round ends of two segments never both cover a pixel: uv.z, uv.w the bisector's angle in the
  // segment's own frame + 4 (1: no cut, an end of the blade).
  var cuts = vec2f(1.0);
  if (g > 0.5) {
    let d0 = normalize(a - blade_point(g - 1.0, segs, th, st, len) + vec2f(0.0, -1e-6));
    let n = normalize(d0 + dir);
    cuts.x = 4.0 + atan2(dot(n, across), dot(n, dir));
  }
  if (g < segs - 1.5) {
    let d1 = normalize(blade_point(g + 2.0, segs, th, st, len) - b + vec2f(0.0, -1e-6));
    let n = normalize(dir + d1);
    cuts.y = 4.0 + atan2(dot(n, across), dot(n, dir));
  }
  // Tapering from the width at the root to half of it at the tip; the round ends at root and tip stay
  // within them.
  let ww = max(w * (1.0 - 0.5 * (g + 0.5) / segs), 0.5);
  if (g < 0.5) { a += dir * min(ww * 0.5, distance(a, b) * 0.5); }
  if (g > segs - 1.5) { b -= dir * min(ww * 0.5, distance(a, b) * 0.5); }
  let l = distance(a, b);
  let c = root + (a + b) * 0.5;
  i.rect = vec4f(c.x - (l + ww) * 0.5, c.y - ww * 0.5, l + ww, ww);
  i.uv = vec4f(ww * 0.5, 0.0, cuts);
  i.xf = vec4f(dir.x - 1.0, -dir.y, dir.y, dir.x - 1.0);
  i.paint = blade_paint(i.paint, i.extra.y, th, iv.uv.y, iv.resp.zw);
  i.extra.y = 0.0;
  i.look.z = 0.0; i.look.w = 0.0;
  i.resp.y = 0.0;
  return i;
}
/// A blade's head (a glyph): rect upright, the bottom middle of its ink on the upright tip; xf the root
/// (x, y), the length, stiffness × 100 + 128 × the most lean (0.1°); extra.y −(1 + (own lean + 90°) × 10);
/// extra.x the blade's lean paint as a segment's extra.y. Turned about that point with the tip's tangent
/// (the lean) and moved onto the tip; its paint goes to the lean paint as the blade's does.
fn blade_head(iv: VIn) -> VIn {
  var i = iv;
  let root = i.xf.xy; let len = i.xf.z;
  let mq = floor(i.xf.w / 128.0); let st = (i.xf.w - mq * 128.0) / 100.0;
  let most = radians(mq / 10.0);
  let own = radians((-i.extra.y - 1.0) / 10.0 - 90.0);
  let th = blade_lean(root, own, most);
  let tip = blade_at(1.0, th, st, len);
  let cs = cos(th); let sn = sin(th);
  let up = root + vec2f(0.0, -len);
  let mid = i.rect.xy + i.rect.zw * 0.5 - up;
  let m2 = root + tip + vec2f(cs * mid.x - sn * mid.y, sn * mid.x + cs * mid.y);
  i.rect = vec4f(m2 - i.rect.zw * 0.5, i.rect.zw);
  i.xf = vec4f(cs - 1.0, -sn, sn, cs - 1.0);
  i.paint = blade_paint(i.paint, i.extra.x, th, most, iv.resp.zw);
  i.extra.x = 0.0; i.extra.y = 0.0;
  i.look.z = 0.0; i.look.w = 0.0;
  i.resp.y = 0.0;
  return i;
}

@vertex fn vs_inst(i: VIn) -> VOut { return vs_pad(i, u.fd.z); }
/// How far the fronts passing `centre` push what is there (device px), outward from each.
fn fronts_push(centre: vec2f) -> vec2f {
  let w = u.fb.z * 1.3 * u.res.z;
  var push = vec2f(0.0);
  for (var k = 0; k < 8; k++) {
    if (!in_scope(centre, k)) { continue; }
    let f = front_at(centre, u.fronts[k]);
    let dir = normalize(centre - u.fronts[k].xy + vec2f(1e-4));
    push += dir * exp(-(f.y * f.y) / (w * w)) * f.x * u.fc.x * u.res.z;
  }
  return push;
}
/// A glyph box (device px) at display size, 32 px or more each way, under an override (`look.x`) by a
/// material that draws round the ink: carvr (5, its ridges) or near (7, its bands).
fn display_override(size: vec2f, code: f32) -> bool {
  let id = i32(round(code)) % 16;
  return code >= 127.5 && (id == 5 || id == 7) && min(size.x, size.y) >= 32.0 * u.res.z;
}
fn vs_pad(iv: VIn, spad: f32) -> VOut {
  var i = iv;
  if (i.paint.w > 0.5 && i.paint.w < 1.5 && i.extra.y > 0.5) { i = blade_seg(iv); }
  else if (i.paint.w < 0.5 && i.extra.y < -0.5) { i = blade_head(iv); }
  let c = vec2f(f32(i.vi & 1u), f32((i.vi >> 1u) & 1u));
  // Glyph rects already include the spread; shapes and pictures are padded here so their nearness fits.
  // A glyph at display size in its own material is padded by the spread once more, for what its
  // material draws round it (`near_in`); past its atlas slot it has no ink (`inst`).
  let pad = select(spad, select(0.0, spad, display_override(i.rect.zw, i.look.x)), i.paint.w < 0.5);
  let r = vec4f(i.rect.xy - vec2f(pad), i.rect.zw + vec2f(pad * 2.0));
  var p = r.xy + c * r.zw;
  let mid = i.rect.xy + i.rect.zw * 0.5;
  let q = p - mid;
  p = mid + q + vec2f(i.xf.x * q.x + i.xf.y * q.y, i.xf.z * q.x + i.xf.w * q.y);
  // Each instance rides the wave: it is pushed outward a little as a front passes under it.
  // A legible piece is pushed as one: every instance of it measures the push at the piece's centre.
  let centre = select(i.rect.xy + i.rect.zw * 0.5, i.resp.zw, i.resp.y > 1.5);
  let still = calm_at(centre) || i.resp.y < 0.5;
  var push = select(fronts_push(centre), vec2f(0.0), still);
  // The field map's flow carries what may be pushed and reads it (look.z), by its centre; not a picture
  // or a 3D view (look.w), which loosens in place.
  if (!still && i.look.z > 0.5 && i.look.w % 2.0 < 0.5) { push += fmap_flow(centre) * u.fmap.y; }
  // A legible piece moves by whole device pixels, so its glyphs are never sampled between texels (a
  // label pushed by a press would blur for that frame and lose its contrast).
  if (i.resp.y > 1.5) { push = round(push); }
  p += push;
  var o: VOut;
  o.pos = vec4f(p.x / u.res.x * 2.0 - 1.0, 1.0 - p.y / u.res.y * 2.0, 0.0, 1.0);
  o.local = c * r.zw - vec2f(pad);
  o.size = i.rect.zw;
  o.uv = select(i.uv.xy + o.local / i.rect.zw * (i.uv.zw - i.uv.xy), mix(i.uv.xy, i.uv.zw, c), i.paint.w < 0.5 && pad == 0.0);
  o.paint = i.paint; o.clip = i.clip; o.extra = i.extra; o.radius = vec2f(i.uv.x, select(0.0, 1.0, i.paint.w > 1.5 && i.resp.y < 1.5 && i.resp.w > 0.5)); o.look = i.look;
  o.resp = vec4f(i.resp.x, i.resp.y, centre);
  o.org = i.rect.xy + push;
  o.uvr = i.uv;
  o.xf = i.xf;
  return o;
}

/// How far outside its round clip (`El::clip_round`) the pixel is, device px (−1: a square clip).
fn clip_d(i: VOut) -> f32 {
  let r = floor(i.look.w * 0.5) * 0.25;
  if (r <= 0.0) { return -1.0; }
  return box_d(i.pos.xy - i.clip.xy, i.clip.zw - i.clip.xy, r);
}
fn box_d(local: vec2f, size: vec2f, r: f32) -> f32 {
  let rr = min(r, min(size.x, size.y) * 0.5);
  let q = abs(local - size * 0.5) - (size * 0.5 - vec2f(rr));
  return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - rr;
}

// coverage, signed nearness, picture flag, signed distance (device px, negative inside; glyphs from
// the atlas' nearness)
fn inst(i: VOut) -> vec4f {
  if (i.pos.x < i.clip.x || i.pos.y < i.clip.y || i.pos.x >= i.clip.z || i.pos.y >= i.clip.w) { discard; }
  if (clip_d(i) > 0.5) { discard; }
  let kind = i.paint.w;
  if (kind > 2.5) {
    // A 3D element (DESIGN.md §1.18): its resolve gives the silhouette's and the faces' signed nearness
    // (filtered: the target may be smaller than the box). Past the box it falls off by the distance.
    let e = mesh_near(i);
    let over = max(max(-i.local.x, i.local.x - i.size.x), max(-i.local.y, i.local.y - i.size.y));
    let sil = (e.r - 0.5) * 2.0 * u.fd.z - max(over, 0.0);
    let face = (e.g - 0.5) * 2.0 * u.fd.z - max(over, 0.0);
    let pic = i.extra.x > 0.5 && sil > 0.0;
    return vec4f(clamp(sil + 0.5, 0.0, 1.0), clamp(0.5 + face / (2.0 * u.fd.z), 0.0, 1.0), select(0.0, 1.0, pic), -face);
  }
  if (kind > 1.5 && i.extra.x > 0.5) {
    // Character marks over a picture (`el::Cells`): ink only where a character is.
    let c = cells_of(i);
    return vec4f(c.a, 0.5 + 0.5 * c.a, select(0.0, 1.0, c.a > 0.0), 0.5 - c.a);
  }
  if (kind > 1.5) {
    // A picture with an alpha channel (a cut-out) has its true outline: outside it, nothing is there.
    var d = box_d(i.local, i.size, 0.0);
    let ta = textureSampleLevel(tex1, samp, clamp(i.uv, vec2f(0.0), vec2f(1.0)), 0.0).a;
    let inside = d <= 0.0 && ta > 0.5;
    let near = clamp(0.5 - d / (2.0 * u.fd.z), 0.0, 1.0) * step(0.5, ta);
    return vec4f(clamp(0.5 - d, 0.0, 1.0) * ta, near, select(0.0, 1.0, inside), d);
  }
  if (kind < 0.5 && i.uvr.x < -0.5) {
    // A view of a cached plate (`el::cached_view`): its ink's coverage at the texel under the pixel,
    // nearest, as a glyph's (tex0 is the plate's ink here).
    let t = -1.0 - i.uv - 4.0 * floor((-1.0 - i.uvr.x) / 4.0);
    let dims = vec2f(textureDimensions(tex0));
    let q = vec2i(clamp(floor(t * dims), vec2f(0.0), dims - 1.0));
    let cov = textureLoad(tex0, q, 0).a;
    let near = select(0.0, 0.25 + 0.5 * cov, cov > 0.0);
    return vec4f(cov, near, 0.0, (0.5 - near) * 2.0 * u.fd.z);
  }
  if (kind < 0.5) {
    if (any(i.uv < i.uvr.xy) || any(i.uv > i.uvr.zw)) { return vec4f(0.0, 0.0, 0.0, 2.0 * u.fd.z); }
    let g = textureSampleLevel(tex0, samp, i.uv, 0.0);
    // A weight (device px in extra.x) moves the outline: the coverage comes from the signed distance.
    if (i.extra.x != 0.0) {
      let sd = (g.g - 0.5) * 2.0 * u.fd.z + i.extra.x;
      return vec4f(clamp(sd + 0.5, 0.0, 1.0), clamp(0.5 + sd / (2.0 * u.fd.z), 0.0, 1.0), 0.0, -sd);
    }
    return vec4f(g.r, g.g, 0.0, (0.5 - g.g) * 2.0 * u.fd.z);
  }
  // A blade's segment (`blade_seg`): what lies past a joint's bisector is the next segment's, by the
  // pixel's centre, so every pixel is one segment's and no joint is drawn twice.
  if (i.paint.w > 0.5 && i.paint.w < 1.5 && i.uvr.z > 0.5) {
    let r = i.size.y * 0.5;
    if (i.uvr.z > 2.0 && dot(i.local - vec2f(r, r), vec2f(cos(i.uvr.z - 4.0), sin(i.uvr.z - 4.0))) < 0.0) { discard; }
    if (i.uvr.w > 2.0 && dot(i.local - vec2f(i.size.x - r, r), vec2f(cos(i.uvr.w - 4.0), sin(i.uvr.w - 4.0))) >= 0.0) { discard; }
  }
  var d = box_d(i.local, i.size, i.radius.x);
  if (i.extra.x > 0.0) { d = abs(d + i.extra.x * 0.5) - i.extra.x * 0.5; }
  let near = clamp(0.5 - d / (2.0 * u.fd.z), 0.0, 1.0);
  return vec4f(clamp(0.5 - d, 0.0, 1.0), near, 0.0, d);
}

/// Character marks over a picture (`el::Cells`, DESIGN.md §1.6.1): the colour of this pixel's cell and
/// how much of the pixel its character covers. Packed by ui.rs: extra.x = characters + 64 × (cell width
/// + 256 × cell height), device px, plus a quarter when bright cells are dense; look.y = the ink's hue + 4
/// × (the strip's atlas texel x + 2048 × y); paint.x = mix + 256 × tone + 65536 × accent, in 255ths; uv the
/// box in the picture's coordinates.
fn cells_of(i: VOut) -> vec4f {
  let e = floor(i.extra.x);
  let light = i.extra.x - e > 0.1;
  let n = e % 64.0; let wh = floor(e / 64.0);
  let pc = vec2f(wh % 256.0, floor(wh / 256.0));
  let at = floor(i.look.y / 4.0);
  let hue = i.look.y - at * 4.0;
  let org = vec2f(at % 2048.0, floor(at / 2048.0));
  let grid = floor(i.size / pc + 1e-3);
  let cell = floor(i.local / pc);
  if (cell.x < 0.0 || cell.y < 0.0 || cell.x >= grid.x || cell.y >= grid.y || n < 1.0) { return vec4f(0.0); }
  // The cell's colour: the picture at four points of it.
  let c0 = cell * pc;
  var acc = vec3f(0.0);
  for (var j = 0; j < 2; j++) {
    for (var k = 0; k < 2; k++) {
      let lp = c0 + (vec2f(f32(k), f32(j)) + 0.5) * pc * 0.5;
      let uv = i.uvr.xy + lp / i.size * (i.uvr.zw - i.uvr.xy);
      acc += textureSampleLevel(tex1, samp, clamp(uv, vec2f(0.0), vec2f(1.0)), 0.0).rgb;
    }
  }
  let c = acc * 0.25;
  let l = clamp(lum(c), 0.0, 1.0);
  let lev = clamp(floor(select(1.0 - l, l, light) * n), 0.0, n - 1.0);
  let t = floor(i.local - c0);
  let cov = textureLoad(tex0, vec2i(i32(org.x + lev * pc.x + t.x), i32(org.y + t.y)), 0).r;
  let pk = i.paint.x;
  let m = (pk % 256.0) / 255.0;
  let tone = (floor(pk / 256.0) % 256.0) / 255.0;
  let ac = floor(pk / 65536.0) / 255.0;
  let ink = grey(tone, i.pos.xy) * (1.0 - ac) + hue_col(code_hue(hue)) * ac;
  return vec4f(mix(c, ink, m), cov);
}
/// A 3D element's resolved nearness here (tex0 is its resolve for mesh batches).
fn mesh_near(i: VOut) -> vec4f { return textureSampleLevel(tex0, samp, clamp(i.uv, vec2f(0.0), vec2f(1.0)), 0.0); }
/// A 3D element's colour texel here, nearest (tex1 is its colour image): shade, tone, accent and
/// 0.5 + hue / 4. At the silhouette's filtered edge an empty texel takes a drawn neighbour's colour.
/// The outline's tone of the copies drawn nearest here (the resolve's alpha, unfiltered; just off the
/// silhouette, a neighbour's); the ink where none is.
/// The texel of a 3D element's target that stands for this pixel: from the pixel's whole position in
/// the box (not the interpolated uv, which lands on a texel's edge at whole scales and rounds either
/// way by the driver), the texel under its centre, `floor((k + ½) × dims ÷ size)`, exact in floats for a
/// box of whole pixels at any scale whose target is that many times its size. At scale 2 a pixel covers
/// its own 2 × 2 texels and stands for the lower right one. A turned element maps by its uv.
fn mesh_texel(i: VOut, dims: vec2f) -> vec2i {
  var t = floor(clamp(i.uv, vec2f(0.0), vec2f(1.0)) * dims);
  if (all(i.xf == vec4f(0.0))) { t = floor((floor(i.pos.xy) - i.org + 0.5) * dims / i.size); }
  return vec2i(clamp(t, vec2f(0.0), dims - 1.0));
}
fn mesh_outline(i: VOut) -> f32 {
  let dims = vec2f(textureDimensions(tex0));
  let q = mesh_texel(i, dims);
  let lim = vec2i(dims) - 1;
  var dirs = array<vec2i, 9>(vec2i(0, 0), vec2i(1, 0), vec2i(-1, 0), vec2i(0, 1), vec2i(0, -1), vec2i(1, 1), vec2i(-1, 1), vec2i(1, -1), vec2i(-1, -1));
  for (var k = 0; k < 9; k++) {
    let t = textureLoad(tex0, clamp(q + dirs[k], vec2i(0), lim), 0).a;
    if (t > 0.5 / 255.0) { return t; }
  }
  return 1.0;
}
fn mesh_px(i: VOut) -> vec4f {
  let dims = vec2f(textureDimensions(tex1));
  let q = mesh_texel(i, dims);
  var c = textureLoad(tex1, q, 0);
  if (c.a < 0.25) {
    let lim = vec2i(dims) - 1;
    var dirs = array<vec2i, 8>(vec2i(1, 0), vec2i(-1, 0), vec2i(0, 1), vec2i(0, -1), vec2i(1, 1), vec2i(-1, 1), vec2i(1, -1), vec2i(-1, -1));
    for (var k = 0; k < 8; k++) {
      let n = textureLoad(tex1, clamp(q + dirs[k], vec2i(0), lim), 0);
      if (n.a >= 0.25) { c = n; break; }
    }
  }
  return c;
}

/// The ink code in b (see the top of this file).
fn ink_code(i: VOut) -> f32 { return theme_hue(i.look.y) * 0.15 + select(0.0, 0.05, i.paint.w > 0.5); }
/// How loose an instance is here: its own looseness and the fronts passing.
fn loose1(i: VOut) -> f32 { return clamp(own_loose(i) * loose_scale(), 0.0, u.rip.x); }
/// An instance's own looseness here (held, born, set at rest) plus what the fronts that reach it add,
/// which only fills up to its cap: text to read never passes `legible_cap` by the field.
fn own_loose(i: VOut) -> f32 {
  // Less motion (scrq.w): nothing is born loose; it fades in instead (`rm_fade`).
  let born = select(clamp((u.scrq.y - i.extra.w) / max(u.fc.z, 1e-3), 0.0, 1.0), 1.0, u.scrq.w > 0.0);
  // Text to read and controls (a piece pushed as one) are born up to the legibility cap only: a control
  // that changes (a switch turned) is born again, and its label and track stay readable while it does.
  let k = max(i.extra.z, select(1.0 - born, min(1.0 - born, u.burst_t.w), i.resp.y > 1.5));
  if (i.resp.x <= 0.0) { return k; }
  var w = wave_in(i.pos.xy, i.resp.zw, 0.6);
  // The lens is a source of the field like a front: it reaches pictures and 3D elements that may be
  // loosened and that the pointer is not operating (the flag in radius.y), up to their cap.
  if (i.radius.y > 0.5) { w = max(w, lens(i.pos.xy)); }
  // The field map is one more source, read where the instance is drawn (not a stirring element's own
  // subtree, not what a morph or recomposition flies: look.z is 0 there).
  if (i.look.z > 0.5) { w = max(w, fmap_e(i.pos.xy)); }
  // A 3D element also loosens under a hover's burst.
  if (i.paint.w > 2.5) { w = max(w, burst_k(i.pos.xy)); }
  return k + clamp(w, 0.0, max(i.resp.x - k, 0.0));
}

@fragment fn fs_ink(i: VOut) -> @location(0) vec4f { return ink_of(exact_i(i), false); }
// The modal layer: the same (its own pipeline, drawn without blending in the near image).
@fragment fn fs_ink_top(i: VOut) -> @location(0) vec4f { return ink_of(exact_i(i), true); }
/// Less motion asked for (scrq.w): a new instance fades in over scrq.w (`Ui::reduced_fade`) instead of condensing.
fn rm_fade(i: VOut) -> f32 { return select(1.0, clamp((u.scrq.y - i.extra.w) / max(u.scrq.w, 1e-3), 0.0, 1.0), u.scrq.w > 0.0); }
// ── A stroke with a ramp (draw.rs `Drawing::ramp`; ui.rs packs it): its far paint in uv.z and middle
// paint in uv.w, each −(1 + tone + 256 accent + 65536 alpha) in 255ths; extra.x −(1 + far hue + 4 middle
// hue + 16 × where the middle is, in 255ths).
fn is_ramp(i: VOut) -> bool { return i.paint.w > 0.5 && i.paint.w < 1.5 && i.uvr.z < -0.5; }
fn ramp_paint(v: f32) -> vec3f {
  let n = -v - 1.0;
  let alpha = floor(n / 65536.0); let r = n - alpha * 65536.0; let accent = floor(r / 256.0);
  return vec3f(r - accent * 256.0, accent, alpha) / 255.0;
}
/// How far along the stroke the pixel is, a to b (0–1): its projection on the stroke, so each round end
/// is its end's colour.
fn ramp_t(i: VOut) -> f32 { let w = i.size.y; return clamp((i.local.x - w * 0.5) / max(i.size.x - w, 1e-3), 0.0, 1.0); }
/// The ramp's stops at t: the two paints (tone, accent, alpha) and hues it lies between, and how far.
struct RampAt { p0: vec3f, p1: vec3f, h0: f32, h1: f32, f: f32 }
fn ramp_at(i: VOut) -> RampAt {
  let t = ramp_t(i);
  let code = -i.extra.x - 1.0;
  let at = floor(code / 4096.0) / 255.0; let hues = code - floor(code / 4096.0) * 4096.0;
  let mh = floor(hues / 64.0); let fh = hues - mh * 64.0;
  // Each stop's opacity is a share of the instance's (`Ui::paint`), the start's in extra.y.
  let k = vec3f(1.0, 1.0, i.paint.z);
  let start = vec3f(i.paint.xy, (-i.extra.y - 1.0) * i.paint.z); let far = ramp_paint(i.uvr.z) * k;
  if (i.uvr.w < -0.5) {
    let mid = ramp_paint(i.uvr.w) * k;
    if (t < at) { return RampAt(start, mid, i.look.y, mh, t / max(at, 1e-4)); }
    return RampAt(mid, far, mh, fh, (t - at) / max(1.0 - at, 1e-4));
  }
  return RampAt(start, far, i.look.y, fh, t);
}
fn lin1(c: f32) -> f32 { return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045); }
fn srgb1(c: f32) -> f32 { return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308); }
fn oklab(c: vec3f) -> vec3f {
  let l = vec3f(lin1(c.r), lin1(c.g), lin1(c.b));
  let lms = pow(max(mat3x3f(0.4122214708, 0.2119034982, 0.0883024619, 0.5363325363, 0.6806995451, 0.2817188376, 0.0514459929, 0.1073969566, 0.6299787005) * l, vec3f(0.0)), vec3f(1.0 / 3.0));
  return mat3x3f(0.2104542553, 1.9779984951, 0.0259040371, 0.7936177850, -2.4285922050, 0.7827717662, -0.0040720468, 0.4505937099, -0.8086757660) * lms;
}
fn from_oklab(lab: vec3f) -> vec3f {
  let lms = mat3x3f(1.0, 1.0, 1.0, 0.3963377774, -0.1055613458, -0.0894841775, 0.2158037573, -0.0638541728, -1.2914855480) * lab;
  let l = mat3x3f(4.0767416621, -1.2684380046, -0.0041960863, -3.3077115913, 2.6097574011, -0.7034186147, 0.2309699292, -0.3413193965, 1.7076147010) * (lms * lms * lms);
  let c = clamp(l, vec3f(0.0), vec3f(1.0));
  return vec3f(srgb1(c.r), srgb1(c.g), srgb1(c.b));
}
/// A paint's colour as drawn over a picture: ground, ink and its accent mixed (`ink_of`).
fn paint_col(p: vec3f, hue: f32, at: vec2f) -> vec3f { return grey(p.x, at) * (1.0 - p.y) + hue_col(round(hue)) * p.y; }

fn ink_of(iv: VOut, solid: bool) -> vec4f {
  var i = iv;
  // A ramp off a picture: its paint's numbers in a line, its own hue all the way (one hue a pixel).
  var ramp: RampAt;
  let ramped = is_ramp(iv);
  if (ramped) { ramp = ramp_at(iv); i.paint = vec4f(mix(ramp.p0, ramp.p1, ramp.f), i.paint.w); }
  let s = inst(i);
  var a = s.x * i.paint.z * rm_fade(i) * clamp(0.5 - clip_d(i), 0.0, 1.0);
  // A shape scribblr fades (its alpha before the fade in uv.y) keeps its fill within `r0` of text to read
  // that holds it, to a one-pixel edge: no soft rim.
  if (i.paint.w > 0.5 && i.paint.w < 1.5 && i.uvr.y > 0.0) { a = mix(a, s.x * i.uvr.y * rm_fade(i), core_at(i.pos.xy)); }
  if (i.paint.w > 2.5) {
    // Flat shading in the theme's colours: the lit share of the object's tone and accent, so the
    // material decodes it like any shape (code 0.07 above the hue: a shape, and a mesh).
    let c = mesh_px(i);
    let hue = clamp(round((c.a - 0.5) * 4.0), 0.0, 2.0);
    let r = c.r * c.g * (1.0 - c.b);
    let g = c.r * c.b;
    if (i.extra.x > 0.5) {
      // Drawn as a picture: its own colours, straight.
      let col = paint_rgb(r, g, hue, vec2f(0.0));
      return vec4f(col * a, a);
    }
    let code = hue * 0.15 + 0.07;
    // A drawn outline on the silhouette and the creases (Mesh3d::outline, device px in paint.x), in its
    // copies' grey (`MeshInstances::outline`, the resolve's alpha, nearest).
    if (i.paint.x > 0.0) {
      let line = clamp(i.paint.x + 0.5 - abs(s.w), 0.0, 1.0) * step(0.02, s.x);
      return vec4f(mix(r, mesh_outline(i), line) * a, mix(g, 0.0, line) * a, code * a, a);
    }
    return vec4f(r * a, g * a, code * a, a);
  }
  if (i.paint.w > 1.5 && i.extra.x > 0.5) { return vec4f(cells_of(i).rgb * a, a); }
  if (i.paint.w > 1.5) { return vec4f(textureSampleLevel(tex1, samp, clamp(i.uv, vec2f(0.0), vec2f(1.0)), 0.0).rgb * a, a); }
  // Over a picture the pixel is read as the picture's colours (its flag is in the near image), so text
  // and shapes there are written as their own colour, straight, as the material shows them at rest: an
  // antialiased edge then blends with the picture by its coverage, and a see-through group by its
  // opacity, whatever else is drawn.
  if (i.paint.w < 1.5 && over_pic(i, solid) > 0.0) {
    // A ramp over a picture: its stops' colours mixed in OKLab, the opacity after.
    if (ramped) {
      let col = from_oklab(mix(oklab(paint_col(ramp.p0, ramp.h0, i.pos.xy)), oklab(paint_col(ramp.p1, ramp.h1, i.pos.xy)), ramp.f));
      return vec4f(col * a, a);
    }
    let r = i.paint.x * (1.0 - i.paint.y); let g = i.paint.y;
    let col = paint_rgb(r, g, round(i.look.y), i.pos.xy);
    return vec4f(col * a, a);
  }
  // Text set in the ground's colour (a label on a solid button) under `near`, which draws panels open:
  // written as the ink's colour, by the text's own paint, so its antialiased edges over the panel decode
  // as text and panel, not as a colour between the panel and the ground.
  var tone = i.paint.x;
  if (i.paint.w < 0.5 && i.paint.x < 0.02 && i.paint.y < 0.02 && eff_mat(i.look.x) == NEAR) { tone = 1.0; }
  return vec4f(tone * (1.0 - i.paint.y) * a, i.paint.y * a, ink_code(i) * a, a);
}
/// The flag of the picture under this pixel of a glyph or shape (the hold image's b, 0 for none). In the
/// modal layer, only where none of its own shapes or pictures is (a): over those the ink is the layer's.
fn over_pic(i: VOut, top: bool) -> f32 {
  let h = textureLoad(hold, vec2i(clamp(floor(i.pos.xy), vec2f(0.0), u.res.xy - 1.0)), 0);
  return select(h.b, 0.0, top && h.a > 0.5);
}
fn near_of(i: VOut) -> vec4f { return near_in(i, false); }
fn near_in(i: VOut, top: bool) -> vec4f {
  let s = inst(i);
  let born = clamp((u.scrq.y - i.extra.w) / max(u.fc.z, 1e-3), 0.0, 1.0);
  // Looseness belongs to the instance's own ink: where it has ink (coverage over a half) it is written
  // as 0.5 + k / 2, in its halo as 0.49 k. Max-blended, any ink beats any halo, so one instance's
  // looseness never spills onto a neighbour's ink (a new marker beside a column of dots).
  let loose = enc_loose(min(own_loose(i), 1.0), s.x * i.paint.z > 0.5) * step(0.02, s.y) * step(0.01, i.paint.z);
  // A picture marks where it is only while it shows (as the hold image's flag, `Renderer::upload`).
  var slot = select(0.0, (i.paint.y + 1.0) / 8.0 - select(0.0, 1.0 / 32.0, i.resp.y > 1.5), s.z > 0.5 && i.paint.z >= 0.01);
  // Ink written as a picture's colour keeps that picture's flag where it is drawn (`ink_of`).
  if (i.paint.w < 1.5 && s.x > 0.0) { slot = max(slot, over_pic(i, top)); }
  // A 3D element drawn through the material marks its pixels (within the nearness) with the projected
  // normal's code, 1..9 / 255: below any picture's flag, so nothing else reads it (charr does).
  if (i.paint.w > 2.5 && i.extra.x < 0.5 && s.y > 0.01) { slot = max(round(mesh_near(i).b * 255.0), 1.0) / 255.0; }
  var code = select(0.0, round(i.extra.y * 2.0) / 255.0, i.paint.w < 0.5 && s.y > 0.02);
  // A material override reaches through the nearness of shapes and pictures, but only just around
  // glyphs, so text set in its own material does not change the shape it sits on. A glyph at display
  // size in a material that draws round the ink takes all the ground its quad covers: near's bands and
  // carvr's ridges are part of the letter.
  if (i.look.x >= 127.5 && (s.y > select(0.02, 0.42, i.paint.w < 0.5) || (i.paint.w < 0.5 && display_override(i.size, i.look.x)))) { code = i.look.x / 255.0; }
  return vec4f(s.y * step(0.01, i.paint.z), slot, loose, code);
}
// Text to read over others' ink (scene.rs Scene::holds): 1 on its own ink and within paint.x device px of
// its true outline, falling smoothly to 0 at paint.y; max-blended into the hold image.
@vertex fn vs_hold(i: VIn) -> VOut { return vs_pad(i, i.paint.y); }
/// The text mask (`PostSpec::clear_text`): 1 on a glyph of text to read and within 2 device px of its
/// outline, falling to 0 a pixel further (a glyph's rect already reaches SPREAD past its ink).
@vertex fn vs_tmask(i: VIn) -> VOut { return vs_pad(i, 0.0); }
@fragment fn fs_tmask(iv: VOut) -> @location(0) vec4f {
  let s = inst(exact_i(iv));
  let d = max(s.w, 0.0);
  return vec4f(clamp((4.0 - d) * 0.5, 0.0, 1.0), 0.0, 0.0, 0.0);
}
@fragment fn fs_hold(iv: VOut) -> @location(0) vec4f {
  // A control's island (paint.x −3, `Ui::holds`): its box (rect, radius in uv.x) within the control's
  // outline at rest (clip, radius in look.x); hard, a one-pixel edge, and held all through (r and g).
  if (iv.paint.x < -2.5) {
    let p = iv.pos.xy;
    let d1 = box_d(p - iv.org, iv.size, iv.radius.x);
    let d2 = box_d(p - iv.clip.xy, iv.clip.zw - iv.clip.xy, iv.look.x);
    let a = clamp(0.5 - max(d1, d2), 0.0, 1.0);
    return vec4f(a, a, 0.0, 0.0);
  }
  let i = exact_i(iv);
  let s = inst(i);
  // A picture of the page: its flag where it is (b). The modal layer's own shapes and pictures: a.
  if (i.paint.x < -1.5) { return vec4f(0.0, 0.0, 0.0, step(0.02, s.x)); }
  if (i.paint.x < -0.5) { return vec4f(0.0, 0.0, i.paint.y * step(0.5, s.z), 0.0); }
  let d = max(s.w, 0.0);
  // g: within `r0` of the outline, with a one-pixel edge (the island a material keeps exactly).
  return vec4f(1.0 - smoothstep(i.paint.x, i.paint.y, d), clamp(i.paint.x - d + 0.5, 0.0, 1.0), 0.0, 0.0);
}
// ── Stage 3: annotations, exactly, in the gaze colour (paint.y mixes toward the page ground, for
// the plates under labels) ─────────────────────────────────────────────────────────────────────────
@vertex fn vs_annot(i: VIn) -> VOut {
  let c = vec2f(f32(i.vi & 1u), f32((i.vi >> 1u) & 1u));
  // Its own turn about its centre (`xf`, zero for none): edge notes are turned a quarter.
  let q = (c - 0.5) * i.rect.zw;
  var p = i.rect.xy + c * i.rect.zw + vec2f(i.xf.x * q.x + i.xf.y * q.y, i.xf.z * q.x + i.xf.w * q.y);
  // The hover / focus marks ride the fronts with the control they frame, by whole device pixels.
  if (i.resp.y > 1.5) { p += round(fronts_push(i.resp.zw)); }
  var o: VOut;
  o.pos = vec4f(p.x / u.res.x * 2.0 - 1.0, 1.0 - p.y / u.res.y * 2.0, 0.0, 1.0);
  o.local = c * i.rect.zw;
  o.org = i.rect.xy;
  o.uvr = i.uv;
  o.size = i.rect.zw;
  o.uv = mix(i.uv.xy, i.uv.zw, c);
  o.paint = i.paint; o.clip = i.clip; o.extra = i.extra; o.radius = vec2f(0.0); o.look = i.look;
  o.xf = vec4f(0.0);
  return o;
}
@fragment fn fs_annot(i: VOut) -> @location(0) vec4f {
  if (i.pos.x < i.clip.x || i.pos.y < i.clip.y || i.pos.x >= i.clip.z || i.pos.y >= i.clip.w) { discard; }
  var a = 1.0;
  if (i.paint.w < 0.5) { a = textureSampleLevel(tex0, samp, i.uv, 0.0).r; }
  a *= i.paint.z;
  var c = mix(i.look.rgb, u.bg.rgb, i.paint.y);
  // Black and white only: the marks are ink where they cover half the pixel (as text is, shade_new).
  if (u.post[4].y > 0.5) { a = step(0.5, a); c = mix(u.fg.rgb, u.bg.rgb, i.paint.y); }
  if (u.fd.y > 0.5) { c = pow(c, vec3f(2.2)); }
  return vec4f(c * a, a);
}
@fragment fn fs_near(i: VOut) -> @location(0) vec4f { return near_of(exact_i(i)); }

// The modal layer (and parts in flight), drawn without blending: wherever it has any ink it replaces what is beneath, so beneath's
// looseness and picture flag stop at its outline.
@fragment fn fs_near_top(iv: VOut) -> @location(0) vec4f {
  let i = exact_i(iv);
  let s = inst(i);
  if (s.x < 0.02 || i.paint.z < 0.01) { discard; }
  // A glyph's faint outer edge leaves the panel's nearness under it, so the materials that read the
  // nearness (collage's torn edge, carvr's cut) do not take a label's rim for the panel's outline.
  if (i.paint.w < 0.5 && s.y < 0.5) { discard; }
  return near_in(i, true);
}

// ── Stage 2: material ─────────────────────────────────────────────────────────────────────────────
@vertex fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4f {
  return vec4f(f32((vi << 1u) & 2u) * 2.0 - 1.0, f32(vi & 2u) * 2.0 - 1.0, 0.0, 1.0);
}
fn h1(p: vec2f) -> f32 { return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453); }
fn h2(p: vec2f) -> vec2f { return vec2f(h1(p), h1(p + vec2f(19.19, 73.3))); }

/// How disturbed a point is by passing fronts: 1 on a front, a soft tail behind it.
fn calm_at(p: vec2f) -> bool {
  for (var j = 0; j < 4; j++) {
    let r = u.calm[j];
    if (r.z > r.x && p.x >= r.x && p.y >= r.y && p.x < r.z && p.y < r.w) { return true; }
  }
  return false;
}
/// The fronts at a pixel of the ground (no instance): a front reaches it when the pixel is in its scope.
fn wave(p: vec2f, tail_gain: f32) -> f32 { return wave_in(p, p, tail_gain); }
/// The fronts at pixel `p` of an instance centred at `c`: only fronts whose scope holds `c`.
fn wave_in(p: vec2f, c: vec2f, tail_gain: f32) -> f32 {
  if (calm_at(p)) { return 0.0; }
  var k = 0.0;
  let s = u.res.z;
  let w = max(u.fb.z, 1e-3) * s;
  let tail = max(u.fb.w, 1e-3) * s;
  for (var j = 0; j < 8; j++) {
    // A slot with no front costs a test, not the front's two exponentials.
    if (u.fronts[j].w <= 0.0 || !in_scope(c, j)) { continue; }
    let f = front_at(p, u.fronts[j]);
    let front = exp(-(f.y * f.y) / (w * w));
    let behind = select(0.0, exp(f.y / tail) * tail_gain, f.y < 0.0);
    k = max(k, max(front, behind) * f.x);
  }
  return k;
}
/// The half-plane source (compare wipe): `strength` right of the line, inside its rect.
fn half_plane(p: vec2f) -> f32 {
  let r = u.wipe_rect;
  let inside = p.x >= r.x && p.y >= r.y && p.x < r.z && p.y < r.w && p.x >= u.wipe.x;
  return select(0.0, u.wipe.y, inside && u.wipe.y > 0.0);
}
fn lens(p: vec2f) -> f32 {
  let d = (p - u.pointer.xy) / (max(u.fc.y, 1e-3) * u.res.z);
  let bell = exp(-dot(d, d));
  let disc = 1.0 - step(1.0, length(d));
  return mix(bell, disc, clamp(u.pointer.w, 0.0, 1.0)) * u.pointer.z * step(0.5, u.fc.y);
}
/// Looseness as stage 1 writes it into near.b (8 bits): on an instance's own ink 128 + 127 k, in its
/// halo 125 k, so any ink is above any halo and 0 is exactly 0 either way.
fn enc_loose(k: f32, owns: bool) -> f32 { return select(round(125.0 * k), 128.0 + round(127.0 * k), owns) / 255.0; }
fn dec_loose(b: f32) -> f32 { let v = round(b * 255.0); return select(v / 125.0, (v - 128.0) / 127.0, v >= 128.0); }
/// How loose the ink (or the halo) at an exact texel is.
/// The looseness here: what the near image holds, no more than the legibility cap where text to read
/// holds the ink under it and around it (the hold image: 1 on it and within r0, falling to 0 at r1).
/// How firmly text to read holds the looseness here (0 nowhere near any, 1 on it and within r0).
fn hold_at(p: vec2f) -> f32 { return textureLoad(hold, vec2i(clamp(floor(p), vec2f(0.0), u.res.xy - 1.0)), 0).r; }
/// How much of the pixel is within `r0` of text to read (its island, held exactly: 1 inside, a
/// one-pixel edge).
fn core_at(p: vec2f) -> f32 { return textureLoad(hold, vec2i(clamp(floor(p), vec2f(0.0), u.res.xy - 1.0)), 0).g; }
/// The most looseness a hold of `h` allows.
fn hold_cap(h: f32) -> f32 { return mix(1.0, u.burst_t.w, h); }
/// The hold where the pixel being shaded is (`shade`): a material that takes a mark's looseness from
/// somewhere else (a cell's centre) still draws this pixel no looser than its own hold allows.
var<private> hold_here: f32 = 1.0;
fn loose_t(p: vec2f) -> f32 {
  let k = dec_loose(near_t(p).b);
  let h = hold_at(p);
  if (h <= 0.0 && hold_here >= 1.0) { return k; }
  return min(min(k, hold_cap_at(p)), hold_here);
}
/// The most looseness the hold at `p` allows: nothing within the island (`core_at`), up to the
/// legibility cap beyond it, falling off to none by r1.
fn hold_cap_at(p: vec2f) -> f32 { return hold_cap(hold_at(p)) * (1.0 - core_at(p)); }
fn ink(p: vec2f) -> vec4f { return textureSampleLevel(tex0, samp, p / u.res.xy, 0.0); }
fn near(p: vec2f) -> vec4f { return textureSampleLevel(tex1, samp, p / u.res.xy, 0.0); }
// Exact texels (no filtering), for averaging cells and for pitch 1 to be the image itself.
fn ink_t(p: vec2f) -> vec4f { return textureLoad(tex0, vec2i(clamp(floor(p), vec2f(0.0), u.res.xy - 1.0)), 0); }
fn near_t(p: vec2f) -> vec4f { return textureLoad(tex1, vec2i(clamp(floor(p), vec2f(0.0), u.res.xy - 1.0)), 0); }
fn unit(d: vec2f, r: f32) -> f32 {
  if (u.fc.w < 0.5) { return smoothstep(r + 0.75, r - 0.75, length(d)); }
  return smoothstep(r + 0.5, r - 0.5, max(abs(d.x), abs(d.y)));
}

// ── Pictures into marks (DESIGN.md §1.6.1) ────────────────────────────────────────────────────────
const SETS: f32 = 20.0;
const CHAR_MIN: f32 = 4.0;
const CHAR_MAX: f32 = 40.0;
/// A character tile read texel for texel at the pitch it was cut for (no resampling, so it stays sharp).
fn char_mark(col: f32, uv: vec2f, P: f32) -> f32 {
  let row = (P * (P - 1.0) - CHAR_MIN * (CHAR_MIN - 1.0)) * 0.5;
  let t = min(floor(uv * P), vec2f(P - 1.0));
  return textureLoad(chars, vec2i(i32(col * P + t.x), i32(row + t.y)), 0).r;
}
fn pv(slot: i32, i: i32) -> vec4f { return u.pics[slot * 7 + i]; }
fn lum(c: vec3f) -> f32 { return dot(c, vec3f(0.299, 0.587, 0.114)); }
fn straight(c: vec4f) -> vec3f { return c.rgb / max(c.a, 1e-3); }
/// 8 × 8 Bayer threshold in (0, 1), the same bit-interleaving as marks.rs `bayer8`.
fn bayer8(cell: vec2f) -> f32 {
  let x = u32(i32(cell.x) & 7); let y = u32(i32(cell.y) & 7);
  let a = x ^ y;
  let v = ((a & 1u) << 5u) | ((y & 1u) << 4u) | ((a & 2u) << 2u) | ((y & 2u) << 1u) | ((a & 4u) >> 1u) | ((y & 4u) >> 2u);
  return (f32(v) + 0.5) / 64.0;
}
// ── Scrolling (DESIGN.md §1.8.3) ─────────────────────────────────────────────────────────────────
/// Inside a scroll node the lattices of the materials are fixed to the content: `anc` is the content's
/// offset here (whole device px), and every lattice and noise is taken at `p + anc`. Set once per pixel
/// by `shade_new`.
var<private> anc: vec2f;
/// The scroll trace here: its strength, the way the content moves on screen (a unit vector), the axis.
struct Sc { o: vec2f, k: f32, d: vec2f };
fn scroll_here(p: vec2f) -> Sc {
  var r: Sc; r.o = vec2f(0.0); r.k = 0.0; r.d = vec2f(0.0, 1.0);
  for (var j = 0; j < 8; j++) {
    let b = u.scrolls[j];
    if (b.z <= b.x || p.x < b.x || p.y < b.y || p.x >= b.z || p.y >= b.w) { continue; }
    let o = u.scrollo[j];
    r.o += o.xy;
    if (o.z > r.k) {
      r.k = o.z;
      r.d = select(vec2f(0.0, sign(o.w)), vec2f(sign(o.w), 0.0), abs(o.w) > 1.5);
    }
  }
  return r;
}
/// Rows of content held to a lattice of `P` while it moves: where to read so that the content shows at
/// its offset rounded to `P` along the axis it moves on.
fn latch(p: vec2f, sc: Sc, P: f32) -> vec2f {
  if (sc.k <= 0.02) { return p; }
  let ax = abs(sc.d);
  let o = dot(sc.o, ax);
  return p + ax * (round(o / P) * P - o);
}
fn lat(p: vec2f) -> vec2f { return p + anc; }

/// Average of the ink in one cell (premultiplied), from 4 texels, or 9 when cells are large. Cells are
/// on the lattice fixed to the content (`lat`).
fn cell_avg(cell: vec2f, P: f32) -> vec4f {
  let n = select(2, 3, P > 16.0);
  var acc = vec4f(0.0);
  for (var j = 0; j < n; j++) {
    for (var i = 0; i < n; i++) {
      acc += ink_t(floor((cell + (vec2f(f32(i), f32(j)) + 0.5) / f32(n)) * P - anc));
    }
  }
  return acc / f32(n * n);
}
fn nearest_colour(c: vec3f, row: f32, count: f32) -> vec3f {
  var best = c; var bd = 1e9;
  for (var i = 0; i < 16; i++) {
    if (f32(i) >= count) { break; }
    let q = textureLoad(palettes, vec2i(i, i32(row)), 0).rgb;
    let d = dot(c - q, c - q);
    if (d < bd) { bd = d; best = q; }
  }
  return best;
}
/// Mark (coverage, height) of set `s`, atlas column `col`, at tile coordinate `uv`, read at mip `lod`
/// and kept half a texel inside the tile so neighbouring tiles never bleed in.
fn mark(s: f32, col: f32, uv: vec2f, lod: f32) -> vec2f {
  let h = 0.5 * exp2(max(lod, 0.0)) / 32.0;
  let q = clamp(uv, vec2f(h), vec2f(1.0 - h));
  return textureSampleLevel(marks, samp, vec2f((col + q.x) / 16.0, (s + q.y) / SETS), lod).rg;
}
/// The picture's own looseness at a point: the field (fronts, half-plane), the lens, and the
/// instance's own (held, born, set at rest).
// A picture to read (a picture in a control: legible, so its instances carry the push code 2) is marked
// in near.g a little below its slot's step; the lens loosens it at most to the legibility cap.
fn pic_k(q: vec2f) -> f32 {
  // The lens reaches a picture through its own looseness (stage 1), as the fronts do.
  return clamp(half_plane(q) + loose_t(q) + u.rip.y, 0.0, u.rip.x);
}
/// Step 2: the pitch grows from 1 to `pitch` over the first half of loosening.
fn pic_p(k: f32, pitch: f32) -> f32 {
  return max(1.0, round(mix(1.0, max(1.0, round(pitch * u.res.z)), smoothstep(0.0, 0.5, k))));
}

fn picture(frag: vec2f, slot: i32) -> vec3f {
  let a0 = pv(slot, 0); let a1 = pv(slot, 1); let a2 = pv(slot, 2); let a3 = pv(slot, 3);
  let pitch = clamp(a0.x, 1.0, 64.0);
  let levels = clamp(round(a0.y), 2.0, 16.0);
  // Looseness is read at the centre of the cell the point falls in, so a cell is all one pitch.
  let p0 = pic_p(pic_k(frag), pitch);
  let k = pic_k((floor(lat(frag) / p0) + 0.5) * p0 - anc);
  // 1. Not loose: the picture itself.
  if (k <= 0.0) { return straight(ink_t(frag)); }
  let P = pic_p(k, pitch);
  // A cell of one pixel cannot hold a mark: marks, palette and parallax all come in from P = 1 to 2,
  // so pitch 1 is the picture at any looseness.
  let w = clamp(P - 1.0, 0.0, 1.0);
  var p = frag;
  if (a2.x > 0.0 && u.pointer.z > 0.0) {
    // Parallax: bright parts move further toward the pointer.
    let dir = normalize(u.pointer.xy - frag + vec2f(1e-3));
    p += dir * lum(straight(ink_t(frag))) * a2.x * u.res.z * w;
  }
  // 2–3. One lattice for the whole screen; the cell's colour is the average under it.
  let cell = floor(lat(p) / P);
  var cc = cell_avg(cell, P);
  if (cc.a < 0.5) { cc = ink_t(frag); }
  let c = straight(cc);
  let b = bayer8(cell);
  // Limited colour: the nearest palette colour after a dithered nudge.
  var cu = c;
  if (a2.w >= 1.0) { cu = mix(c, nearest_colour(c + (b - 0.5) * a1.z, clamp(a2.z, 0.0, u.fd.w - 1.0), min(a2.w, 16.0)), w); }
  // 4. Brightness into levels, dithered across the hand-over by `spread`.
  let L = pow(clamp(lum(c), 0.0, 1.0), clamp(a1.y, 0.3, 3.0));
  let n = clamp(floor(L * levels + (b - 0.5) * a1.z), 0.0, levels - 1.0);
  // Regions: a coarser lattice picks the set, by hash or by its own brightness band.
  var ms = a0.z;
  if (a3.y >= 1.0 && a3.w >= 1.0) {
    let rp = P * round(a3.y);
    let big = floor(lat(p) / rp);
    let cnt = min(a3.w, 4.0);
    var idx = floor(h1(big + 3.7) * cnt);
    if (a3.z >= 0.5) { idx = floor(clamp(lum(straight(cell_avg(big, rp))), 0.0, 0.999) * cnt); }
    ms = pv(slot, 6)[i32(clamp(idx, 0.0, 3.0))];
  }
  ms = clamp(round(ms), 0.0, SETS - 1.0);
  // 5. Inside the cell; marks turn to lie along the contours (the gradient turned a quarter).
  var uv = fract(lat(p) / P);
  if (a1.w > 0.0) {
    // Central differences of the neighbouring cells' averages, so the direction flows smoothly; where
    // the picture is flat the marks stay unturned rather than pointing at noise.
    let gx = lum(straight(cell_avg(cell + vec2f(1.0, 0.0), P))) - lum(straight(cell_avg(cell - vec2f(1.0, 0.0), P)));
    let gy = lum(straight(cell_avg(cell + vec2f(0.0, 1.0), P))) - lum(straight(cell_avg(cell - vec2f(0.0, 1.0), P)));
    let gm = abs(gx) + abs(gy);
    if (gm > 1e-3) {
      let th = (atan2(gy, gx) + 1.5707963) * a1.w * smoothstep(0.004, 0.03, gm);
      let cs = cos(th); let sn = sin(th);
      let d = uv - 0.5;
      uv = fract(vec2f(cs * d.x + sn * d.y, -sn * d.x + cs * d.y) + 0.5);
    }
  }
  // 6. The mark for this level.
  let col = round(n * 15.0 / (levels - 1.0));
  let lod = max(0.0, log2(32.0 / P));
  var m = mark(ms, col, uv, lod).x;
  let crisp = ms == 0.0 && P >= CHAR_MIN && P <= CHAR_MAX;
  if (crisp) { m = char_mark(col, uv, P); }
  // 7. Ink and ground colours. The page's ground colour is not among them.
  let mode = round(a0.w);
  var inkc = cu;
  // Characters carry the tone by which glyph is drawn; ink at the cell's own darkness would count the
  // darkness twice and dark pictures would vanish. So their original colour is lifted to full value.
  if (ms == 0.0 && mode == 0.0) { inkc = cu / max(max(cu.r, max(cu.g, cu.b)), 0.04); }
  if (mode == 1.0) { inkc = u.fg.rgb; } else if (mode == 2.0) { inkc = mix(pv(slot, 4).rgb, pv(slot, 5).rgb, L); } else if (mode == 3.0) { inkc = pv(slot, 4).rgb; }
  let g = cu * clamp(a1.x, 0.0, 1.0);
  // Relief: the height channel's slope, lit from the upper left.
  var shade = 1.0;
  if (a3.x > 0.0) {
    let e = exp2(lod) / 32.0;
    let hx = mark(ms, col, uv + vec2f(e, 0.0), lod).y - mark(ms, col, uv - vec2f(e, 0.0), lod).y;
    let hy = mark(ms, col, uv + vec2f(0.0, e), lod).y - mark(ms, col, uv - vec2f(0.0, e), lod).y;
    shade = clamp(1.0 + a3.x * (hx + hy), 0.15, 2.0);
  }
  // 8. Marks over the cell's ground, plus a glow: light from this cell and its eight neighbours,
  // each by its own brightness, falling off with distance, so bright marks bloom across their gaps.
  var glow = vec3f(0.0);
  if (a2.y > 0.0) {
    for (var j = -1; j <= 1; j++) {
      for (var i = -1; i <= 1; i++) {
        let nb = cell + vec2f(f32(i), f32(j));
        let nc = straight(ink_t((nb + 0.5) * P - anc));
        let nl = pow(clamp(lum(nc), 0.0, 1.0), clamp(a1.y, 0.3, 3.0));
        let d = (lat(p) - (nb + 0.5) * P) / (P * (0.35 + 0.5 * nl));
        var gc = nc;
        if (mode == 1.0) { gc = u.fg.rgb; } else if (mode == 2.0) { gc = mix(pv(slot, 4).rgb, pv(slot, 5).rgb, nl); } else if (mode == 3.0) { gc = pv(slot, 4).rgb; }
        glow += gc * nl * nl * exp(-dot(d, d) * 2.0);
      }
    }
  }
  let r = mix(g, inkc * shade, m) + glow * a2.y * 0.5 * w;
  // 9. First half only coarsens; the marks come up in the second half.
  return mix(cu, r, smoothstep(0.5, 1.0, k) * w);
}

/// Whether the page's ink is lighter than its ground (a dark theme).
fn ink_light() -> bool { return lum(u.fg.rgb) > lum(gnd(vec2f(0.0))); }
/// A picture under `halftonr` past half loose: a rotated screen whose dots follow the cell's
/// brightness (bright cells big on a dark ground, dark cells big on a light one), from a quarter of the
/// pitch to nearly all of it, in the cell's own colour.
fn pic_halftonr(p: vec2f, slot: i32) -> vec3f {
  let base = picture(p, slot);
  let k = pic_k(p);
  if (k <= 0.5) { return base; }
  let a0 = mp(HALFTONR, 0);
  let pitch = max(3.0, a0.x * u.res.z * 1.5); let ang = radians(a0.y);
  let q = rot(lat(p), ang);
  let cell = floor(q / pitch);
  let c = rot((cell + 0.5) * pitch, -ang) - anc;
  let e = pitch * 0.25;
  let avg = ink_t(c + vec2f(-e, -e)) + ink_t(c + vec2f(e, -e)) + ink_t(c + vec2f(-e, e)) + ink_t(c + vec2f(e, e));
  let col = straight(avg);
  let l = clamp(lum(col), 0.0, 1.0);
  // Stretched over the tones pictures mostly have, so a picture shows its whole range of sizes.
  let size = smoothstep(0.08, 0.75, select(1.0 - l, l, ink_light()));
  let r = pitch * mix(0.25, 0.95, size) * 0.5;
  let m = smoothstep(r + 0.6, r - 0.6, length(q - (cell + 0.5) * pitch));
  return mix(base, mix(gnd(p), col, m), smoothstep(0.5, 1.0, k));
}
/// A picture under `charr` past half loose: square cells of the title's size, each a character from
/// the character set by the cell's brightness (denser where brighter on a dark ground, where darker on
/// a light one), in the cell's average colour over a faint tile of it.
fn pic_charr(p: vec2f, slot: i32) -> vec3f {
  let base = picture(p, slot);
  let k = pic_k(p);
  if (k <= 0.5) { return base; }
  let a0 = mp(CHARR, 0);
  let P0 = clamp(round(a0.x * u.res.z), CHAR_MIN, CHAR_MAX);
  let P = select(P0, min(CHAR_MAX, round(P0 * 1.5)), k >= 0.7);
  let levels = clamp(round(a0.y), 2.0, 16.0);
  let cell = floor(lat(p) / P);
  let cc = straight(cell_avg(cell, P));
  let l = clamp(lum(cc), 0.0, 1.0);
  let dense = select(1.0 - l, l, ink_light());
  let n = clamp(floor(dense * levels), 0.0, levels - 1.0);
  let m = char_mark(round(n * 15.0 / (levels - 1.0)), fract(lat(p) / P), P);
  // On a dark ground the character carries the tone, so its colour is lifted to full value.
  let ink = select(cc, cc / max(max(cc.r, max(cc.g, cc.b)), 0.04), ink_light());
  let tile = mix(gnd(p), cc, 0.22);
  return mix(base, mix(tile, ink, m), smoothstep(0.5, 1.0, k));
}

// ── Materials (DESIGN.md §1.6) ───────────────────────────────────────────────────────────────────
fn vnoise(p: vec2f) -> f32 {
  let i = floor(p); let f = fract(p); let w = f * f * (3.0 - 2.0 * f);
  return mix(mix(h1(i), h1(i + vec2f(1.0, 0.0)), w.x), mix(h1(i + vec2f(0.0, 1.0)), h1(i + vec2f(1.0, 1.0)), w.x), w.y);
}
fn rot(p: vec2f, a: f32) -> vec2f { let c = cos(a); let s = sin(a); return vec2f(c * p.x - s * p.y, s * p.x + c * p.y); }
/// The page ground here (a gradient from top to bottom when the theme has one).
fn gnd(p: vec2f) -> vec3f {
  if (all(u.ground2.rgb == u.bg.rgb)) { return u.bg.rgb; }
  return mix(u.bg.rgb, u.ground2.rgb, clamp(p.y / u.res.y, 0.0, 1.0));
}

/// Text and shape ink, decoded: straight colour and coverage, whether it is a shape or has a glyph in it,
/// its accent, tone and hue (as `hue_col` takes it).
struct Ink { rgb: vec3f, a: f32, shape: bool, acc: f32, tone: f32, txt: bool, hue: f32 };
fn dec_at(c: vec4f, p: vec2f) -> Ink {
  var o: Ink;
  o.a = c.a; o.shape = false; o.rgb = gnd(p); o.acc = 0.0; o.tone = 0.0; o.txt = false; o.hue = 0.0;
  if (c.a < 1e-3) { return o; }
  let t = c.r / c.a; let g = c.g / c.a; let b = c.b / c.a;
  let code = clamp(round(b / 0.15), 0.0, 3.0);
  let hue = code_hue(code);
  let rest = b - code * 0.15;
  o.shape = rest > 0.025;
  // Any glyph in the pixel, even a faint edge over a shape: a pure shape's rest is 0.05.
  o.txt = rest < 0.049;
  o.acc = g; o.tone = t;
  o.rgb = paint_rgb(t, g, hue, p);
  o.hue = hue;
  return o;
}
fn dec(p: vec2f) -> Ink { return dec_at(ink_t(p), p); }
/// The exact picture of text and shapes over the ground: what every material shows at rest.
fn exact(p: vec2f) -> vec3f { let d = dec(p); return mix(gnd(p), d.rgb, d.a); }
/// Signed distance to the nearest outline (device px, positive inside), up to SPREAD either way.
fn sdist(p: vec2f) -> f32 { return (near_t(p).r - 0.5) * 2.0 * u.fd.z; }
/// `sdist` filtered between texel centres (`near`), for a distance at any point.
fn sdist_f(p: vec2f) -> f32 { return (near(p).r - 0.5) * 2.0 * u.fd.z; }
/// The least a loose cell with ink in it is drawn with (DESIGN.md §1.6): a cell of a mark material that
/// some ink at least half covers a pixel of, sampled below this, is drawn as if sampled at this, so a thin
/// line is a row of the least marks and not gone; a cell sampled at this or more is as it was.
const INK_FLOOR: f32 = 0.25;
/// The ink in the cell centred at `c`, `h` from its centre to each side (text or a shape, not a picture):
/// the centre of the pixel of ink nearest the centre (half a pixel past the outline, covered at least
/// half) when it is one of the cell's pixels (the cell's top and left edges in it, its bottom and right ones
/// not), in `xy` with `z` 1; `z` 0 when there is none. So a line of whole pixels lights the one row
/// (column) of cells it is in, and a line on the edge between two goes to the lower (right) one. A cell
/// larger than the nearness reaches is looked at as up to 4 x 4 smaller ones.
fn cell_ink(c: vec2f, h: f32) -> vec3f {
  if (NO_INK_FLOOR) { return vec3f(0.0); }
  let n = clamp(ceil((h * 1.415 + 1.0) / (u.fd.z - 1.0)), 1.0, 4.0);
  let hs = h / n;
  for (var j = 0; j < 4; j++) {
    if (f32(j) >= n) { break; }
    for (var i = 0; i < 4; i++) {
      if (f32(i) >= n) { break; }
      let q = c - vec2f(h) + (vec2f(f32(i), f32(j)) * 2.0 + 1.0) * hs;
      let d = sdist_f(q);
      if (d < 0.5 - u.fd.z) { continue; }
      let g = sgrad(q);
      let at = floor(select(q, q + g / max(length(g), 1e-3) * (0.5 - d), d < 0.5)) + 0.5;
      if (ink_t(at).a >= 0.5 && near_t(at).g <= 0.05 && all(at >= c - vec2f(h)) && all(at < c + vec2f(h))) { return vec3f(at, 1.0); }
    }
  }
  return vec3f(0.0);
}
fn sgrad(p: vec2f) -> vec2f {
  return vec2f(sdist(p + vec2f(1.0, 0.0)) - sdist(p - vec2f(1.0, 0.0)), sdist(p + vec2f(0.0, 1.0)) - sdist(p - vec2f(0.0, 1.0))) * 0.5;
}
/// The ink at the nearest outline (stepping inward along the distance's gradient), for colouring what
/// a material draws just outside the exact ink.
fn ink_inward(p: vec2f) -> Ink {
  let d = sdist(p);
  if (d > 0.5) { return dec(p); }
  let g = sgrad(p);
  let n = g / max(length(g), 1e-3);
  let q = p + n * (1.5 - d);
  // A picture's ink is its own colour, not text's: nothing to extend from there.
  if (near_t(q).g > 0.05) { var o = dec(vec2f(-10.0)); o.a = 0.0; o.shape = false; return o; }
  return dec(q);
}
fn burst_k(p: vec2f) -> f32 {
  let age = u.res.w - u.burst_t.x;
  let r = u.burst;
  if (u.burst_t.y <= 0.0 || age < 0.0 || age > u.burst_t.z || p.x < r.x || p.y < r.y || p.x >= r.z || p.y >= r.w) { return 0.0; }
  return u.burst_t.y * (1.0 - age / u.burst_t.z);
}
/// How loose text and shapes are here: fronts and their tails, the half-plane, and each instance's
/// own looseness, scaled by the style.
fn kt(p: vec2f) -> f32 { return clamp((half_plane(p) + loose_t(p) + u.rip.y) * loose_scale(), 0.0, u.rip.x); }
/// `kt`, only where something is really loose: text held at the legibility cap (0.15) keeps the
/// material's resting look, and the loosened look is all there by 0.26, so a quarter loose is already
/// on its way.
fn kt_loose(p: vec2f) -> f32 { return kt(p) * smoothstep(0.18, 0.26, half_plane(p) + loose_t(p) + u.rip.y); }
/// `kt_loose` by the ink's own looseness, before any hold of text to read: for a material that keeps
/// the held island itself (`core_at`) and decides whole cells around it.
fn kt_loose_own(p: vec2f) -> f32 {
  let k = dec_loose(near_t(p).b);
  return clamp((half_plane(p) + k + u.rip.y) * loose_scale(), 0.0, u.rip.x) * smoothstep(0.18, 0.26, half_plane(p) + k + u.rip.y);
}
/// Whatever is drawn at a point, pictures as their marks: for materials that move the picture around.
fn scene_at(q: vec2f) -> vec3f {
  let n = near_t(q);
  if (n.g > 0.05) { return picture(q, clamp(i32(round(n.g * 8.0)) - 1, 0, 7)); }
  return exact(q);
}

// ── Materials (DESIGN.md §1.6): src/materials/<name>.wgsl, in id order. Each begins with its name and its
// numbers (`style::MAT_PARAMS`) and names its own id `SELF`. ────────────────────────────────────────

/// How much of what is at `q` leaves a scroll trace: text, and shapes drawn in the ink itself (rules,
/// strokes); not panels, fills in the ground's tone, accent planes or pictures, which would smear whole
/// areas.
fn trace_w(e: Ink, q: vec2f) -> f32 {
  if (near_t(q).g > 0.05) { return 0.0; }
  return e.a * select(smoothstep(0.55, 0.9, e.tone), 1.0, e.txt);
}
/// The scroll trace over a material's colour at `p` (DESIGN.md §1.8.3): whole lines as one, behind the
/// motion, never over the ink itself, so text stays exactly what it is. `q` is where the material read
/// (a latched or slipped point), `m` the material.
fn scroll_trace(col: vec3f, q: vec2f, m: i32, sc: Sc) -> vec3f {
  let k = sc.k;
  if (k <= 0.0) { return col; }
  let s = u.res.z;
  let own = dec(q);
  if (own.a > 0.5) { return col; }
  let d = sc.d;
  var c = col;
  if (m == DOTTR) {
    // A phosphor trail: the rows the content has just left glow on as dots of the lattice, fading.
    let pitch = max(1.5, u.fa.x * s);
    let cell = floor(lat(q) / pitch);
    let centre = (cell + 0.5) * pitch - anc;
    let n = max(1.0, round(u.scrp.x * k / pitch));
    var best = 0.0; var bc = vec3f(0.0);
    for (var j = 1; j <= 6; j++) {
      if (f32(j) > n) { break; }
      let e = pitch * 0.25;
      let at = centre + d * f32(j) * pitch;
      let a = (ink(at + vec2f(-e, -e)) + ink(at + vec2f(e, -e)) + ink(at + vec2f(-e, e)) + ink(at + vec2f(e, e))) * 0.25;
      let da = dec_at(a, at);
      let w = trace_w(da, at) * (1.0 - (f32(j) - 0.5) / (n + 0.5));
      if (w > best) { best = w; bc = da.rgb; }
    }
    let r = pitch * 0.34 * sqrt(best);
    let dotm = unit(q - centre, r) * step(0.12, best);
    return mix(c, mix(bc, u.accent.rgb, 0.35), dotm * 0.55 * k);
  }
  if (m == CHARR) {
    // A trail of light characters on the character grid.
    let a0 = mp(CHARR, 0);
    let P = clamp(round(a0.x * s), CHAR_MIN, CHAR_MAX);
    let cell = floor(lat(q) / P);
    let centre = (cell + 0.5) * P - anc;
    let n = max(1.0, round(u.scrp.x * k / P));
    var best = 0.0;
    for (var j = 1; j <= 5; j++) {
      if (f32(j) > n) { break; }
      let at = (cell + d * f32(j) + 0.5) * P - anc;
      let a = cell_avg(cell + d * f32(j), P);
      best = max(best, trace_w(dec_at(a, at), at) * (1.0 - (f32(j) - 0.5) / (n + 0.5)));
    }
    let lv = clamp(floor(best * 7.0), 0.0, 6.0);
    if (lv < 1.0) { return c; }
    let mk = char_mark(lv, fract(lat(q) / P), P);
    return mix(c, mix(gnd(q), u.fg.rgb, 0.55), mk * k);
  }
  if (m == HALFTONR || m == SMUDGR) {
    // The second colour out of register along the motion: a screened ghost of the key ink, behind it.
    let off = d * u.scrp.y * k;
    let at = q + off;
    if (near_t(at).g > 0.05) { return c; }
    if (m == HALFTONR) {
      let pitch = max(2.0, mp(HALFTONR, 0).x * s * 0.5);
      let ang = radians(mp(HALFTONR, 0).y + 30.0);
      let rq = rot(lat(at), ang);
      let cell = floor(rq / pitch);
      let cc = rot((cell + 0.5) * pitch, -ang) - anc;
      let e = pitch * 0.3;
      let a = min(1.2 * 0.25 * (trace_w(dec(cc + vec2f(-e, -e)), cc) + trace_w(dec(cc + vec2f(e, -e)), cc) + trace_w(dec(cc + vec2f(-e, e)), cc) + trace_w(dec(cc + vec2f(e, e)), cc)), 1.0);
      let r = pitch * 0.5 * sqrt(a);
      let dm = smoothstep(r + 0.5, r - 0.5, length(rq - (cell + 0.5) * pitch)) * step(0.08, a);
      return mix(c, u.accent.rgb, dm * 0.6 * k);
    }
    let a = trace_w(dec(at), at);
    // On paper the second plate prints over (multiplies); on a dark ground it shows as its own colour.
    if (lum(gnd(q)) > 0.5) { return mix(c, c * mix(vec3f(1.0), u.accent.rgb / max(gnd(q), vec3f(0.05)), 0.7), a * k); }
    return mix(c, u.accent.rgb, a * k * 0.5);
  }
  if (m == CARVR) {
    // A carved echo: broken lines of ink behind each edge, ink or ground.
    let L = u.scrp.x * k;
    var hit = 0.0;
    for (var j = 1; j <= 4; j++) {
      let t = f32(j) / 4.0;
      hit = max(hit, step(0.5, trace_w(dec(q + d * L * t), q + d * L * t)) * step(fract(dot(lat(q), d) / (3.0 * s) + t * 0.5), 0.45) * step(0.5 * t, vnoise(lat(q) * vec2f(0.05, 0.3) / s)));
    }
    return mix(c, u.fg.rgb, hit);
  }
  if (m == GLITCHR || m == NEAR) { return c; }
  // plain: a soft trailing edge, at most.
  let L = u.scrp.x * k * 0.45;
  var a = 0.0; var tc = vec3f(0.0);
  for (var j = 1; j <= 3; j++) {
    let t = f32(j) / 3.0;
    let e = dec(q + d * L * t);
    let w = trace_w(e, q + d * L * t) * (1.0 - t * 0.85) * 0.14;
    if (w > a) { a = w; tc = e.rgb; }
  }
  return mix(c, tc, a * k);
}

/// The final colour of the pixel centred at `p` (device px of the full-resolution frame).
fn shade_new(pf: vec2f) -> vec3f {
  let s = u.res.z; let t = u.res.w;
  // Inside a scroll node: the lattices follow the content; moving fast, dottr and charr hold their rows
  // to their lattice and glitchr slips a band now and then (DESIGN.md §1.8.3).
  let sc = scroll_here(pf);
  anc = sc.o;
  var p = pf;
  let m0 = eff_mat(near_t(pf).a * 255.0);
  if (sc.k > 0.0) {
    if (m0 == DOTTR) { p = latch(pf, sc, max(1.5, u.fa.x * s)); }
    else if (m0 == CHARR) { p = latch(pf, sc, clamp(round(mp(CHARR, 0).x * s), CHAR_MIN, CHAR_MAX)); }
    else if (m0 == GLITCHR) {
      // One band at a time, no more often than every `slip_ms`, and only as fast as the motion asks.
      let tq = floor(t / u.scrq.x);
      let ax = abs(sc.d);
      let across = vec2f(ax.y, ax.x);
      let bh = u.scrp.w;
      let band = floor(dot(pf, ax) / bh);
      let pick = floor(h1(vec2f(tq, 1.7)) * dot(u.res.xy, ax) / bh);
      if (band == pick && h1(vec2f(tq, 4.1)) < sc.k) { p = pf + across * (h1(vec2f(tq, 8.3)) - 0.5) * 2.0 * u.scrp.z * sc.k; }
    }
  }
  let here = near_t(p);
  let is_pic = here.g > 0.05;
  let m_here = eff_mat(here.a * 255.0);
  // In a pass for one material of several: another's pixel is that one's pass's to draw.
  if (ONLY && m_here != MAT) { discard; }
  let m = select(m_here, MAT, MAT >= 0);
  var col: vec3f;
  if (is_pic && m != GLITCHR) {
    // A picture is drawn from its own colours only (DESIGN.md §1.6.1), in every material.
    let slot = clamp(i32(round(here.g * 8.0)) - 1, 0, 7);
    var q = p;
    if (m == DOTTR) {
      // Glitchr slip of dottr: a slip that would leave the picture keeps it in place.
      let bh = 5.0 * s; let band = floor(p.y / bh);
      let w0 = wave(vec2f(p.x, (band + 0.5) * bh), 0.6);
      let pick = step(0.55, h1(vec2f(band, floor(t * 9.0))));
      q.x += (h1(vec2f(band, floor(t * 17.0))) - 0.5) * 70.0 * s * u.fa.w * pick * smoothstep(0.2, 0.9, w0);
      q = select(p, q, near_t(q).g > 0.05);
    }
    // Each material its own picture past half loose (DESIGN.md §1.6.2): halftonr's screen, charr's
    // characters; the rest keep the marks.
    if (m == HALFTONR) { col = pic_halftonr(q, slot); } else if (m == CHARR) { col = pic_charr(q, slot); } else { col = picture(q, slot); }
    // Text to read drawn over a picture is in the picture's terms here (`ink_of`): its island is shown
    // as it is, however loose the picture beneath.
    if (core_at(p) >= 0.5) { col = straight(ink_t(p)); }
    // The compare wipe's line.
    let wx = floor(u.wipe.x);
    let r = u.wipe_rect;
    if (u.wipe.y > 0.0 && p.x >= wx && p.x < wx + max(1.0, round(s)) && p.y >= r.y && p.y < r.w) { col = u.accent.rgb; }
  } else {
    switch m {
      case 1: { col = m_dottr(p); }
      case 2: { col = m_halftonr(p); }
      case 3: { col = m_charr(p); }
      case 4: { col = m_smudgr(p); }
      case 5: { col = m_carvr(p); }
      case 6: { col = m_glitchr(p, is_pic); }
      case 7: { col = m_near(p); }
      case 8: { col = m_scribblr(p); }
      default: { col = m_plain(p); }
    }
    col = scroll_trace(col, p, m, sc);
  }
  // Black and white only (post.binary), where the ink is drawn as it is (plain, scribblr): text, lines
  // and shapes are ink where they cover at least half the pixel, however light their tone; pictures are
  // a two-level ordered dither of their light. The finishing pass's palette then takes the two colours.
  if (u.post[4].y > 0.5 && (m == PLAIN || m == SCRIBBLR)) { col = binary_px(p, col, is_pic); }
  if (u.fd.y > 0.5) { col = pow(col, vec3f(2.2)); }
  return col;
}
fn binary_px(p: vec2f, col: vec3f, is_pic: bool) -> vec3f {
  let g = gnd(p);
  let ink = u.fg.rgb;
  let lum = vec3f(0.30, 0.59, 0.11);
  if (is_pic) {
    // The share of the way from the ground to the ink, lifted (square root) so a dark picture on a dark
    // ground keeps its middle tones as dots.
    let f = (dot(col, lum) - dot(g, lum)) / select(dot(ink, lum) - dot(g, lum), 1e-3, abs(dot(ink, lum) - dot(g, lum)) < 1e-3);
    return select(g, ink, sqrt(clamp(f, 0.0, 1.0)) > bayer_t(p, 4u, 2u));
  }
  let d = dec(p);
  // Faint fills (a panel in the ground's tone) stay ground.
  let tone = length(d.rgb - g) / max(length(ink - g), 1e-3);
  return select(g, ink, d.a >= 0.5 && tone > 0.2);
}

/// The final colour: the new frame, or during a commit the old one beyond the front travelling out from
/// the press, or the preset's transition program between them (DESIGN.md §4.1).
// `shade_new` is called from one place only: it holds every material, and each call site is compiled in
// full (several made the material pipeline too big to build on a CPU rasteriser).
fn shade(p: vec2f) -> vec3f {
  hold_here = hold_cap_at(p);
  var s: TS;
  s.m = 1.0; s.offn = vec2f(0.0); s.offo = vec2f(0.0); s.tint = vec4f(0.0); s.fade = 0.0; s.split = 0.0; s.toner = 0.0;
  if (u.split.w > 0.5) {
    // A commit within a scope: outside its box the frame stays as it was.
    if (u.sbox.z > u.sbox.x && (p.x < u.sbox.x || p.y < u.sbox.y || p.x >= u.sbox.z || p.y >= u.sbox.w)) { return old_at(p); }
    if (u.tr[0].x <= 0.0) {
      let r = (u.res.w - u.split.z) * u.fb.x * u.res.z;
      if (distance(p, u.split.xy) > r) { return old_at(p); }
    } else {
      let x = clamp((u.res.w - u.tr[0].w) / max(u.tr[0].x, 1e-4), 0.0, 1.0);
      // The last frame of a transition is the new frame, exactly.
      if (x < 1.0) { s = tr_eval(p, x); }
    }
  }
  if (s.m <= 0.0 && s.tint.a <= 0.0 && s.fade <= 0.0 && s.split <= 0.0) { return old_at(p + s.offo); }
  // Only where the new frame shows is it shaded.
  var nw = vec3f(0.0);
  if (s.m > 0.0) { nw = clamp(shade_new(p + s.offn) + s.toner, vec3f(0.0), vec3f(1.0)); }
  if (s.m >= 1.0 && s.tint.a <= 0.0) { return nw; }
  let qo = p + s.offo;
  var od = old_at(qo);
  if (s.split > 0.0) { let d = vec2f(s.split, 0.0); od = vec3f(old_at(qo + d).r, od.g, old_at(qo - d).b); }
  od = mix(od, enc(gnd(p)), clamp(s.fade, 0.0, 1.0));
  var col = mix(od, nw, clamp(s.m, 0.0, 1.0));
  return mix(col, enc(s.tint.rgb), clamp(s.tint.a, 0.0, 1.0));
}
/// The frame before the commit (already encoded for the surface).
fn old_at(p: vec2f) -> vec3f { return textureLoad(before, vec2i(clamp(floor(p), vec2f(0.0), u.res.xy - 1.0)), 0).rgb; }
fn enc(c: vec3f) -> vec3f { return select(c, pow(c, vec3f(2.2)), u.fd.y > 0.5); }

// ── Transitions (DESIGN.md §4.1) ──────────────────────────────────────────────────────────────────
// A program of up to four steps, each a kind and five numbers over a window of the transition's time.
// What they build: how much of the new frame shows here (m), where each frame is sampled from, a tint
// laid over (planes, a light bar, box lines), how far the old frame has faded to the ground, and a
// channel split. Kinds: 1 reveal, 2 bands, 3 fade, 4 bar, 5 planes, 6 loosen (in kt, CPU), 7 layers,
// 8 boxes, 9 carve, 10 outline, 11 crack (preset.rs StepKind).
struct TS { m: f32, offn: vec2f, offo: vec2f, tint: vec4f, fade: f32, split: f32, toner: f32 };
fn dir_of(deg: f32) -> vec2f { let a = radians(deg); return vec2f(cos(a), sin(a)); }
/// How far along a line at `deg` the point is, 0 at the window's first corner to 1 at its last.
fn along(p: vec2f, deg: f32) -> f32 {
  let d = dir_of(deg);
  let w = u.res.xy;
  let lo = min(0.0, d.x * w.x) + min(0.0, d.y * w.y);
  let hi = max(0.0, d.x * w.x) + max(0.0, d.y * w.y);
  return (dot(p, d) - lo) / max(hi - lo, 1.0);
}
fn diag() -> f32 { return length(u.res.xy); }
fn tr_step(p: vec2f, x: f32, k: i32, s: ptr<function, TS>) {
  let h = u.tr[1 + k * 2]; let q = u.tr[2 + k * 2];
  let kind = i32(round(h.x));
  if (kind == 0) { return; }
  let e = clamp((x - h.y) / max(h.z - h.y, 1e-4), 0.0, 1.0);
  let a = h.w; let b = q.x; let c = q.y; let d = q.z; let ee = q.w;
  let sc = u.res.z;
  if (kind == 1) {
    // Reveal by a shape.
    var o = 0.0;
    let shape = i32(round(a));
    if (shape == 0) { o = distance(p, u.tr[0].yz) / diag(); }
    else if (shape == 1) { o = along(p, b); }
    else if (shape == 2) { let rh = max(c * sc, 1.0); o = floor(p.y / rh) * rh / u.res.y; }
    else if (shape == 3) { let cs = max(c * sc, 2.0); o = h1(floor(p / cs) + 0.37); }
    else { o = clamp(along(p, b) + (vnoise(p / (40.0 * sc)) - 0.5) * c * sc / diag() * 4.0, 0.0, 1.0); }
    let soft = max(d * sc / diag(), 1e-4);
    var m = 1.0 - smoothstep(e * (1.0 + soft) - soft, e * (1.0 + soft), o);
    // Rows just reached show only the lattice's dots, as if they latch on one by one.
    if (ee > 0.0 && o < e && o > e - ee) {
      let pitch = max(1.5, u.fa.x * sc);
      let cell = (floor(p / pitch) + 0.5) * pitch;
      m = unit(p - cell, pitch * 0.42);
    }
    (*s).m = max((*s).m, m);
  } else if (kind == 2) {
    // Bands tear sideways, the channels part, a few hard cuts; locked from `ee` on.
    let bh = max(a * sc, 1.0);
    let band = floor(p.y / bh);
    let lock = clamp(ee, 0.05, 1.0);
    let env = 1.0 - smoothstep(lock - 0.08, lock, e);
    let cut = floor(e * max(d, 1.0));
    let off = (h1(vec2f(band, cut + 0.5)) - 0.5) * 2.0 * b * sc * env * step(0.45, h1(vec2f(band * 0.7, cut + 3.0)));
    (*s).offn = (*s).offn + vec2f(off, 0.0);
    (*s).offo = (*s).offo + vec2f(off, 0.0);
    (*s).split = max((*s).split, c * sc * env);
    // Before the lock each cut shows one frame or the other; some bands run ahead.
    var m = select(0.0, 1.0, h1(vec2f(cut, 9.1)) > 0.5 || h1(vec2f(band, cut + 5.0)) < e);
    if (e >= lock) { m = 1.0; }
    (*s).m = max((*s).m, m);
  } else if (kind == 3) {
    let m = smoothstep(0.0, 1.0, e);
    (*s).offn = (*s).offn + dir_of(b) * a * sc * (1.0 - m);
    (*s).m = max((*s).m, m);
  } else if (kind == 4) {
    // A photocopier: the new page behind a light bar, toner noise settling.
    let o = along(p, a);
    let span = diag();
    let edge = e * 1.1;
    let dist = (edge - o) * span;
    let bar = exp(-(dist * dist) / max(b * b * sc * sc * 0.25, 1.0));
    (*s).tint = max((*s).tint, vec4f(1.0, 1.0, 0.98, bar * 0.85 * (1.0 - smoothstep(0.92, 1.0, e))));
    if (dist > 0.0) {
      (*s).m = max((*s).m, 1.0);
      let n = (h1(floor(p / max(sc, 1.0)) + floor(e * 30.0)) - 0.5) * c * exp(-dist / max(d * sc, 1.0));
      (*s).toner = (*s).toner + n; // brightness noise on the new side, settling behind the bar
    }
  } else if (kind == 5) {
    // Flat colour planes on a grid slide across and uncover the new page.
    let rows = max(round(a), 1.0);
    let rh = u.res.y / rows;
    let j = floor(p.y / rh);
    let w = u.res.x;
    let lead = (e * 1.8 - (j / rows) * 0.5) * w * 1.25;
    let tail = lead - w * 0.55;
    if (p.x < tail) { (*s).m = max((*s).m, 1.0); }
    else if (p.x < lead) {
      let hue = j % max(round(b), 1.0);
      let col = select(select(u.accent.rgb, u.second.rgb, hue > 0.5), u.third.rgb, hue > 1.5);
      (*s).tint = vec4f(col, 1.0);
    }
  } else if (kind == 7) {
    // Layers part in depth (the brighter the nearer), swap at the middle, close again.
    let dv = dir_of(b) * a * sc;
    let lo = lum(old_at(p));
    (*s).offo = (*s).offo + dv * sin(3.14159 * min(e * 2.0, 1.0)) * (0.4 + lo) * step(e, 0.5);
    (*s).offn = (*s).offn - dv * sin(3.14159 * max(e * 2.0 - 1.0, 0.0)) * 0.7;
    (*s).m = max((*s).m, smoothstep(0.45, 0.55, e));
  } else if (kind == 8) {
    // Boxes find elements one after another, then what is in them is the new page.
    let n = i32(clamp(round(a), 0.0, 16.0));
    let lw = max(b * sc, 1.0);
    for (var i = 0; i < n; i++) {
      let r = u.boxes[i];
      if (r.z <= r.x) { continue; }
      let st = f32(i) / f32(max(n, 1)) * 0.5;
      let dp = clamp((e - st) / 0.25, 0.0, 1.0);
      if (dp <= 0.0) { continue; }
      let inside = p.x >= r.x && p.y >= r.y && p.x < r.z && p.y < r.w;
      let edge = inside && (p.x < r.x + lw || p.y < r.y + lw || p.x >= r.z - lw || p.y >= r.w - lw);
      // The frame is drawn on around its edge.
      let per = 2.0 * ((r.z - r.x) + (r.w - r.y));
      var pos = 0.0;
      if (p.y < r.y + lw) { pos = p.x - r.x; } else if (p.x >= r.z - lw) { pos = (r.z - r.x) + (p.y - r.y); }
      else if (p.y >= r.w - lw) { pos = (r.z - r.x) + (r.w - r.y) + (r.z - p.x); } else { pos = per - (p.y - r.y); }
      if (edge && pos <= dp * per) { (*s).tint = vec4f(u.accent.rgb, 1.0 - smoothstep(0.85, 1.0, e)); }
      if (inside && e > st + 0.25) { (*s).m = max((*s).m, 1.0); }
    }
    (*s).m = max((*s).m, smoothstep(0.8, 1.0, e));
  } else if (kind == 9) {
    // Carved in: the old fades to the ground, strokes grow along the new outlines, the ink floods.
    let sd = sdist(p);
    let reach = a * sc * smoothstep(0.1, b, e);
    (*s).fade = max((*s).fade, smoothstep(0.0, 0.3, e));
    if (abs(sd) < reach && near_t(p).r > 0.02) { (*s).m = max((*s).m, 1.0); }
    (*s).m = max((*s).m, smoothstep(b, 1.0, e));
  } else if (kind == 10) {
    // Outlines first, in reading order, then the content fills.
    let sd = sdist(p);
    let o = along(p, 30.0);
    (*s).fade = max((*s).fade, smoothstep(0.0, 0.25, e));
    if (abs(sd) < max(a * sc, 0.8) && near_t(p).r > 0.02 && o < (e - 0.05) / 0.45) { (*s).m = max((*s).m, 1.0); }
    (*s).m = max((*s).m, smoothstep(b, 1.0, e));
  } else if (kind == 11) {
    // Cracks (DESIGN.md §4.1): a hashed cell pattern (pieces of `a` px, squashed along the angle `b` by
    // `c` into long pieces), its edges straight but for `d` px of slow waver. The cracks run out from
    // where the commit was pressed (the page's centre without one), reaching the far corner at 0.4 of
    // the step; they stay thin (0.6 px to 0.06 a). Then each piece leaves whole at its own time, sooner
    // nearer the press and a little by its hash: between `t` and `t + 0.15` its own crack widens until
    // it covers the piece, so at any moment most pieces are whole or gone.
    // Inside a commit's scope (El::commit_scope) everything is measured against the scope's box: its
    // pieces fit three and a half on its short side, and the run and the farthest corner are its own.
    let scoped = u.sbox.z > u.sbox.x;
    let bx0 = select(vec2f(0.0), u.sbox.xy, scoped);
    let bx1 = select(u.res.xy, u.sbox.zw, scoped);
    let ae = select(a, clamp(min(bx1.x - bx0.x, bx1.y - bx0.y) / sc / 3.5, 16.0, max(a, 16.0)), scoped);
    let cs = clamp(ae, 16.0, 400.0) * sc;
    let ang = radians(b);
    let sq = vec2f(mix(1.0, 0.25, clamp(c, 0.0, 1.0)), 1.0);
    let q = rot(p, -ang) / cs * sq;
    let ip = floor(q); let fp = q - ip;
    var mr = vec2f(0.0); var md = 8.0; var mg = vec2f(0.0);
    for (var j = -1; j <= 1; j++) { for (var i = -1; i <= 1; i++) {
      let g = vec2f(f32(i), f32(j));
      let r = g + h2(ip + g + 0.71) * 0.6 + 0.2 - fp;
      let dd = dot(r, r);
      if (dd < md) { md = dd; mr = r; mg = g; }
    } }
    // The distance to the nearest edge in page px: an edge's distance in the squashed space over how
    // much the squash shortens its normal, so a crack is as wide whichever way it runs.
    var edge = 1e4;
    for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
      let g = mg + vec2f(f32(i), f32(j));
      let r = g + h2(ip + g + 0.71) * 0.6 + 0.2 - fp;
      if (dot(mr - r, mr - r) > 1e-5) {
        let nq = normalize(r - mr);
        edge = min(edge, dot(0.5 * (mr + r), nq) * cs / max(length(nq * sq), 1e-3));
      }
    } }
    var o = u.tr[0].yz;
    if (o.x <= 0.0 && o.y <= 0.0) { o = u.res.xy * 0.5; }
    let dg = select(diag(), distance(bx0, bx1), scoped);
    let rag = (vnoise(p / (max(ae / 5.0, 2.0) * sc)) - 0.5) * 2.0 * d * sc;
    let dist = max(edge + rag, 0.0);
    // The cracks' run and width: both done at 0.4.
    let run = clamp(e / 0.4, 0.0, 1.0);
    let hair = select(0.0, mix(0.6 * sc, max(0.06 * ae * sc, select(0.6, 1.0, scoped) * sc), run), distance(p, o) < run * dg && e > 0.0);
    // The piece this pixel is in: its centre on the page, and when it leaves.
    let cell = ip + mg;
    let centre = rot((cell + h2(cell + 0.71) * 0.6 + 0.2) / sq * cs, ang);
    // Measured against the farthest corner from the press, so the last pieces leave at the step's end.
    let far = max(max(distance(o, bx0), distance(o, vec2f(bx1.x, bx0.y))), max(distance(o, vec2f(bx0.x, bx1.y)), distance(o, bx1)));
    let leave = 0.28 + 0.6 * (0.6 * clamp(distance(centre, o) / max(far, 1.0), 0.0, 1.0) + 0.4 * h1(cell + 3.17));
    let own = clamp((e - leave) / 0.15, 0.0, 1.0);
    // Slow at first and fast at the end, so a piece is seldom half gone.
    let width = max(hair, select(0.0, mix(hair, 1.6 * cs, own * own * own * own), own > 0.0));
    // `width` is the crack's full width: a pixel shows the new page within half of it of an edge.
    let half = width * 0.5;
    var m = select(smoothstep(half + 1.0 * sc, half - 1.0 * sc, dist), step(dist, half), ee >= 0.5);
    if (width <= 0.0) { m = 0.0; }
    if (own >= 1.0 || e >= 1.0) { m = 1.0; }
    if (e <= 0.0) { m = 0.0; }
    (*s).m = max((*s).m, m);
  }
}
/// The state the steps build at `x` (0–1 of the transition). The loop's bound is a uniform, so it is
/// compiled once, not once per step.
fn tr_eval(p: vec2f, x: f32) -> TS {
  var s: TS;
  s.m = 0.0; s.offn = vec2f(0.0); s.offo = vec2f(0.0); s.tint = vec4f(0.0); s.fade = 0.0; s.split = 0.0; s.toner = 0.0;
  let n = i32(clamp(u.rip.z, 0.0, 4.0));
  for (var k = 0; k < n; k++) { tr_step(p, x, k, &s); }
  return s;
}

/// Zero a box of an image (stage 1 drawn again only where things changed, DESIGN.md §1.19).
@fragment fn fs_zero() -> @location(0) vec4f { return vec4f(0.0); }
/// Copy the kept frame to the target.
@fragment fn fs_blit(@builtin(position) frag: vec4f) -> @location(0) vec4f { return textureLoad(tex0, vec2i(floor(frag.xy)), 0); }
/// A see-through group (`El::opacity`) laid on the ink: its own image times its opacity, which comes
/// in the vertex numbers: the triangle's first vertex is 3 × the opacity in thousandths.
struct GOut { @builtin(position) pos: vec4f, @location(0) @interpolate(flat) op: f32 }
@vertex fn vs_group(@builtin(vertex_index) vi: u32) -> GOut {
  var o: GOut;
  let c = vi % 3u;
  o.pos = vec4f(f32((c << 1u) & 2u) * 2.0 - 1.0, f32(c & 2u) * 2.0 - 1.0, 0.0, 1.0);
  o.op = f32(vi / 3u) / 1000.0;
  return o;
}
@fragment fn fs_group(i: GOut) -> @location(0) vec4f { return textureLoad(tex0, vec2i(floor(i.pos.xy)), 0) * i.op; }

// ── Plates (DESIGN.md §1.33): ink that stays. Each pixel keeps when it was last drawn, in ticks, as 24
// bits in rgb (a: drawn at all), twice: where the ink was drawn (`hold` when laid on) and where its
// nearness reached (`lo`), so a new stroke's spread keeps its own outline alive without making the old
// ink under it new. Laid on, the ink's coverage is multiplied by 1 − smoothstep(0.8, 1, age) (it fades,
// as plain's condensing is a fade in), and the nearness, a distance, stays whole until the age is 1.
// The slot comes in the vertex numbers (3 × slot), as a group's opacity does.
fn plate_tick() -> vec4f {
  let t = u.plate.x;
  return vec4f(floor(t / 65536.0) / 255.0, (floor(t / 256.0) % 256.0) / 255.0, (t % 256.0) / 255.0, 1.0);
}
@fragment fn fs_plate_age(iv: VOut) -> @location(0) vec4f {
  let i = exact_i(iv);
  if (ink_of(i, false).a <= 0.0) { discard; }
  return plate_tick();
}
@fragment fn fs_plate_age_near(iv: VOut) -> @location(0) vec4f {
  let i = exact_i(iv);
  if (ink_of(i, false).a <= 0.0 && near_of(i).r <= 0.0) { discard; }
  return plate_tick();
}
fn plate_age(slot: u32, a: vec4f) -> f32 {
  let drawn = round(a.r * 255.0) * 65536.0 + round(a.g * 255.0) * 256.0 + round(a.b * 255.0);
  return select(2.0, max(u.plate.x - drawn, 0.0) / u.plates[slot].x, a.a > 0.0);
}
@fragment fn fs_plate_ink(i: GOut) -> @location(0) vec4f {
  let p = vec2i(floor(i.pos.xy));
  let slot = u32(round(i.op * 1000.0));
  if (u.plates[slot].x <= 0.0) { return textureLoad(tex0, p, 0); }
  return textureLoad(tex0, p, 0) * (1.0 - smoothstep(0.8, 1.0, plate_age(slot, textureLoad(hold, p, 0))));
}
@fragment fn fs_plate_near(i: GOut) -> @location(0) vec4f {
  let p = vec2i(floor(i.pos.xy));
  let slot = u32(round(i.op * 1000.0));
  if (u.plates[slot].x <= 0.0) { return textureLoad(tex1, p, 0); }
  return textureLoad(tex1, p, 0) * step(plate_age(slot, textureLoad(lo, p, 0)), 0.99999);
}

// ── The finishing pass (DESIGN.md §1.21): the kept frame on its way to the screen. In this order:
// bloom → chromatic offset → scanlines → grain → vignette → curvature, then dither and the palette,
// then the negative.
// Curvature is the last thing done to the image, so its warp is worked out first and everything
// before it is read where it lands; the dither is on the screen's own pixels.
fn post_src(p: vec2f) -> vec3f { return textureSampleLevel(tex0, samp, p / u.res.xy, 0.0).rgb; }
/// Whether `p` is within the bloom's region (`El::bloom`), when it has one.
fn in_bloom(p: vec2f) -> bool { let b = u.post[6]; return u.post[4].w < 0.5 || (p.x >= b.x && p.y >= b.y && p.x < b.z && p.y < b.w); }
fn post_bloom(p: vec2f) -> vec3f {
  var c = post_src(p);
  let s = u.post[1].x;
  // With a region, it glows only inside it and only from what is inside it.
  if (s > 0.0 && in_bloom(p)) {
    let r = u.post[1].z;
    let th = u.post[1].y;
    var acc = vec3f(0.0);
    // Three rings of six taps (at the radius, two thirds and a third of it), the whole pattern turned
    // by a per-pixel noise so the taps never show as copies of what glows: a soft glow in one pass,
    // weighted toward the centre.
    let spin = fract(52.9829189 * fract(dot(floor(p), vec2f(0.06711056, 0.00583715)))) * 6.2831853;
    var wsum = 0.0;
    for (var i = 0; i < 18; i++) {
      let ring = i / 6;
      let k = 1.0 - f32(ring) / 3.0;
      let a = f32(i % 6) * 1.0471976 + spin + f32(ring) * 0.5235988;
      let w = 0.6 + 0.4 * (1.0 - k);
      let q = p + vec2f(cos(a), sin(a)) * r * k;
      acc += w * max(post_src(q) - vec3f(th), vec3f(0.0)) * select(0.0, 1.0, in_bloom(q));
      wsum += w;
    }
    c += acc * (s * 2.0 / wsum);
  }
  return c;
}
fn to_srgb(c: vec3f) -> vec3f { return select(1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3f(0.0031308)); }
fn from_srgb(c: vec3f) -> vec3f { return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045)); }
// `levels` is log2(n), given rather than counted: GLSL ES 3.0 (WebGL2) has no findLSB.
fn bayer_t(p: vec2f, n: u32, levels: u32) -> f32 {
  let x = u32(p.x); let y = u32(p.y);
  var v = 0u;
  for (var i = 0u; i < levels; i++) {
    let bx = (x >> i) & 1u; let by = (y >> i) & 1u;
    let d = select(select(3u, 1u, bx == 1u), select(0u, 2u, bx == 1u), by == 0u);
    v += d << (2u * (levels - 1u - i));
  }
  return (f32(v) + 0.5) / f32(n * n);
}
fn dither_t(kind: f32, p: vec2f) -> f32 {
  let k = i32(round(kind));
  if (k == 1) { return bayer_t(p, 2u, 1u); }
  if (k == 2) { return bayer_t(p, 4u, 2u); }
  if (k == 3) { return bayer_t(p, 8u, 3u); }
  if (k == 4) { return fract(p.x * 0.7548777 + p.y * 0.5698403); }
  if (k == 5) { return 0.125 + 0.75 * bayer_t(p, 4u, 2u); }
  return 0.5;
}
/// The text mask at `p` in the finishing pass (`fs_tmask`, bound in the hold slot).
fn tmask_at(p: vec2f) -> f32 { return textureLoad(hold, vec2i(clamp(floor(p), vec2f(0.0), u.res.xy - 1.0)), 0).r; }
@fragment fn fs_post(@builtin(position) frag: vec4f) -> @location(0) vec4f {
  let res = u.res.xy;
  let ctr = res * 0.5;
  let half = length(ctr);
  var p = frag.xy;
  let bend = u.post[3].z;
  if (bend != 0.0) {
    let d = (p - ctr) / half;
    p = ctr + d * (1.0 + bend * dot(d, d)) * half;
    if (p.x < 0.0 || p.y < 0.0 || p.x >= res.x || p.y >= res.y) { return vec4f(0.0, 0.0, 0.0, 1.0); }
  }
  var c: vec3f;
  let off = u.post[2].x;
  var sl = u.post[2].y;
  if (off != 0.0) {
    let dir = (p - ctr) / max(length(p - ctr), 1.0);
    // Text to read kept clear (`PostSpec::clear_text`; the text mask is in the hold slot here): a channel
    // is not offset where the pixel or the point it would be read from is on its ink or within 2 px of it,
    // so the text has no fringe and leaves none beside it.
    var r = off; var b = off;
    if (u.post[7].x > 0.5) {
      let m = tmask_at(p);
      r *= 1.0 - max(m, tmask_at(p + dir * off));
      b *= 1.0 - max(m, tmask_at(p - dir * off));
    }
    c = vec3f(post_bloom(p + dir * r).r, post_bloom(p).g, post_bloom(p - dir * b).b);
  } else { c = post_bloom(p); }
  // And its scanlines calmer.
  if (u.post[7].x > 0.5) { let m = tmask_at(p); if (m > 0.0) { sl = mix(sl, min(sl, u.post[7].y), m); } }
  if (sl > 0.0) { c *= 1.0 - sl * (0.5 + 0.5 * cos(6.2831853 * (floor(p.y) + 0.5) / u.post[2].z)); }
  let ga = u.post[2].w;
  // Within the region the runtime set (`El::grain`), when it set one.
  let gr = u.post[5];
  let in_gr = u.post[4].z < 0.5 || (p.x >= gr.x && p.y >= gr.y && p.x < gr.z && p.y < gr.w);
  if (ga > 0.0 && in_gr) { c += (h1(floor(p / u.post[3].w) + 0.5) - 0.5) * ga; }
  let vs = u.post[3].x;
  if (vs > 0.0) { c *= 1.0 - vs * smoothstep(u.post[3].y, 1.0, length(p - ctr) / half); }
  c = clamp(c, vec3f(0.0), vec3f(1.0));
  let kind = u.post[0].x;
  let n = i32(round(u.post[0].z));
  if (kind > 0.5 || n > 0) {
    let srgb = u.fd.y > 0.5;
    var s = select(c, to_srgb(c), srgb);
    let t = select(0.5, dither_t(kind, frag.xy), kind > 0.5);
    let amp = u.post[1].w;
    if (n > 0) {
      // Spread by about one palette step, then the nearest colour (the eye's weights).
      s = s + (t - 0.5) * amp / max(f32(n - 1), 1.0);
      var best = u.pal[0].rgb; var bd = 1e9;
      for (var i = 0; i < 16; i++) {
        if (i >= n) { break; }
        let e = (u.pal[i].rgb - s) * vec3f(0.30, 0.59, 0.11);
        let d = dot(e, u.pal[i].rgb - s);
        if (d < bd) { bd = d; best = u.pal[i].rgb; }
      }
      s = best;
    } else {
      let lv = max(u.post[0].y - 1.0, 1.0);
      s = clamp(floor(s * lv + 0.5 + (t - 0.5) * amp), vec3f(0.0), vec3f(lv)) / lv;
    }
    c = select(s, from_srgb(s), srgb);
  }
  // The negative, last of all and in the colours as stored (sRGB), so turning it on twice is the frame.
  if (u.post[4].x > 0.5) {
    let srgb = u.fd.y > 0.5;
    c = select(vec3f(1.0) - c, from_srgb(vec3f(1.0) - to_srgb(c)), srgb);
  }
  return vec4f(c, 1.0);
}

/// The material pass. At full resolution `qual.x` is 1 and every pixel is shaded. For the reduced pass
/// it is 1 / q, so the pixel centre of the small target lands on the full frame's coordinates; a pixel
/// at rest there is not shaded but marked (alpha 1) for the full-resolution pass to shade, a loose one
/// is shaded (alpha 0).
/// Where reduced texel `t` (its centre) is, device px: on a grid fixed to the anchor's content (`u.lop`),
/// on 1/256 px so that a texel lands on the same point whatever the offset.
fn lo_at(t: vec2f) -> vec2f { return round((t + u.lop.zw) * u.qual.x * 256.0) / 256.0 - u.lop.xy; }
@fragment fn fs_material(@builtin(position) frag: vec4f) -> @location(0) vec4f {
  if (u.qual.x > 1.0) {
    let p = lo_at(frag.xy);
    // A stage at its own scale is shaded here; elsewhere, when the pass is for the stages alone, nothing is.
    if (!in_stage(p) && !soft_at(p) && (u.stagen.z > 0.5 || rest_at(p))) { return vec4f(0.0, 0.0, 0.0, 1.0); }
    return vec4f(shade(p), 0.0);
  }
  return vec4f(shade(frag.xy), 1.0);
}

/// Whether the scroll trace can show at `pf` in this frame or the last (DESIGN.md §1.19): a scroll move
/// with its trace showing, `u.mv.x` the stronger of the two and `u.mv.yz` the way it points. As
/// `scroll_trace` reads, from where the material reads: any ink, by the nearness (it reaches SPREAD px),
/// within as far round the trace's samples as they spread across, along its longest reach.
fn trace_may(pf: vec2f) -> bool {
  let s = u.res.z;
  let sc = scroll_here(pf);
  anc = sc.o;
  var p = pf;
  let m0 = eff_mat(near_t(pf).a * 255.0);
  if (sc.k > 0.0) {
    if (m0 == DOTTR) { p = latch(pf, sc, max(1.5, u.fa.x * s)); }
    else if (m0 == CHARR) { p = latch(pf, sc, clamp(round(mp(CHARR, 0).x * s), CHAR_MIN, CHAR_MAX)); }
  }
  let here = near_t(p);
  let m = eff_mat(here.a * 255.0);
  if (here.g > 0.05 || m == GLITCHR || m == NEAR || dec(p).a > 0.5) { return false; }
  let k = u.mv.x; let d = u.mv.yz;
  var o = p; var len = 0.0; var side = 0.0;
  if (m == DOTTR) {
    let pitch = max(1.5, u.fa.x * s);
    o = (floor(lat(p) / pitch) + 0.5) * pitch - anc;
    len = max(1.0, round(u.scrp.x * k / pitch)) * pitch; side = pitch * 0.36;
  } else if (m == CHARR) {
    let P = clamp(round(mp(CHARR, 0).x * s), CHAR_MIN, CHAR_MAX);
    o = (floor(lat(p) / P) + 0.5) * P - anc;
    len = max(1.0, round(u.scrp.x * k / P)) * P; side = P * 0.71;
  } else if (m == HALFTONR) {
    len = u.scrp.y * k; side = max(2.0, mp(HALFTONR, 0).x * s * 0.5) * 1.14;
  } else if (m == SMUDGR) { len = u.scrp.y * k; }
  else if (m == CARVR) { len = u.scrp.x * k; }
  else { len = u.scrp.x * k * 0.45; }
  // Probes every `h` px: ink within `r` of one is where the nearness is above −r. The trace reaches ink
  // within `side + 1.5` of its path; a probe stands for the path within `h / 2` of it, so the probes are as
  // far apart as the nearness lets them be (it reads up to SPREAD px), and never nearer than 4 px.
  let c = side + 1.5;
  let h = max(4.0, 2.0 * (u.fd.z - 1.0 - c));
  let r = c + h * 0.5;
  let n = i32(ceil(len / h));
  if (r >= u.fd.z || n > 64) { return true; }
  for (var j = 0; j <= 64; j++) {
    if (j > n) { break; }
    if (sdist(o + d * min(f32(j) * h, len)) >= -r) { return true; }
  }
  return false;
}
/// A scroll move with its trace showing: the pixels the trace can reach are shaded, the rest kept.
@fragment fn fs_trace(@builtin(position) frag: vec4f) -> @location(0) vec4f {
  if (!trace_may(frag.xy)) { discard; }
  return vec4f(shade(frag.xy), 1.0);
}

/// Whether `p` is in a stage shaded at its own scale (DESIGN.md §1.35) and not under what is drawn over it.
fn in_stage(p: vec2f) -> bool {
  let n = i32(u.stagen.x);
  var hit = false;
  for (var k = 0; k < 16; k++) {
    if (k >= n) { break; }
    let r = u.stages[k];
    if (all(p >= r.xy) && all(p < r.zw)) { hit = true; break; }
  }
  if (!hit) { return false; }
  let m = i32(u.stagen.y);
  for (var k = 0; k < 16; k++) {
    if (k >= m) { break; }
    let r = u.sharp[k];
    if (all(p >= r.xy) && all(p < r.zw)) { return false; }
  }
  return true;
}
/// Whether `p` is shaded softened while it scrolls (DESIGN.md §1.12): inside a scroll area, and not at
/// an edge of the ink (a glyph, a thin stroke, a focus corner, a panel's edge stay sharp), which is where
/// the ink around it is not the same (its coverage, or a glyph's tone over a shape).
fn soft_at(p: vec2f) -> bool {
  if (u.stagen.w < 0.5) { return false; }
  var hit = false;
  for (var j = 0; j < 8; j++) {
    let b = u.scrolls[j];
    if (b.z <= b.x) { break; }
    if (all(p >= b.xy) && all(p < b.zw)) { hit = true; break; }
  }
  if (!hit) { return false; }
  let e = 0.75 * u.qual.x + 0.5;
  let c = ink_t(p);
  var most = 0.0;
  for (var k = 0; k < 4; k++) {
    let d = vec2f(select(-e, e, (k & 1) == 1), select(-e, e, (k & 2) == 2));
    let v = abs(ink_t(p + d) - c);
    most = max(most, max(max(v.r, v.g), max(v.b, v.a)));
  }
  return most < 0.02;
}
/// Whether nothing is really loose at `p` (it is shaded at full resolution): nothing past the
/// legibility cap from fronts, the half-plane or its own looseness (what is held there, a pressed
/// control, keeps the material's resting look, sharp), no lens, no burst over a 3D element, a
/// picture's marks not showing.
fn rest_at(p: vec2f) -> bool {
  let n = near_t(p);
  if (n.g > 0.05) { return pic_k(p) <= 0.0; }
  return kt_loose(p) < 0.02 && (n.g < 0.002 || burst_k(p) <= 0.0) && lens(p) < 0.02;
}
/// After the reduced pass: where any of the reduced texels around this pixel was at rest, the
/// pixel is shaded here at full resolution (so a page at rest is exactly what quality 1 draws, and the
/// edge of a loose region is sharp); where all four were loose, it takes their filtered colour.
@fragment fn fs_material_up(@builtin(position) frag: vec4f) -> @location(0) vec4f {
  let p = frag.xy;
  // The four texels round it, weighed by where it is among their centres (`lo_at`): its alpha is above 0
  // exactly when a texel with weight here was at rest. Read and weighed here, not by the sampler, so the
  // same content point weighs them the same at any offset.
  let t = (p + u.lop.xy) * u.qual.y - u.lop.zw - 0.5;
  let i = floor(t);
  let w = t - i;
  let hi = vec2i(u.qual.zw) - 1;
  let a = clamp(vec2i(i), vec2i(0), hi);
  let b = clamp(vec2i(i) + 1, vec2i(0), hi);
  var c = mix(mix(textureLoad(lo, a, 0), textureLoad(lo, vec2i(b.x, a.y), 0), w.x), mix(textureLoad(lo, vec2i(a.x, b.y), 0), textureLoad(lo, b, 0), w.x), w.y);
  if (SAMPLED_UP) { c = textureSampleLevel(lo, samp, (t + 0.5) / u.qual.zw, 0.0); }
  if (c.a > 0.001) { return vec4f(shade(p), 1.0); }
  return vec4f(c.rgb, 1.0);
}
