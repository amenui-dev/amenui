//! A card stack's outlines, card by card (`Stack::outlines`, `MeshInstances::outline`): by default every
//! card is raised, the top one outlined in the ink and the ones under it in the hairline grey, each drawn
//! exactly in its grey; a turned card's outline is a smooth line.
use amenui::el::{El, Paint, col};
use amenui::offscreen::Offscreen;
use amenui::style::{DARK, LIGHT, Look, Material};
use amenui::ui::Ui;
use amenui::widgets::{Stack, StackMsg, card_stack};

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }
const W: u32 = 360;
const H: u32 = 240;
#[derive(Clone)]
struct S { deck: Stack, light: bool }
fn view(s: &S) -> El<()> { col().key("page").width(W as f32).height(H as f32).pad(20.0, 20.0).child(card_stack("cs", &s.deck, 320.0, 200.0, |_| ())) }
/// The stack fanned under its top card, so each card's edge shows.
fn fanned(outlines: Vec<Paint>) -> Stack {
    let mut d = Stack::default();
    d.outlines = outlines;
    d.update(StackMsg::Hover(Some(0)), 0.0);
    let mut t = 0.0;
    while d.moving() && t < 5.0 { t += 1.0 / 60.0; d.step(t); }
    d
}
/// Pixels within 1 of `c` in the stack's box.
fn count(f: &[u8], c: [f32; 3]) -> usize {
    let c = c.map(|v| (v * 255.0).round() as i32);
    f.chunks_exact(4).filter(|p| (0..3).all(|k| (p[k] as i32 - c[k]).abs() <= 1)).count()
}

#[test]
fn the_top_card_is_outlined_in_ink_and_the_ones_under_it_in_hairline() {
    assert_eq!((Stack::default().paints, Stack::default().outlines), (vec![Paint::RAISED], vec![Paint::INK, Paint::HAIRLINE]));
    let mut o = Offscreen::new(W, H);
    for light in [false, true] {
        let th = if light { LIGHT } else { DARK };
        let shot = |o: &mut Offscreen, deck: Stack| {
            let mut u = Ui::new(S { deck, light }, view, |_, _| {}, fonts(), W, H, 1.0)
                .on_look(|s| Look { material: Material::Plain, theme: if s.light { LIGHT } else { DARK }, ..Look::default() });
            u.tick(2.0);
            u.settle();
            let l = u.look();
            o.r.partial = false;
            o.render(&mut u, &l)
        };
        // The cards under the top one, turned, have whole pixels of their outline: exactly the grey asked.
        let both = shot(&mut o, fanned(vec![Paint::INK, Paint::HAIRLINE]));
        let under_ink = shot(&mut o, fanned(vec![Paint::HAIRLINE, Paint::INK]));
        let (bh, ui) = (count(&both, th.greys.hairline), count(&under_ink, th.ink));
        eprintln!("{}: hairline under {bh} hairline px; ink under {ui} ink px", th.name);
        assert!(bh > 150, "{}: the cards under the top one in the hairline grey: {bh} px", th.name);
        assert!(ui > 100, "{}: asked for ink, the cards under the top one are the ink: {ui} px", th.name);
        // The top card (square to the pixels) alone changes with its own
        // grey: in ink it is nearer the ink than in hairline wherever they differ.
        let top_h = shot(&mut o, fanned(vec![Paint::HAIRLINE, Paint::HAIRLINE]));
        let ink = th.ink.map(|v| (v * 255.0).round() as i32);
        let off_ink = |p: &[u8]| (0..3).map(|k| (p[k] as i32 - ink[k]).abs()).sum::<i32>();
        let diff: Vec<usize> = (0..both.len() / 4).filter(|&i| both[i * 4..i * 4 + 3] != top_h[i * 4..i * 4 + 3]).collect();
        assert!(diff.len() > 100, "{}: the top card's outline changes with its grey: {} px", th.name, diff.len());
        assert!(diff.iter().all(|&i| off_ink(&both[i * 4..i * 4 + 3]) < off_ink(&top_h[i * 4..i * 4 + 3])), "{}: the top card's ink outline is toward the ink", th.name);
        assert!(count(&both, th.greys.raised) > 4000, "{}: the cards are raised", th.name);
    }
}

#[test]
fn a_turned_cards_outline_is_a_smooth_line() {
    // Along the turned cards' upper left edge, each row: the ground, then a pixel partly toward the
    // hairline (the edge's share of it). Where the edge falls in a row, from that share, lies on a straight
    // line to within 0.45 px; steps of whole texels are 0.5 px off it at the least (1.1 at the old 0.5).
    let mut o = Offscreen::new(W, H);
    for light in [false, true] {
        let th = if light { LIGHT } else { DARK };
        let mut u = Ui::new(S { deck: fanned(vec![Paint::INK, Paint::HAIRLINE]), light }, view, |_, _| {}, fonts(), W, H, 1.0)
            .on_look(|s| Look { material: Material::Plain, theme: if s.light { LIGHT } else { DARK }, ..Look::default() });
        u.tick(2.0);
        u.settle();
        let l = u.look();
        o.r.partial = false;
        let f = o.render(&mut u, &l);
        let (g, h) = ((th.ground[0] * 255.0).round(), (th.greys.hairline[0] * 255.0).round());
        let at = |x: u32, y: u32| f[((y * W + x) * 4) as usize] as f32;
        let (mut edge, mut soft) = (vec![], 0);
        for y in 40..70u32 {
            let x = (0..W).find(|&x| (at(x, y) - g).abs() > 1.0).expect("the card");
            let share = ((at(x, y) - g) / (h - g)).clamp(0.0, 1.0);
            if share < 0.95 { soft += 1; }
            edge.push((y as f32, x as f32 + 1.0 - share));
        }
        let n = edge.len() as f32;
        let (my, mx) = (edge.iter().map(|e| e.0).sum::<f32>() / n, edge.iter().map(|e| e.1).sum::<f32>() / n);
        let slope = edge.iter().map(|e| (e.0 - my) * (e.1 - mx)).sum::<f32>() / edge.iter().map(|e| (e.0 - my).powi(2)).sum::<f32>();
        let off = edge.iter().map(|e| (e.1 - (mx + slope * (e.0 - my))).abs()).fold(0.0, f32::max);
        assert!(soft >= 27, "{}: the edge's first pixel is its share of the line: {soft} of 30 rows", th.name);
        assert!(slope < -0.5 && off < 0.45, "{}: the edge is a straight line: slope {slope}, {off} px off it at most", th.name);
    }
}

/// Cards handed to a mark material (DESIGN.md §1.24): at rest the cards' pixels are those `plain` draws and
/// nothing asks for frames; from `LOOSEN_S` after the fan starts they are marks (a quarter of the cards'
/// inside differs from `plain` at least, under halftonr 15% and some in every window of 3 x 3 cells), and from `TIGHTEN_S` after the last spring stops they are drawn as
/// in `plain` again, and no frames are asked for.
#[test]
fn marked_cards_are_plain_at_rest_and_marks_only_while_they_fan() {
    use amenui::widgets::{LOOSEN_S, StackLook, TIGHTEN_S};
    #[derive(Clone)]
    struct M { deck: Stack, light: bool, mat: Material }
    fn view(s: &M) -> El<()> { col().key("page").width(W as f32).height(H as f32).pad(20.0, 20.0).child(card_stack("cs", &s.deck, 320.0, 200.0, |_| ())) }
    fn open(mat: Material, light: bool, scale: f32) -> Ui<M, ()> {
        let mut deck = Stack::default();
        deck.look = if mat == Material::Plain { StackLook::Plain } else { StackLook::Marks(mat) };
        let (w, h) = ((W as f32 * scale) as u32, (H as f32 * scale) as u32);
        let mut u = Ui::new(M { deck, light, mat }, view, |_, _| {}, fonts(), w, h, scale)
            .on_look(|s| Look { material: s.mat, theme: if s.light { LIGHT } else { DARK }, ..Look::default() })
            .on_tick(|s, t| s.deck.step(t)).on_wake(|s, t| s.deck.moving().then_some(t));
        u.pointer_leave();
        u
    }
    fn shot(o: &mut Offscreen, u: &mut Ui<M, ()>, t: f32) -> Vec<u8> { u.tick(t); u.settle(); let l = u.look(); let f = o.render(u, &l); u.frame_drawn(); f }
    for scale in [1.0f32, 2.0] {
        let (w, h) = ((W as f32 * scale) as u32, (H as f32 * scale) as u32);
        let (Ok(mut o), Ok(mut p)) = (Offscreen::try_new(w, h), Offscreen::try_new(w, h)) else { eprintln!("no GPU adapter here: skipped"); return };
        o.r.partial = false;
        p.r.partial = false;
        for light in [false, true] {
            let th = if light { LIGHT } else { DARK };
            let g = th.ground.map(|v| (v * 255.0).round() as u8);
            for mat in [Material::Dottr, Material::Charr, Material::Halftonr] {
                let (mut u, mut v) = (open(mat, light, scale), open(Material::Plain, light, scale));
                let name = format!("{} {mat:?} at {scale}x", th.name);
                // The cards' pixels: what `plain` draws other than the ground; their inside, 3 px from it.
                let cards = |f: &[u8]| -> Vec<bool> { f.chunks_exact(4).map(|q| q[..3].iter().zip(&g).any(|(a, b)| a.abs_diff(*b) > 1)).collect() };
                let inside = |c: &[bool]| -> Vec<usize> {
                    let (w, h) = (w as i32, h as i32);
                    (0..c.len()).filter(|&i| { let (x, y) = ((i as i32) % w, (i as i32) / w); (-3..=3).all(|d: i32| [(x + d, y), (x, y + d)].iter().all(|&(a, b)| a >= 0 && b >= 0 && a < w && b < h && c[(b * w + a) as usize])) }).collect()
                };
                let same_on = |a: &[u8], b: &[u8], c: &[bool]| c.iter().enumerate().filter(|(i, k)| **k && a[i * 4..i * 4 + 3] != b[i * 4..i * 4 + 3]).count();
                // At rest.
                let (a0, b0) = (shot(&mut o, &mut u, 1.5), shot(&mut p, &mut v, 1.5));
                let c0 = cards(&b0);
                assert!(c0.iter().filter(|k| **k).count() > 4000, "{name}: the cards are drawn");
                assert_eq!(same_on(&a0, &b0, &c0), 0, "{name}: at rest the cards' pixels are plain's");
                let a1 = shot(&mut o, &mut u, 4.0);
                assert!(a0 == a1, "{name}: at rest 1.5 s and 4 s are the same frame");
                assert!(!u.needs_frame(4.1), "{name}: at rest no frames");
                // Fanned (held under the top card), frame by frame.
                let t0 = 5.0;
                for ui in [&mut u, &mut v] { ui.tick(t0); ui.state.deck.update(StackMsg::Press(Some(0)), t0); ui.relayout(); }
                let mut t = t0;
                let mut stopped = None;
                let mut marked = 0;
                let mut least = 1.0f32;
                while t < t0 + 4.0 {
                    t += 1.0 / 60.0;
                    let (a, b) = (shot(&mut o, &mut u, t), shot(&mut p, &mut v, t));
                    let l = u.state.deck.looseness();
                    // The springs stopped where the looseness began to fall from 1.
                    if stopped.is_none() && l < 1.0 && t > t0 + LOOSEN_S + 0.02 { stopped = Some(t - (1.0 - l) * TIGHTEN_S); }
                    if t >= t0 + LOOSEN_S + 1e-3 && stopped.is_none() {
                        let ins = inside(&cards(&b));
                        let diff = ins.iter().filter(|&&i| a[i * 4..i * 4 + 3] != b[i * 4..i * 4 + 3]).count();
                        least = least.min(diff as f32 / ins.len() as f32);
                        marked += 1;
                        // Halftonr: the screen is over the whole card, not only a part of it: every window of
                        // 3 x 3 cells (its pitch at looseness 1) wholly inside the cards has pixels that differ.
                        if mat == Material::Halftonr && marked == 6 {
                            let k = (3.0 * 7.0 * 1.5 * scale).ceil() as usize;
                            let (w, h) = (w as usize, h as usize);
                            let mut is_in = vec![false; w * h];
                            for &i in &ins { is_in[i] = true; }
                            // Summed areas of the inside and of the pixels that differ from plain.
                            let sums = |f: &dyn Fn(usize) -> bool| {
                                let mut s = vec![0u32; (w + 1) * (h + 1)];
                                for y in 0..h { for x in 0..w { s[(y + 1) * (w + 1) + x + 1] = f(y * w + x) as u32 + s[y * (w + 1) + x + 1] + s[(y + 1) * (w + 1) + x] - s[y * (w + 1) + x]; } }
                                s
                            };
                            let si = sums(&|i| is_in[i]);
                            let sd = sums(&|i| is_in[i] && a[i * 4..i * 4 + 3] != b[i * 4..i * 4 + 3]);
                            let box_sum = |s: &[u32], x: usize, y: usize| s[(y + k) * (w + 1) + x + k] + s[y * (w + 1) + x] - s[y * (w + 1) + x + k] - s[(y + k) * (w + 1) + x];
                            let mut windows = 0;
                            for y in 0..h.saturating_sub(k) { for x in 0..w.saturating_sub(k) {
                                if box_sum(&si, x, y) as usize != k * k { continue; }
                                windows += 1;
                                assert!(box_sum(&sd, x, y) > 0, "{name} at {:.0} ms: the {k} px window at {x}, {y} inside the cards is plain's", (t - t0) * 1000.0);
                            } }
                            assert!(windows > 100, "{name}: {windows} windows of 3 x 3 cells inside the cards");
                        }
                    }
                    if let Some(s) = stopped && t >= s + TIGHTEN_S + 1e-3 {
                        let c = cards(&b);
                        assert_eq!(same_on(&a, &b, &c), 0, "{name} at {:.0} ms after the springs stopped: the cards' pixels are plain's", (t - s) * 1000.0);
                        assert!(!u.needs_frame(t + 0.1), "{name}: settled and tight: no frames");
                        break;
                    }
                }
                eprintln!("{name}: {:.1}% of the cards' inside differs from plain at the least, in {marked} frames", least * 100.0);
                // Halftonr's dots at full ink meet, so a card's inside differs only between them: 15% at the
                // least (measured 19-28%), and every window of 3 x 3 cells has some (above).
                let floor = if mat == Material::Halftonr { 0.15 } else { 0.25 };
                assert!(least >= floor, "{name}: {:.1}% of the cards' inside differs from plain at the least", least * 100.0);
                assert!(marked >= 10 && stopped.is_some(), "{name}: {marked} frames of marks, the springs stopped at {stopped:?}");
            }
        }
    }
}

/// At rest the top card's outline is on whole pixels: one pixel at the ink on every side (within 2/255),
/// and no pixel part way to it beside it, dark and light. In a box the size of the site's stack at 1x and
/// 2x, the box at a whole logical px and at a quarter, a half and three quarters past one; and as the
/// site draws it in a narrow window, made 520 × 340 and zoomed (as its page's `El::fit` does) to 334 px across at
/// (192, 17), at 2x.
#[test]
fn at_rest_the_top_cards_outline_is_one_whole_pixel_on_every_side() {
    const BW: f32 = 520.0;
    const BH: f32 = 340.0;
    #[derive(Clone)]
    struct R { light: bool, ox: f32, zoomed: bool }
    fn rest(s: &R) -> El<()> {
        let stack = card_stack("cs", &Stack::default(), BW, BH, |_| ());
        if s.zoomed {
            return col().key("page").pad(192.0, 17.0).child(stack.zoom(334.0 / BW)).key(if s.light { "light" } else { "dark" });
        }
        let mut c = col().key("page").pad(20.0, 20.0).child(stack).key(if s.light { "light" } else { "dark" });
        c.style.padding.left = taffy::prelude::length(20.0 + s.ox);
        c
    }
    let deck = Stack::default();
    let Some([x0, y0, x1, y1]) = deck.scene().spec.snap else { panic!("a flat stack puts its top card on whole pixels") };
    let mut cases: Vec<(f32, f32, bool, (u32, u32))> = [1.0f32, 2.0].into_iter().flat_map(|s| [0.0f32, 0.25, 0.5, 0.75].map(move |o| (s, o, false, (((BW + 40.0) * s) as u32, ((BH + 40.0) * s) as u32)))).collect();
    cases.push((2.0, 0.0, true, (1436, 556)));
    for (scale, ox, zoomed, (fw, fh)) in cases {
        let mut o = Offscreen::new(fw, fh);
        let name = if zoomed { "zoomed".to_owned() } else { format!("+{ox}") };
        for light in [false, true] {
            let th = if light { LIGHT } else { DARK };
            let mut u = Ui::new(R { light, ox, zoomed }, rest, |_, _| {}, fonts(), fw, fh, scale)
                .on_look(|s| Look { material: Material::Plain, theme: if s.light { LIGHT } else { DARK }, ..Look::default() });
            u.tick(2.0);
            u.settle();
            // The card's edges in device px: where the straight-on camera puts them in the box as drawn
            // (on whole device px), each on the nearest whole one.
            let b = u.node("cs").expect("the stack").inner;
            if zoomed { assert!((b.w - 334.0).abs() < 0.5 && (b.x - 192.0).abs() < 0.5 && (b.y - 17.0).abs() < 0.5, "the box as the site has it: {b:?}"); }
            let (rx, ry) = ((b.x * scale).round(), (b.y * scale).round());
            let (rw, rh) = ((b.right() * scale).round() - rx, (b.bottom() * scale).round() - ry);
            let (hw, hh) = (1.25 * 0.5 * rw / rh, 1.25 * 0.5);
            let ex = |x: f32| (rx + (0.5 + 0.5 * x / hw) * rw).round() as i32;
            let ey = |y: f32| (ry + (0.5 - 0.5 * y / hh) * rh).round() as i32;
            let (l, r, t, bt) = (ex(x0), ex(x1), ey(y1), ey(y0));
            let lk = u.look();
            o.r.partial = false;
            let f = o.render(&mut u, &lk);
            let ink = th.ink.map(|v| (v * 255.0).round() as i32);
            let raised = th.greys.raised.map(|v| (v * 255.0).round() as i32);
            let px_at = |x: i32, y: i32| { let i = ((y as u32 * fw + x as u32) * 4) as usize; [f[i] as i32, f[i + 1] as i32, f[i + 2] as i32] };
            let off = |p: [i32; 3]| (0..3).map(|c| (p[c] - ink[c]).abs()).max().unwrap_or(0);
            let span = (0..3).map(|c| (raised[c] - ink[c]).abs()).max().unwrap_or(1).max(1);
            // Across each edge (the outline is the pixel inside it), 4 device px either way, along a line
            // through the middle of the side.
            let (mx, my) = ((l + r) / 2, (t + bt) / 2);
            let sides: [(&str, Vec<(i32, i32)>); 4] = [
                ("left", (-4..4).map(|d| (l + d, my)).collect()),
                ("right", (-4..4).map(|d| (r + d, my)).collect()),
                ("top", (-4..4).map(|d| (mx, t + d)).collect()),
                ("bottom", (-4..4).map(|d| (mx, bt + d)).collect()),
            ];
            for (side, line) in sides {
                let offs: Vec<i32> = line.iter().map(|&(x, y)| off(px_at(x, y))).collect();
                let peak = offs.iter().copied().min().unwrap_or(255);
                assert!(peak <= 2, "{} {scale}x {name} {side}: the outline peaks {peak}/255 from the ink ({offs:?})", th.name);
                let part = offs.iter().filter(|&&d| d > 2 && d * 4 < span * 3).count();
                assert_eq!(part, 0, "{} {scale}x {name} {side}: pixels part way to the ink beside the outline ({offs:?})", th.name);
            }
        }
    }
}
