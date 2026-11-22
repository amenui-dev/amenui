// @material charr
// @param 点距 4.0 40.0 1.0 0 px 10.0
// @param 级数 2.0 16.0 1.0 0 - 12.0
// @param 上色方式 0.0 1.0 1.0 0 - 0.0
// @param 法线 0.0 1.0 0.05 2 - 1.0
// charr: cells of the print pitch show characters from the character mark set, chosen by the ink
// they cover; the more loose, the coarser the cells.
fn m_charr(p: vec2f) -> vec3f {
  // Within `r0` of text to read the ink is exactly as at rest, an island that follows the glyphs; around
  // it every cell is a whole character (one under the island's edge is hidden by it, not cut).
  let core = core_at(p);
  if (core >= 1.0) { return exact(p); }
  return mix(m_charr_cells(p), exact(p), core);
}
fn m_charr_cells(p: vec2f) -> vec3f {
  let s = u.res.z;
  let a0 = mp(SELF, 0);
  let P0 = clamp(round(a0.x * s), CHAR_MIN, CHAR_MAX);
  let coarse = floor(lat(p) / P0);
  let kc = kt_loose_own((coarse + 0.5) * P0 - anc);
  if (kc <= 0.0 && kt_loose_own(p) <= 0.0) { return exact(p); }
  // One cell size for the whole element at a time, by its own looseness, and a clean jump between
  // them: half the pitch first, the pitch from 0.3, one and a half times it from 0.7; from there the
  // levels fall to half, so the characters go on getting coarser to the end.
  let kl = clamp((dec_loose(near_t(p).b) + u.rip.y) * loose_scale(), 0.0, u.rip.x);
  let levels = clamp(round(mix(round(a0.y), round(a0.y) * 0.5, smoothstep(0.7, 1.0, kl))), 2.0, 16.0);
  let P = select(select(P0, min(CHAR_MAX, round(P0 * 1.5)), kl >= 0.7), max(CHAR_MIN, round(P0 * 0.5)), kl < 0.3);
  let cell = floor(lat(p) / P);
  let centre = (cell + 0.5) * P - anc;
  // A thin accent element (a rule) shows as exactly one row of characters in the accent, unbroken and
  // at one height: in this column, the accent ink within a cell and a half (of the largest cell) above
  // and below is found, on one grid of taps for the whole column (at most 3 px apart), and one cell's
  // height centred on its centreline is the rule's row. Its cells take one size for each stretch of
  // three pitches, by the looseness on the rule there, so a size changes between whole characters; every
  // cell gets at least a middle character, and the pixels beside the row none of the rule.
  let Pbig = min(CHAR_MAX, round(P0 * 1.5));
  var acc_ink = vec4f(0.0);
  var lo_y = 1e9;
  var hi_y = -1e9;
  let tap = min(P * 0.25, 3.0);
  let taps = i32(ceil(3.0 * Pbig / tap)) + 1;
  let y0 = floor((centre.y - 1.5 * Pbig) / tap) * tap + 0.5;
  for (var j = 0; j <= taps; j++) {
    let q = vec2f(centre.x, y0 + f32(j) * tap);
    let t = ink_t(q);
    // A picture's own colours are not a rule, however warm.
    if (t.a > 0.3 && dec_at(t, q).acc > 0.5 && near_t(q).g <= 0.05) {
      lo_y = min(lo_y, q.y);
      hi_y = max(hi_y, q.y);
      if (t.a > acc_ink.a) { acc_ink = t; }
    }
  }
  // Thin: its accent ink in this column is less than a cell tall (a filled accent box is not a rule).
  let thin = hi_y >= lo_y && hi_y - lo_y < P;
  let mid = 0.5 * (lo_y + hi_y);
  let span = 3.0 * P0;
  let rq = vec2f((floor(lat(p).x / span) + 0.5) * span - anc.x, mid);
  let klr = clamp((loose_t(rq) + u.rip.y) * loose_scale(), 0.0, u.rip.x);
  let Pr = select(select(P0, Pbig, klr >= 0.7), max(CHAR_MIN, round(P0 * 0.5)), klr < 0.3);
  // Where the rule is really loose: its row only, every cell of it a character (whatever the cell's own
  // turn would be), and a pixel beside it that would show the rule shows the ground.
  let rule_on = thin && kt_loose(vec2f(centre.x, mid)) > 0.0;
  if (rule_on) {
    if (abs(p.y - mid) < 0.5 * Pr) {
      let lv = clamp(round(mix(round(a0.y), round(a0.y) * 0.5, smoothstep(0.7, 1.0, klr))), 2.0, 16.0);
      let ra = cell_avg(vec2f(floor(lat(p).x / Pr), floor((mid + anc.y) / Pr)), Pr);
      let rn = max(clamp(floor(ra.a * lv), 0.0, lv - 1.0), floor(lv * 0.55));
      let ruv = vec2f(fract(lat(p).x / Pr), clamp((p.y - mid) / Pr + 0.5, 0.0, 0.999));
      return mix(gnd(p), dec_at(acc_ink, centre).rgb, char_mark(round(rn * 15.0 / (lv - 1.0)), ruv, Pr));
    }
    // Beside the row: neither the rule's own ink nor a character a cell would make of it.
    if (dec_at(ink_t(p), p).acc > 0.5 && ink_t(p).a > 0.3) { return gnd(p); }
    let ca = cell_avg(cell, P);
    if (ca.a > 0.02 && dec_at(ca, centre).acc > 0.5) { return gnd(p); }
  }
  // The least characters of this cell and the four beside it that reach here (one moved onto a thin
  // line), within the nearness of ink only.
  var fl = vec4f(0.0);
  if (near_t(p).r > 0.0) {
    for (var k = 0; k < 5; k++) {
      let dk = select(select(vec2f(0.0, f32(k * 2 - 7)), vec2f(f32(k * 2 - 3), 0.0), k < 3), vec2f(0.0), k == 0);
      let f = charr_floor(p, cell + dk, P, levels);
      if (f.w > fl.w) { fl = f; }
    }
  }
  // Characters only where the ink is really loose (its own looseness: text to read keeps its island
  // above, and the cells round it are whole characters).
  if (kt_loose_own(centre) <= 0.1 + 0.4 * h1(cell + 1.3)) { return mix(exact(p), fl.rgb, fl.w); }
  // A cell over a picture's edge is not made into characters: the picture keeps its own edge.
  let o0 = cell * P - anc;
  for (var j = 0; j < 3; j++) { for (var i = 0; i < 3; i++) {
    if (near_t(o0 + (vec2f(f32(i), f32(j)) * 0.5) * P).g > 0.05) { return exact(p); }
  } }
  let avg = cell_avg(cell, P);
  let d = dec_at(avg, centre);
  var n = clamp(floor(avg.a * levels), 0.0, levels - 1.0);
  // Toward a held island the characters grow denser, a level at a time, up to the densest by `r0`.
  let hc = hold_at(centre);
  if (hc > 0.0 && avg.a > 0.02) { n = clamp(round(mix(n, levels - 1.0, hc)), 0.0, levels - 1.0); }
  var m = -1.0;
  // On a 3D element (DESIGN.md §1.18): density by the light on it, and on its edges and slanted faces a
  // stroke (| / - \) along the edge or across the slope, by the projected normal, as often as
  // `ascii_normal_weight` says.
  let code = round(near_t(centre).g * 255.0);
  if (code >= 1.0 && code <= 9.0) {
    n = clamp(floor(clamp(d.tone + d.acc, 0.0, 1.0) * avg.a * levels), 0.0, levels - 1.0);
    let wgt = clamp(a0.w, 0.0, 1.0);
    let sd = sdist(centre);
    var ang = -1.0;
    if (abs(sd) < P * 0.75) {
      // On an edge: along it (across the nearness's gradient).
      let g = sgrad(centre);
      if (length(g) > 1e-3) { ang = atan2(g.y, g.x) + 1.5707963; }
    } else if (code >= 2.0 && sd > 0.0) {
      // A face turned away: across its slope (the normal's direction turned a quarter).
      ang = (code - 2.0 + 0.5) / 8.0 * 3.14159265 + 1.5707963;
    }
    if (ang > -0.5 && h1(cell + 5.1) < wgt) {
      // The nearest of the four families, drawn as a line through the cell one stroke wide.
      let fam = round(fract(ang / 3.14159265) * 4.0) % 4.0;
      let dir = vec2f(cos(fam * 0.7853982), sin(fam * 0.7853982));
      let q = fract(lat(p) / P) - 0.5;
      let dist = abs(q.x * dir.y - q.y * dir.x) * P;
      m = clamp(max(1.0, P * 0.14) - dist + 0.5, 0.0, 1.0) * step(abs(dot(q, dir)), 0.42) * step(0.02, avg.a);
    }
  }
  // A cell sampled under the floor with ink in it has its least character on the line instead (`fl`).
  if (m < 0.0 && avg.a < INK_FLOOR && !(hc > 0.0 && avg.a > 0.02) && cell_ink(centre, P * 0.5).z > 0.0) { m = 0.0; }
  if (m < 0.0) { m = char_mark(round(n * 15.0 / (levels - 1.0)), fract(lat(p) / P), P); }
  var colour = select(d.rgb, u.fg.rgb, a0.z >= 0.5);
  if (fl.w > m) { return mix(gnd(p), select(fl.rgb, u.fg.rgb, a0.z >= 0.5), fl.w); }
  return mix(gnd(p), colour, m);
}
/// The least character of the cell `cell` at pitch `P` where it covers `p` (DESIGN.md §1.6): in a loose
/// cell with ink in it sampled under `INK_FLOOR` (a thin line's), the lightest character that
/// is not blank, its ink centred on the line's centreline across the line, as a thin accent rule's row is
/// (§1.6.2), in the line's colour (`w`: how much of `p` it covers).
fn charr_floor(p: vec2f, cell: vec2f, P: f32, levels: f32) -> vec4f {
  let centre = (cell + 0.5) * P - anc;
  if (kt_loose_own(centre) <= 0.1 + 0.4 * h1(cell + 1.3)) { return vec4f(0.0); }
  let avg = cell_avg(cell, P);
  if (avg.a >= INK_FLOOR || (hold_at(centre) > 0.0 && avg.a > 0.02)) { return vec4f(0.0); }
  let e = cell_ink(centre, P * 0.5);
  if (e.z <= 0.0) { return vec4f(0.0); }
  // Across the line: of the two runs of ink through that pixel, across and down, the shorter, to its
  // middle (the line's centreline).
  var run = vec4f(0.0);
  for (var a = 0; a < 4; a++) {
    let dir = select(select(vec2f(0.0, -1.0), vec2f(0.0, 1.0), a == 3), select(vec2f(-1.0, 0.0), vec2f(1.0, 0.0), a == 1), a < 2);
    for (var k = 1; k <= 8; k++) { if (ink_t(e.xy + dir * f32(k)).a < 0.5) { break; } run[a] = f32(k); }
  }
  let down = run.z + run.w <= run.x + run.y;
  let ax = select(vec2f(1.0, 0.0), vec2f(0.0, 1.0), down);
  let lo = select(run.x, run.z, down);
  let hi = select(run.y, run.w, down);
  let mid = dot(e.xy, ax) + (hi - lo) * 0.5;
  // The character's ink box in its tile (`marks::INK_BOX_ROW`), and where its tile goes: the cell's, moved
  // across the line by whole pixels so the box's centre is on the centreline.
  let ir = i32(((CHAR_MAX + 1.0) * CHAR_MAX - CHAR_MIN * (CHAR_MIN - 1.0)) * 0.5);
  let bx = i32(4.0 * (P - CHAR_MIN));
  let b = vec4f(textureLoad(chars, vec2i(bx, ir), 0).r, textureLoad(chars, vec2i(bx + 1, ir), 0).r, textureLoad(chars, vec2i(bx + 2, ir), 0).r, textureLoad(chars, vec2i(bx + 3, ir), 0).r) * 255.0;
  let o0 = cell * P - anc;
  // Ink 8 px across or more is no thin line: the character stays in its cell.
  var o2 = select(vec2f(o0.x, round(mid - 0.5 * (b.y + b.w))), vec2f(round(mid - 0.5 * (b.x + b.z)), o0.y), ax.x > 0.5);
  if (lo >= 8.0 || hi >= 8.0) { o2 = o0; } else {
    // At the line's end (in sight along it), the character's ink is kept within the line's length: past
    // it the ground does not know the line is loose and the character would be cut.
    let al = vec2f(1.0) - ax;
    let b0 = dot(b.xy, al);
    let b1 = dot(b.zw, al);
    let r0 = select(run.x, run.z, !down);
    let r1 = select(run.y, run.w, !down);
    var oa = dot(o2, al);
    if (r0 < 8.0) { oa = max(oa, ceil(dot(e.xy, al) - 0.5 - r0 - b0)); }
    if (r1 < 8.0) { oa = min(oa, floor(dot(e.xy, al) + 0.5 + r1 - b1)); }
    o2 = o2 * ax + oa * al;
  }
  let t = floor(p) - o2;
  if (any(t < vec2f(0.0)) || any(t >= vec2f(P))) { return vec4f(0.0); }
  let row = (P * (P - 1.0) - CHAR_MIN * (CHAR_MIN - 1.0)) * 0.5;
  let m = textureLoad(chars, vec2i(i32(P + t.x), i32(row + t.y)), 0).r;
  return vec4f(dec(e.xy).rgb, m);
}
