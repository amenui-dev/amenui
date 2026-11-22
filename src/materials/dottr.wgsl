// @material dottr
// dottr: the global lattice; text loosens into units sized by the ink under them, the ground is a field
// of small units.
fn m_dottr(frag: vec2f) -> vec3f {
  let s = u.res.z; let t = u.res.w;
  var p = frag;
  // While a front is passing, some thin horizontal bands slip sideways.
  let bh = 5.0 * s; let band = floor(lat(p).y / bh);
  let w0 = wave(vec2f(p.x, (band + 0.5) * bh - anc.y), 0.6);
  let pick = step(0.55, h1(vec2f(band, floor(t * 9.0))));
  // Only what is really loose slips: nudged text, and text held at the legibility cap, stays in place.
  p.x += (h1(vec2f(band, floor(t * 17.0))) - 0.5) * 70.0 * s * u.fa.w * pick * smoothstep(0.2, 0.9, w0) * smoothstep(0.2, 0.45, loose_t(frag));
  let here = near_t(frag);
  // Past half loose the units themselves go on changing: their lattice opens to 1.6 times the pitch
  // and they stray twice as far from it (the element's own looseness, so one element keeps one lattice).
  // (Its own looseness, before any hold of text to read: the lattice does not change size round a label.)
  let on = smoothstep(0.5, 1.0, clamp((dec_loose(here.b) + u.rip.y) * loose_scale(), 0.0, u.rip.x));
  let pitch = max(1.5, u.fa.x * s) * mix(1.0, 1.6, on);
  let cell = floor(lat(p) / pitch);
  let block = (cell + 0.5) * pitch - anc;
  // Toward text to read the units stray less and grow, until by `r0` they join into its island.
  let hb = hold_at(block);
  let centre = block + (h2(cell) - 0.5) * u.fa.z * pitch * (1.0 + on) * (1.0 - hb);
  // Units by the ink's own looseness: text to read keeps its island (`core_at`, below) and the lattice
  // runs right up to it, so the island is the glyphs' shape, not a block of cells.
  let own = select(loose_t(centre), min(dec_loose(near_t(centre).b), u.rip.x), hb > 0.0);
  let loose = min((half_plane(centre) + own + (h1(cell + 7.0) - 0.5) * 0.22) * loose_scale(), u.rip.x);
  let core = core_at(frag);
  let printed = select(0.0, here.a * 255.0 * 0.5, here.a < 0.5);
  let printed_b = select(0.0, near_t(block).a * 255.0 * 0.5, near_t(block).a < 0.5);
  var col: vec3f; var alpha: f32;
  if (loose > 0.3) {
    let q = pitch * 0.25;
    var c = (ink(block + vec2f(-q, -q)) + ink(block + vec2f(q, -q)) + ink(block + vec2f(-q, q)) + ink(block + vec2f(q, q))) * 0.25;
    let grow = smoothstep(0.3, 0.42, loose);
    var d = dec_at(c, frag);
    // A cell with ink in it that the taps (almost) missed, a thin line's: a dot of the least ink.
    if (c.a < INK_FLOOR) { let e = cell_ink(block, pitch * 0.5); if (e.z > 0.0) { if (c.a < 1e-3) { d.rgb = dec(e.xy).rgb; } c.a = INK_FLOOR; } }
    var r = pitch * mix(0.5, 0.44 * sqrt(c.a), grow) * select(1.0, 0.96, u.fc.w > 0.5);
    // Units grow toward the island until they touch their neighbours by `r0`.
    r = mix(r, max(r, pitch * 0.56), smoothstep(0.15, 0.85, hb));
    let m = unit(p - centre, r) * step(0.06, c.a);
    col = d.rgb; alpha = m * mix(c.a, 1.0, grow);
  } else if (max(printed, printed_b) > 0.0) {
    // Printed type: the glyph seen through a coarse lattice, each cell filled when more than half of
    // it is covered.
    let pp = max(1.0, round(max(printed, printed_b) * s));
    let cc = cell_avg(floor(lat(p) / pp), pp);
    let d = dec_at(cc, frag);
    col = d.rgb; alpha = step(0.5, cc.a);
  } else {
    let d = dec_at(ink(p), frag);
    col = d.rgb; alpha = d.a;
  }
  // Ground: units that follow the fronts and the pointer, drift slowly at rest, and shrink away from
  // ink by its true outline (the nearness image), never by a rectangle.
  var g = gnd(frag);
  let gp = 9.0 * s; let gc = floor(lat(frag) / gp); let gcen = (gc + 0.5) * gp - anc;
  // The ground drifts on the ground clock, which runs only with what moves the whole window (a commit,
  // a front that reaches everywhere, a morph, a flow): a local press does not move the whole ground.
  let mt = u.rip.w;
  let drift = (sin(gc.x * 0.21 + mt * 0.33) + sin(gc.y * 0.37 - mt * 0.21) + sin((gc.x + gc.y) * 0.13 + mt * 0.17)) / 3.0;
  var lvl = max(0.0, drift - 0.25) * 0.55 * step(h1(gc), 0.4) + wave(gcen, 0.16) * 0.95 + lens(gcen) * 0.7;
  // The field map: its energy lifts the units like a front, its flow leans them up to half a pitch.
  var gat = gcen;
  if (fmap_on() && !calm_at(gcen)) { lvl += fmap_e(gcen) * 0.95; gat = gcen + fmap_flow(gcen) * gp * 0.5; }
  lvl *= 1.0 - smoothstep(0.1, 0.35, near(gcen).r);
  let gu = unit(frag - gat, gp * 0.42 * clamp(lvl, 0.0, 1.0)) * step(0.05, lvl);
  g = mix(g, u.fg.rgb, gu * 0.34 * select(0.0, 1.0, ground_marks()));
  let out = mix(g, col, alpha);
  return mix(out, exact(frag), core);
}
