// The mesh pass and its resolve (DESIGN.md §1.18). A scene3d element's meshes are drawn into an
// off-screen target (its box × resolution_scale) with depth, into three images:
//   colour (rgba8): r = flat shade (ambient + lambert), g = tone, b = accent, a = 0.5 + hue / 4 (0 = empty)
//   normal (rgba8): rg = view-space normal xy (0.5 + 0.5 n), ba = linear depth as 16 bits (hi, lo)
//   id     (rgba8): rg = copy number + 1 as 16 bits (lo, hi), b = view-space normal z, a = where drawn,
//          the outline's tone (1: the ink; at least 1 / 255), 0 elsewhere
// Then three full-target passes turn that into what stage 1 needs:
//   mask:  r = inside the faces (drawn and not on an edge), g = drawn (the silhouette)
//   h:     for both masks, the horizontal distance (texels) to the nearest texel inside and outside
//   v:     the exact Euclidean distance (two-pass, Felzenszwalb style brute force over ±R rows), as the
//          signed nearness stage 1 uses (its zero on a whole step, MESH_ZERO, not at 0.5: a texel
//          centre a quarter of a pixel either side of an edge then decodes the same distance inside and
//          out, so an edge on a pixel boundary covers the pixel inside it fully and the one outside not
//          at all): r = silhouette, g = silhouette and edges; b = the projected
//          normal's code (1 = facing the viewer, 2..9 = its direction in eighths of a half turn); a =
//          the outline's tone where drawn (the id image's), 0 elsewhere

/// The resolve's zero distance: 128 of 255, a whole step (shader.wgsl has the same).
const MESH_ZERO: f32 = 128.0 / 255.0;

struct MU {
  view: mat4x4f,
  proj: mat4x4f,
  light: vec4f, // direction towards the light (world), ambient
  misc: vec4f,  // near, far, edge_depth, cos(edge_normal)
  size: vec4f,  // target width, height (texels), device px per texel, search radius R (texels)
  spread: vec4f, // stage 1's SPREAD (device px), two tones, terminator, _
  depth: vec4f,  // marks by depth (style::Marks): scale, dim; the copies' nearest and farthest view depth
};
@group(0) @binding(0) var<uniform> mu: MU;
@group(0) @binding(1) var tm: texture_2d<f32>; // the mask (h and v passes)
@group(0) @binding(2) var th: texture_2d<f32>; // the horizontal distances (v pass)
@group(0) @binding(3) var tg: texture_2d<f32>; // normal and depth
@group(0) @binding(4) var tk: texture_2d<f32>; // id and normal z

struct MIn {
  @location(0) pos: vec3f, @location(1) nrm: vec3f,
  @location(2) m0: vec4f, @location(3) m1: vec4f, @location(4) m2: vec4f, @location(5) m3: vec4f,
  @location(6) paint: vec4f, // tone, accent, hue, the outline's tone (0: the ink)
  @location(7) id: vec4f,    // copy number + 1 (lo byte, hi byte), _, _
};
struct MOut {
  @builtin(position) pos: vec4f,
  @location(0) n: vec3f,
  @location(1) z: f32,
  @location(2) @interpolate(flat) paint: vec4f,
  @location(3) @interpolate(flat) id: vec4f,
  @location(4) nw: vec3f,
};

@vertex fn vs_mesh(i: MIn) -> MOut {
  let m = mat4x4f(i.m0, i.m1, i.m2, i.m3);
  let v = mu.view * (m * vec4f(i.pos, 1.0));
  let nw = normalize((m * vec4f(i.nrm, 0.0)).xyz);
  var o: MOut;
  o.pos = mu.proj * v;
  o.n = normalize((mu.view * vec4f(nw, 0.0)).xyz);
  o.z = -v.z;
  o.paint = i.paint;
  o.id = i.id;
  o.nw = nw;
  return o;
}

struct GOut { @location(0) c: vec4f, @location(1) g: vec4f, @location(2) k: vec4f };
@fragment fn fs_mesh(i: MOut, @builtin(front_facing) front: bool) -> GOut {
  // The inside of an open mesh (a plane seen from below) faces the other way.
  let n = select(-normalize(i.n), normalize(i.n), front);
  var shade = mu.light.w + (1.0 - mu.light.w) * max(dot(normalize(i.nw), normalize(mu.light.xyz)), 0.0);
  // Two-tone: the lit side one flat tone, the shadow side another (Mesh3d::tones), a hard terminator.
  if (mu.spread.y > 0.5) { shade = select(mu.light.w + (1.0 - mu.light.w) * 0.42, 1.0, shade > mu.light.w + (1.0 - mu.light.w) * mu.spread.z); }
  if (!front) { shade = mu.light.w; }
  let d = clamp((i.z - mu.misc.x) / max(mu.misc.y - mu.misc.x, 1e-4), 0.0, 1.0);
  let hi = floor(d * 255.0);
  // Marks by depth: far parts are dimmer (`depth_dim` takes the shade down) and carry less ink
  // (`depth_scale` takes the tone down), so materials that size their marks by the ink there (dottr,
  // halftonr, charr, picture marks) draw them smaller and fainter with distance.
  let far = clamp((i.z - mu.depth.z) / max(mu.depth.w - mu.depth.z, 1e-4), 0.0, 1.0);
  shade *= 1.0 - mu.depth.y * far;
  let tone = i.paint.x * (1.0 - mu.depth.x * far);
  var o: GOut;
  o.c = vec4f(shade, tone, i.paint.y, 0.5 + i.paint.z * 0.25);
  o.g = vec4f(n.xy * 0.5 + 0.5, hi / 255.0, d * 255.0 - hi);
  o.k = vec4f(i.id.x / 255.0, i.id.y / 255.0, n.z * 0.5 + 0.5, select(i.paint.w, 1.0, i.paint.w <= 0.0));
  return o;
}

@vertex fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4f {
  return vec4f(f32((vi << 1u) & 2u) * 2.0 - 1.0, f32(vi & 2u) * 2.0 - 1.0, 0.0, 1.0);
}

fn inside(q: vec2i) -> bool { return q.x >= 0 && q.y >= 0 && f32(q.x) < mu.size.x && f32(q.y) < mu.size.y; }
fn drawn(q: vec2i) -> bool { return inside(q) && textureLoad(tk, q, 0).a > 0.5 / 255.0; }
fn depth(q: vec2i) -> f32 { let g = textureLoad(tg, q, 0); let d = (round(g.z * 255.0) + g.w) / 255.0; return mu.misc.x + d * (mu.misc.y - mu.misc.x); }
fn normal(q: vec2i) -> vec3f { let g = textureLoad(tg, q, 0); let k = textureLoad(tk, q, 0); return vec3f(g.xy * 2.0 - 1.0, k.z * 2.0 - 1.0); }
fn ident(q: vec2i) -> f32 { let k = textureLoad(tk, q, 0); return round(k.x * 255.0) + round(k.y * 255.0) * 256.0; }
/// Whether there is an edge between two drawn texels: another copy, a step in depth, a crease.
fn crease(p: vec2i, q: vec2i) -> bool {
  if (!drawn(q)) { return false; }
  if (ident(p) != ident(q)) { return true; }
  let zp = depth(p);
  let zq = depth(q);
  if (abs(zp - zq) > mu.misc.z * min(zp, zq)) { return true; }
  return dot(normal(p), normal(q)) < mu.misc.w;
}

// ── mask ── (a texel next to an edge on its right or below is on the edge: lines one texel wide)
@fragment fn fs_mask(@builtin(position) f: vec4f) -> @location(0) vec4f {
  let p = vec2i(floor(f.xy));
  if (!drawn(p)) { return vec4f(0.0); }
  let edge = crease(p, p + vec2i(1, 0)) || crease(p, p + vec2i(0, 1));
  return vec4f(select(1.0, 0.0, edge), 1.0, 0.0, 1.0);
}

// ── h ── horizontal distances: (to inside, to outside) for the silhouette, then for faces; 255 = none
fn mask(q: vec2i) -> vec2f { if (!inside(q)) { return vec2f(0.0); } return textureLoad(tm, q, 0).rg; }
@fragment fn fs_h(@builtin(position) f: vec4f) -> @location(0) vec4f {
  let p = vec2i(floor(f.xy));
  let R = i32(mu.size.w);
  var d = vec4f(255.0); // in_sil, out_sil, in_face, out_face
  for (var dx = -R; dx <= R; dx++) {
    let m = mask(p + vec2i(dx, 0));
    let a = f32(abs(dx));
    if (m.g > 0.5) { d.x = min(d.x, a); } else { d.y = min(d.y, a); }
    if (m.r > 0.5) { d.z = min(d.z, a); } else { d.w = min(d.w, a); }
  }
  return d / 255.0;
}

// ── v ── the distance to the nearest texel of the other kind, signed (+ inside), as nearness
fn hdist(q: vec2i) -> vec4f {
  // Rows above and below the target are outside: no inside texel, the outside right there.
  if (!inside(q)) { return vec4f(255.0, 0.0, 255.0, 0.0); }
  return round(textureLoad(th, q, 0) * 255.0);
}
@fragment fn fs_v(@builtin(position) f: vec4f) -> @location(0) vec4f {
  let p = vec2i(floor(f.xy));
  let R = i32(mu.size.w);
  let own = mask(p);
  let sil_in = own.g > 0.5;
  let face_in = own.r > 0.5;
  var best = vec2f(1e9);
  for (var dy = -R; dy <= R; dy++) {
    let h = hdist(p + vec2i(0, dy));
    let dy2 = f32(dy * dy);
    // To the other kind: inside looks for outside, outside for inside.
    let a = select(h.x, h.y, sil_in);
    let b = select(h.z, h.w, face_in);
    if (a < 254.5) { best.x = min(best.x, a * a + dy2); }
    if (b < 254.5) { best.y = min(best.y, b * b + dy2); }
  }
  let far = mu.size.w + 0.5;
  // Texel centres are a whole texel apart; the boundary lies half a texel short of the nearest one.
  var dsil = min(sqrt(best.x), far) - 0.5;
  var dface = min(sqrt(best.y), far) - 0.5;
  dsil = select(-dsil, dsil, sil_in);
  dface = select(-dface, dface, face_in);
  let px = mu.size.z;
  let s = mu.spread.x;
  var code = 0.0;
  if (sil_in) {
    let n = normal(p);
    let l = length(n.xy);
    // Screen y points down, view y up.
    let ang = atan2(-n.y, n.x);
    code = select(2.0 + floor(fract(ang / 3.14159265) * 8.0 + 0.0001) % 8.0, 1.0, l < 0.35);
  }
  let tone = select(0.0, textureLoad(tk, p, 0).a, inside(p));
  return vec4f(clamp(MESH_ZERO + dsil * px / (2.0 * s), 0.0, 1.0), clamp(MESH_ZERO + dface * px / (2.0 * s), 0.0, 1.0), code / 255.0, tone);
}
