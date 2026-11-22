//! Thin lines under the mark materials (DESIGN.md §1.6): a loose cell of `dottr` or `charr` with ink in it,
//! sampled below `INK_FLOOR`, is drawn with the least mark (a dot of that ink; the lightest character that
//! is not blank, its ink centred on the line), so a 1 px line at looseness 1 is a row of marks at every
//! offset from the cells, a mark in each cell it crosses. A cell sampled at the floor or more is as it was,
//! so a solid square changes only on its edge; `halftonr` has no floor (the line stays whole under its
//! screens) and is as it was.
use amenui::el::{El, Paint, col, space};
use amenui::offscreen::Offscreen;
use amenui::style::{DARK, LIGHT, Look, Material, Off, Theme};
use amenui::ui::Ui;
use amenui::widgets::{Stack, StackLook, StackMsg, card_stack};

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }
const W: u32 = 320;
const H: u32 = 320;
/// The cell at looseness 1 with the default numbers, device px at 1x: dottr's pitch (5) opened 1.6 times,
/// charr's (10) 1.5 times, halftonr's (7) 1.5 times.
fn cell_of(mat: Material) -> f32 { match mat { Material::Dottr => 8.0, Material::Charr => 15.0, _ => 10.5 } }

#[derive(Clone)]
struct S { mat: Material, light: bool, scale: f32, at: (f32, f32), size: (f32, f32) }
fn view(s: &S) -> El<()> {
    let k = 1.0 / s.scale;
    col().key("page").fill_width().fill_height()
        .child(space(s.size.0 * k, s.size.1 * k).key("ink").fill(Paint::INK).loose(1.0).absolute(Some(s.at.0 * k), Some(s.at.1 * k), None, None))
}
fn theme(light: bool) -> Theme { if light { LIGHT } else { DARK } }
/// The page drawn: device px, a shape of `size` at `at`, at looseness 1, the ground's own marks off.
fn shot(o: &mut Offscreen, s: S) -> Vec<u8> {
    let (w, h, sc) = ((W as f32 * s.scale) as u32, (H as f32 * s.scale) as u32, s.scale);
    let mut u = Ui::new(s, view, |_, _| {}, fonts(), w, h, sc)
        .on_look(|s| Look { material: s.mat, theme: theme(s.light), off: Off { ground: true, ..Off::default() }, ..Look::default() });
    u.pointer_leave();
    u.tick(2.0);
    u.settle();
    let l = u.look();
    o.render(&mut u, &l)
}
fn ground(light: bool) -> [u8; 3] { theme(light).ground.map(|v| (v * 255.0).round() as u8) }
/// Pixels off the ground.
fn inked(f: &[u8], g: [u8; 3]) -> Vec<bool> { f.chunks_exact(4).map(|p| (0..3).any(|k| p[k].abs_diff(g[k]) > 2)).collect() }
fn pair(scale: f32) -> Option<(Offscreen, Offscreen)> {
    let (w, h) = ((W as f32 * scale) as u32, (H as f32 * scale) as u32);
    let (Ok(mut o), Ok(mut before)) = (Offscreen::try_new(w, h), Offscreen::try_new(w, h)) else { return None };
    o.r.partial = false;
    before.r.partial = false;
    before.r.checks(false, true);
    Some((o, before))
}

/// Check 3 (dottr, charr): a 1 px line (2 device px at 2x: the layout is in whole px) 200 px long, across and
/// down, at every whole px offset in a cell: a mark in every cell it crosses, as many cells marked at each
/// offset to one, the marks in one row (column); charr's least characters centred on the line (2 device px)
/// and whole (90% of the most ink one has).
#[test]
fn a_thin_line_is_a_row_of_marks_at_every_offset() {
    for scale in [1.0f32, 2.0] {
        let Some((mut o, mut before)) = pair(scale) else { eprintln!("no GPU adapter here: skipped"); return };
        let w = (W as f32 * scale) as usize;
        for light in [false, true] {
            let g = ground(light);
            for mat in [Material::Dottr, Material::Charr] {
                let p = cell_of(mat) * scale;
                for across in [false, true] {
                    let name = format!("{} {mat:?} at {scale}x, a {} line", theme(light).name, if across { "vertical" } else { "horizontal" });
                    let (mut counts, mut whole, mut parts) = (vec![], 0usize, vec![]);
                    for off in (0..p as u32).step_by(scale as usize) {
                        let (a0, a1, c0) = (40.0 * scale, 240.0 * scale, 40.0 * scale + off as f32);
                        let s = S { mat, light, scale, at: if across { (c0, a0) } else { (a0, c0) }, size: if across { (scale, a1 - a0) } else { (a1 - a0, scale) } };
                        let on = inked(&shot(&mut o, s.clone()), g);
                        let was = inked(&shot(&mut before, s), g);
                        // Along and across the line, for each pixel.
                        let at = |i: usize| { let (x, y) = ((i % w) as f32, (i / w) as f32); if across { (y, x) } else { (x, y) } };
                        let first = (a0 / p).floor() as i32;
                        let last = ((a1 - 1.0) / p).floor() as i32;
                        let mut cells: std::collections::BTreeMap<i32, (f32, f32, usize)> = Default::default();
                        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                        for i in (0..on.len()).filter(|&i| on[i]) {
                            let (al, ac) = at(i);
                            let c = cells.entry((al / p).floor() as i32).or_insert((f32::MAX, f32::MIN, 0));
                            *c = (c.0.min(ac), c.1.max(ac + 1.0), c.2 + 1);
                            lo = lo.min(ac);
                            hi = hi.max(ac + 1.0);
                        }
                        for c in first..=last { assert!(cells.contains_key(&c), "{name}, offset {off}: no mark in cell {c} ({first}..={last})"); }
                        assert!(hi - lo <= p, "{name}, offset {off}: the marks are {} px across, more than a row of {p}", hi - lo);
                        counts.push(cells.len());
                        if mat == Material::Charr {
                            // The cells drawn before only by the floor: their marks centred on the line.
                            let mid = c0 + scale * 0.5;
                            let was_cells: std::collections::BTreeSet<i32> = (0..was.len()).filter(|&i| was[i]).map(|i| (at(i).0 / p).floor() as i32).collect();
                            for (c, &(l, h, n)) in &cells {
                                if was_cells.contains(c) || *c < first || *c > last { continue; }
                                let centre = (l + h) * 0.5;
                                assert!((centre - mid).abs() <= 2.0, "{name}, offset {off}: cell {c}'s character is centred {centre}, the line {mid}");
                                whole = whole.max(n);
                                parts.push((off, *c, n));
                            }
                        }
                    }
                    let (least, most) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
                    eprintln!("{name}: {least} to {most} cells marked");
                    assert!(most - least <= 1, "{name}: {least} to {most} cells marked");
                    for (off, c, n) in parts { assert!(n * 10 >= whole * 9, "{name}, offset {off}: cell {c}'s character has {n} of {whole} px"); }
                }
            }
        }
    }
}

/// Check 3 (halftonr): the floor is not used, the frame is as it was; at every offset the line is whole
/// (every pixel of it ink, 24/255 off the ground in some channel, the screens printed over it or not) and
/// the screens' dots beside it lie within a pitch of it.
#[test]
fn a_thin_line_under_halftonr_stays_whole_under_its_screens() {
    for scale in [1.0f32, 2.0] {
        let Some((mut o, mut before)) = pair(scale) else { eprintln!("no GPU adapter here: skipped"); return };
        let w = (W as f32 * scale) as usize;
        for light in [false, true] {
            let th = theme(light);
            let g = ground(light);
            let p = cell_of(Material::Halftonr) * scale;
            let (mut far, mut least) = (0.0f32, 255u8);
            for across in [false, true] {
                let name = format!("{} halftonr at {scale}x, a {} line", th.name, if across { "vertical" } else { "horizontal" });
                for off in (0..p as u32).step_by(scale as usize) {
                    let (a0, a1, c0) = (40.0 * scale, 240.0 * scale, 40.0 * scale + off as f32);
                    let s = S { mat: Material::Halftonr, light, scale, at: if across { (c0, a0) } else { (a0, c0) }, size: if across { (scale, a1 - a0) } else { (a1 - a0, scale) } };
                    let f = shot(&mut o, s.clone());
                    assert!(f == shot(&mut before, s), "{name}, offset {off}: not as it was");
                    let at = |i: usize| { let (x, y) = ((i % w) as f32, (i / w) as f32); if across { (y, x) } else { (x, y) } };
                    for i in 0..f.len() / 4 {
                        let (al, ac) = at(i);
                        let px = &f[i * 4..i * 4 + 3];
                        if al >= a0 && al < a1 && ac >= c0 && ac < c0 + scale {
                            least = least.min((0..3).map(|k| px[k].abs_diff(g[k])).max().unwrap_or(0));
                        } else if (0..3).any(|k| px[k].abs_diff(g[k]) > 2) {
                            far = far.max(if ac < c0 { c0 - ac - 1.0 } else { ac - (c0 + scale) + 1.0 });
                        }
                    }
                }
            }
            eprintln!("{} halftonr at {scale}x: dots beside the line reach {far} px from it (a pitch: {p}); the line {least}/255 off the ground at the least", th.name);
            assert!(least >= 24, "{} halftonr at {scale}x: a pixel of the line is {least}/255 off the ground, not ink", th.name);
            assert!(far <= p, "{} halftonr at {scale}x: dots beside the line reach {far} px from it, past a pitch of {p}", th.name);
        }
    }
}

/// Check 5: a solid square of 6 cells a side at looseness 1 is as it was but in the cells its edge crosses
/// (and those beside them that their least marks reach).
#[test]
fn a_solid_square_changes_only_on_its_edge() {
    for scale in [1.0f32, 2.0] {
        let Some((mut o, mut before)) = pair(scale) else { eprintln!("no GPU adapter here: skipped"); return };
        let w = (W as f32 * scale) as usize;
        for light in [false, true] {
            for mat in [Material::Dottr, Material::Charr, Material::Halftonr] {
                let p = cell_of(mat) * scale;
                let (x0, side) = (40.0 * scale, (6.0 * cell_of(mat)).ceil() * scale);
                let s = S { mat, light, scale, at: (x0, x0), size: (side, side) };
                let (a, b) = (shot(&mut o, s.clone()), shot(&mut before, s));
                let mut changed = 0;
                for i in 0..a.len() / 4 {
                    if a[i * 4..i * 4 + 3] == b[i * 4..i * 4 + 3] { continue; }
                    changed += 1;
                    let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
                    // How far inside the square's edge (negative outside).
                    let inside = (x - x0).min(y - x0).min(x0 + side - x).min(x0 + side - y);
                    assert!(inside < 1.5 * p + 1.0, "{} {mat:?} at {scale}x: a pixel {inside} px inside the square changed, at {x}, {y}", theme(light).name);
                }
                eprintln!("{} {mat:?} at {scale}x: {changed} px on the square's edge changed", theme(light).name);
            }
        }
    }
}

/// Check 4: a card stack fanning under dottr and charr: along the cards' edges no two cells beside each other
/// are both without a mark.
#[test]
fn a_fanning_cards_edges_have_marks_all_along() {
    #[derive(Clone)]
    struct C { deck: Stack, mat: Material, light: bool }
    fn view(s: &C) -> El<()> { col().key("page").width(360.0).height(240.0).pad(20.0, 20.0).child(card_stack("cs", &s.deck, 320.0, 200.0, |_| ())) }
    for scale in [1.0f32, 2.0] {
        let (w, h) = ((360.0 * scale) as u32, (240.0 * scale) as u32);
        let (Ok(mut o), Ok(mut p), Ok(mut before)) = (Offscreen::try_new(w, h), Offscreen::try_new(w, h), Offscreen::try_new(w, h)) else { eprintln!("no GPU adapter here: skipped"); return };
        for o in [&mut o, &mut p, &mut before] { o.r.partial = false; }
        before.r.checks(false, true);
        for light in [false, true] {
            let g = ground(light);
            for mat in [Material::Dottr, Material::Charr] {
                let frame = |o: &mut Offscreen, mat: Material, look: StackLook| {
                    let mut deck = Stack::default();
                    deck.look = look;
                    let mut u = Ui::new(C { deck, mat, light }, view, |_, _| {}, fonts(), w, h, scale)
                        .on_look(|s| Look { material: s.mat, theme: theme(s.light), off: Off { ground: true, ..Off::default() }, ..Look::default() })
                        .on_tick(|s, t| s.deck.step(t));
                    u.pointer_leave();
                    u.tick(1.0);
                    u.state.deck.update(StackMsg::Press(Some(0)), 1.0);
                    u.relayout();
                    let mut t = 1.0;
                    // Mid-fan: 200 ms in, the cards at looseness 1 and still turning.
                    while t < 1.2 { t += 1.0 / 60.0; u.tick(t); u.frame_drawn(); }
                    assert!(look == StackLook::Plain || u.state.deck.looseness() == 1.0);
                    u.settle();
                    let l = u.look();
                    o.render(&mut u, &l)
                };
                let (mf, pf) = (frame(&mut o, mat, StackLook::Marks(mat)), frame(&mut p, Material::Plain, StackLook::Plain));
                if let Ok(d) = std::env::var("THIN_DUMP") { for (n, f) in [("marks", &mf), ("plain", &pf)] { amenui::offscreen::save_png(&format!("{d}/card-{mat:?}-{scale}-{light}-{n}.png"), w, h, f); } }
                let marks = inked(&mf, g);
                // The cards: pixels at least half covered by them (half way from the ground to the raised grey).
                let raised = theme(light).greys.raised.map(|v| (v * 255.0).round() as i32);
                let far = |c: &[u8]| (0..3).map(|k| (c[k] as i32 - g[k] as i32).abs()).max().unwrap_or(0);
                let half = (0..3).map(|k| (raised[k] - g[k] as i32).abs()).max().unwrap_or(0) / 2;
                let cards: Vec<bool> = pf.chunks_exact(4).map(|c| far(c) >= half.max(3)).collect();
                let pc = cell_of(mat) * scale;
                let (wi, hi) = (w as i32, h as i32);
                let edge = |i: usize| { let (x, y) = ((i as i32) % wi, (i as i32) / wi); cards[i] && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| { let (a, b) = (x + dx, y + dy); a < 0 || b < 0 || a >= wi || b >= hi || !cards[(b * wi + a) as usize] }) };
                let cell = |i: usize| (((i as i32 % wi) as f32 / pc).floor() as i32, ((i as i32 / wi) as f32 / pc).floor() as i32);
                let edges: std::collections::BTreeSet<(i32, i32)> = (0..cards.len()).filter(|&i| edge(i)).map(cell).collect();
                // A cell is marked when a mark lies in it or within half a cell of it: the mark material moves
                // what is loose by a few px (the field map's flow), so the cards' edges are not exactly plain's.
                let empties = |marks: &[bool]| -> Vec<(i32, i32)> {
                    edges.iter().copied().filter(|&(cx, cy)| {
                        let (x0, y0) = (((cx as f32 - 0.5) * pc).max(0.0) as i32, ((cy as f32 - 0.5) * pc).max(0.0) as i32);
                        let (x1, y1) = (((((cx + 1) as f32 + 0.5) * pc) as i32).min(wi), ((((cy + 1) as f32 + 0.5) * pc) as i32).min(hi));
                        !(y0..y1).any(|y| (x0..x1).any(|x| marks[(y * wi + x) as usize]))
                    }).collect()
                };
                let empty = empties(&marks);
                let was = empties(&inked(&frame(&mut before, mat, StackLook::Marks(mat)), g)).len();
                let name = format!("{} {mat:?} at {scale}x", theme(light).name);
                eprintln!("{name}: {} of {} cells on the cards' edges without a mark ({was} without the floor)", empty.len(), edges.len());
                for &(x, y) in &empty {
                    let next = empty.iter().find(|&&(a, b)| (a, b) != (x, y) && (a - x).abs() <= 1 && (b - y).abs() <= 1);
                    assert!(next.is_none(), "{name}: cells {:?} and {:?} on the cards' edges are both without a mark", (x, y), next.unwrap());
                }
            }
        }
    }
}

/// charr: a least character centred on a vertical line beside a column of ordinary characters (a solid
/// square's edge, the line a px to a cell from it) shares at most a tenth of its width with the character
/// beside it.
#[test]
fn a_least_character_beside_an_ordinary_one_keeps_clear_of_it() {
    #[derive(Clone)]
    struct B { light: bool, scale: f32, line: Option<f32> }
    fn view(s: &B) -> El<()> {
        let k = 1.0 / s.scale;
        let sq = 90.0 * s.scale;
        let mut c = col().key("page").fill_width().fill_height()
            .child(space(sq * k, sq * k).key("sq").fill(Paint::INK).loose(1.0).absolute(Some(30.0), Some(30.0), None, None));
        if let Some(x) = s.line { c = c.child(space(1.0, 120.0).key("line").fill(Paint::INK).loose(1.0).absolute(Some(30.0 + x * k), Some(20.0), None, None)); }
        c
    }
    for scale in [1.0f32, 2.0] {
        let (w, h) = ((W as f32 * scale) as u32, (H as f32 * scale) as u32);
        let Ok(mut o) = Offscreen::try_new(w, h) else { eprintln!("no GPU adapter here: skipped"); return };
        o.r.partial = false;
        let p = cell_of(Material::Charr) * scale;
        for light in [false, true] {
            let g = ground(light);
            let mut shot = |line: Option<f32>| {
                let mut u = Ui::new(B { light, scale, line }, view, |_, _| {}, fonts(), w, h, scale)
                    .on_look(|s| Look { material: Material::Charr, theme: theme(s.light), off: Off { ground: true, ..Off::default() }, ..Look::default() });
                u.pointer_leave();
                u.tick(2.0);
                u.settle();
                let l = u.look();
                inked(&o.render(&mut u, &l), g)
            };
            let alone = shot(None);
            let (wi, edge) = (w as usize, 30.0 * scale + 90.0 * scale);
            let (mut most, mut seen) = (0.0f32, 0);
            for d in (1..p as u32).step_by(scale as usize) {
                let with = shot(Some(90.0 * scale + d as f32));
                // Per row of cells over the square: the line's characters (what the line added) and the
                // square's last column of characters, by the columns they ink.
                for r in 0..((90.0 * scale / p) as usize) {
                    let (y0, y1) = ((30.0 * scale + r as f32 * p) as usize, (30.0 * scale + (r + 1) as f32 * p) as usize);
                    let cols = |f: &dyn Fn(usize) -> bool| -> std::collections::BTreeSet<usize> { (y0..y1).flat_map(|y| (0..wi).map(move |x| (x, y))).filter(|&(x, y)| f(y * wi + x)).map(|(x, _)| x).collect() };
                    let fl = cols(&|i| with[i] && !alone[i] && (i % wi) as f32 >= edge - p);
                    let ord = cols(&|i| alone[i] && (i % wi) as f32 >= edge - p && ((i % wi) as f32) < edge + p);
                    if fl.is_empty() || ord.is_empty() { continue; }
                    let (a, b) = (*fl.first().unwrap(), *fl.last().unwrap());
                    let shared = ord.iter().filter(|&&x| x >= a && x <= b).count();
                    seen += 1;
                    most = most.max(shared as f32 / (b + 1 - a) as f32);
                }
            }
            eprintln!("{} charr at {scale}x: a least character on a vertical line shares {:.0}% of its width with the character beside it at the most, in {seen} rows", theme(light).name, most * 100.0);
            assert!(seen > 10, "{} charr at {scale}x: {seen} rows with a least character beside an ordinary one", theme(light).name);
            assert!(most <= 0.1, "{} charr at {scale}x: {:.0}% of a least character's width is shared with the one beside it", theme(light).name, most * 100.0);
        }
    }
}
