//! Scrolling moves the last frame (DESIGN.md §1.19): a slow scroll step shades only the band scrolled
//! into view, the edges the material reads across and what changed, counted by the renderer
//! (`Renderer::cost`), and the frame it draws is the frame drawn whole, byte for byte, at every offset,
//! down and back up, in several materials and at two scales. Past the calm speed, with the trace showing,
//! the last frame is moved too and the pixels the trace reaches are shaded again; glitchr is drawn whole.
use amenui::el::{El, Paint, col, row, space, text};
use amenui::offscreen::Offscreen;
use amenui::style::{Look, Material};
use amenui::ui::Ui;

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }
const W: u32 = 480;
const H: u32 = 360;

struct S { mat: Material }
fn view(_: &S) -> El<()> {
    let rows = (0..60).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
        c.child(row().key(format!("row{i}")).gap(10.0).child(space(14.0, 14.0).radius(3.0).fill(if i % 3 == 0 { Paint::ACCENT } else { Paint::DIM }))
            .child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT)))
    });
    col().key("page").fill_width().fill_height()
        .child(text("A bar over the list that stays", 16.0, Paint::INK).key("bar"))
        .child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
}
fn open(mat: Material, scale: f32) -> Ui<S, ()> {
    let mut u = Ui::new(S { mat }, view, |_, _| {}, fonts(), (W as f32 * scale) as u32, (H as f32 * scale) as u32, scale).on_look(|s| Look { material: s.mat, ..Look::default() });
    u.pointer_leave();
    u.tick(5.0);
    u
}

#[test]
fn a_slow_scroll_shades_only_the_band_it_brings_in_and_draws_the_frame_drawn_whole() {
    for scale in [1.0f32, 2.0] {
        let (w, h) = ((W as f32 * scale) as u32, (H as f32 * scale) as u32);
        let (Ok(mut part), Ok(mut whole)) = (Offscreen::try_new(w, h), Offscreen::try_new(w, h)) else { eprintln!("no GPU adapter here: skipped"); return };
        whole.r.partial = false;
        for mat in [Material::Plain, Material::Halftonr, Material::Charr, Material::Dottr] {
            let (mut u, mut v) = (open(mat, scale), open(mat, scale));
            let look = u.look();
            let _ = part.render(&mut u, &look);
            let _ = whole.render(&mut v, &look);
            let (x, y) = (W as f32 * 0.5, H as f32 * 0.6);
            let mut t = 5.0;
            let mut moved = 0;
            // Down by 6 px a frame (360 px/s, under the trace's calm speed), then back up.
            for f in 0..24 {
                t += 1.0 / 60.0;
                let dy = if f < 12 { 6.0 } else { -6.0 };
                for ui in [&mut u, &mut v] { ui.wheel(x, y, dy); ui.tick(t); }
                let a = part.render(&mut u, &look);
                let b = whole.render(&mut v, &look);
                let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
                assert_eq!(diff, 0, "{mat:?} at scale {scale}, frame {f}: {diff} bytes differ from the frame drawn whole");
                let c = part.r.cost;
                let path = u.perf().path;
                assert_eq!(path.moved, c.moved > 0, "{mat:?} at scale {scale}, frame {f}: the readout says {}", path.line());
                if c.moved == 0 { continue; }
                moved += 1;
                assert!((path.shade - c.material as f32 / (w * h) as f32).abs() < 1e-3, "{mat:?} frame {f}: the readout's shade share: {}", path.line());
                // What put the boxes there: the band scrolled in among the three largest, none above the share.
                assert!(path.shade_why.iter().any(|w| w.0 == "band" && w.1 > 0.0), "{mat:?} frame {f}: the band is a reason: {}", path.line());
                assert!(path.shade_why.windows(2).all(|p| p[0].1 >= p[1].1) && path.line().contains("% (") , "{mat:?} frame {f}: {}", path.line());
                // The band is 6 px of the list's height. The strips along the viewport's edges that the
                // material reads across (up to 12 px a side for dottr), the bar's spill and the scroll bar
                // are a fixed cost, large in a window this small: under half of it here, about a sixth of
                // a 1440 px wide one.
                let share = c.material as f64 / (w * h) as f64;
                assert!(share < 0.5, "{mat:?} at scale {scale}, frame {f}: the material shaded {:.0}% of the window", share * 100.0);
                assert!(c.stage1 < c.material, "{mat:?} at scale {scale}, frame {f}: stage 1 drew {} px, the material {}", c.stage1, c.material);
            }
            assert!(moved >= 20, "{mat:?} at scale {scale}: only {moved} of 24 frames moved the last frame");
        }
    }
}

#[test]
fn a_fast_scroll_with_its_trace_moves_the_last_frame_and_draws_the_frame_drawn_whole() {
    for scale in [1.0f32, 2.0] {
        let (w, h) = ((W as f32 * scale) as u32, (H as f32 * scale) as u32);
        let (Ok(mut part), Ok(mut whole)) = (Offscreen::try_new(w, h), Offscreen::try_new(w, h)) else { eprintln!("no GPU adapter here: skipped"); return };
        whole.r.partial = false;
        for mat in [Material::Plain, Material::Dottr, Material::Charr, Material::Halftonr, Material::Smudgr, Material::Carvr] {
            let (mut u, mut v) = (open(mat, scale), open(mat, scale));
            let look = u.look();
            let _ = part.render(&mut u, &look);
            let _ = whole.render(&mut v, &look);
            let (x, y) = (W as f32 * 0.5, H as f32 * 0.6);
            let mut t = 5.0;
            let (mut moved, mut traced) = (0, 0);
            // Down 40 px a frame (2400 px/s, past the trace's calm speed), back up, then still while the
            // trace settles.
            for f in 0..40 {
                t += 1.0 / 60.0;
                let dy = if f < 12 { 40.0 } else if f < 24 { -40.0 } else { 0.0 };
                for ui in [&mut u, &mut v] { if dy != 0.0 { ui.wheel(x, y, dy); } ui.tick(t); }
                let a = part.render(&mut u, &look);
                let b = whole.render(&mut v, &look);
                let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
                assert_eq!(diff, 0, "{mat:?} at scale {scale}, frame {f}: {diff} bytes differ from the frame drawn whole");
                let c = part.r.cost;
                if c.moved > 0 || c.trace > 0 { moved += 1; }
                if c.trace > 0 { traced += 1; }
            }
            assert!(traced >= 12, "{mat:?} at scale {scale}: only {traced} of 40 frames moved the last frame with the trace showing ({moved} moved)");
        }
    }
}

#[test]
fn a_fast_scroll_under_glitchr_is_drawn_whole_in_the_viewport() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let mut u = open(Material::Glitchr, 1.0);
    let look = u.look();
    let _ = o.render(&mut u, &look);
    let mut t = 5.0;
    // 120 px a frame: 7200 px/s, the trace at full strength; glitchr's rows slip on their own clock.
    for _ in 0..3 { t += 1.0 / 60.0; u.wheel(240.0, 200.0, 120.0); u.tick(t); let _ = o.render(&mut u, &look); }
    assert!(u.field().scrollo.iter().any(|o| o[2] > 0.0), "the trace shows");
    assert_eq!(o.r.cost.moved, 0, "under glitchr with the trace showing the frame is not the last one moved");
    // The readout says so, and why.
    let path = u.perf().path;
    assert!(!path.moved && path.why == "trace: glitchr or custom", "{}", path.line());
    assert!(path.line().starts_with("move no (trace: glitchr or custom)"), "{}", path.line());
}

/// A list with stages in it, shaded at their own scale (the Lab): a fast scroll with its trace showing
/// moves the last frame and shades the stages again where they are, and the frame is the frame drawn
/// whole, byte for byte, the moved one with the passes made for its material (`Renderer::spec_force`).
#[test]
fn a_fast_scroll_over_stages_at_their_own_scale_moves_the_last_frame() {
    fn staged(_: &S) -> El<()> {
        let rows = (0..40).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
            let c = c.child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT));
            if i % 8 == 3 { c.child(col().key(format!("cell{i}")).stage().width(220.0).height(90.0).fill(Paint::FAINT).child(text(format!("a stage {i}"), 20.0, Paint::INK))) } else { c }
        });
        col().key("page").fill_width().fill_height().child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    let (Ok(mut part), Ok(mut whole)) = (Offscreen::try_new(W, H), Offscreen::try_new(W, H)) else { eprintln!("no GPU adapter here: skipped"); return };
    whole.r.partial = false;
    // The moved frames with the passes made for the material (its trace pass too), the whole ones with
    // the pass for any.
    part.r.spec_force = Some(true);
    for mat in [Material::Plain, Material::Dottr, Material::Charr] {
        let mk = || { let mut u = Ui::new(S { mat }, staged, |_, _| {}, fonts(), W, H, 1.0).on_look(|s| Look { material: s.mat, quality: 0.75, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
        let (mut u, mut v) = (mk(), mk());
        let look = u.look();
        let _ = part.render(&mut u, &look);
        let _ = whole.render(&mut v, &look);
        let mut t = 5.0;
        let mut traced = 0;
        for f in 0..30 {
            t += 1.0 / 60.0;
            let dy = if f < 10 { 40.0 } else if f < 20 { -40.0 } else { 0.0 };
            for ui in [&mut u, &mut v] { if dy != 0.0 { ui.wheel(240.0, 200.0, dy); } ui.tick(t); }
            let a = part.render(&mut u, &look);
            let b = whole.render(&mut v, &look);
            let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
            assert_eq!(diff, 0, "{mat:?} frame {f}: {diff} bytes differ from the frame drawn whole ({})", u.perf().path.line());
            assert!(u.perf().path.stage < 1.0, "{mat:?} frame {f}: the stages are at their own scale ({})", u.perf().path.line());
            if part.r.cost.trace > 0 { traced += 1; }
            // A move shades the stages in the viewport again (the reduced grid stays with the window).
            let path = u.perf().path;
            if path.moved { assert!(path.shade_why.iter().any(|w| w.0 == "stage moved"), "{mat:?} frame {f}: {}", path.line()); }
        }
        assert_eq!(part.r.specialised(), Some(mat.id()), "{mat:?}: drawn with its own passes");
        assert!(traced >= 8, "{mat:?}: only {traced} of 30 frames moved the last frame with the trace showing ({})", u.perf().path.line());
    }
}

/// Under dottr and charr a fast scroll's trace comes on and fades: as it passes the strength that holds
/// the rows to the lattice, the frame moves too (by what it shows: the content held in one frame, at its
/// offset in the other), and with another lattice's material only outside the viewport (a bar in dottr
/// over a list in charr) it moves as well; each frame is the frame drawn whole, byte for byte.
#[test]
fn the_hold_coming_on_or_off_and_a_lattice_outside_the_viewport_still_move_the_last_frame() {
    struct B { mat: Material, bar: Option<Material> }
    fn barred(s: &B) -> El<()> {
        let rows = (0..60).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
            c.child(row().key(format!("row{i}")).gap(10.0).child(space(14.0, 14.0).radius(3.0).fill(if i % 3 == 0 { Paint::ACCENT } else { Paint::DIM }))
                .child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT)))
        });
        let bar = text("A bar over the list that stays", 16.0, Paint::INK).key("bar");
        col().key("page").fill_width().fill_height().gap(24.0)
            .child(match s.bar { Some(m) => bar.material(m.id()), None => bar })
            .child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    let Ok(mut part) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let Ok(mut whole) = Offscreen::try_new(W, H) else { return };
    whole.r.partial = false;
    for (mat, bar) in [(Material::Dottr, None), (Material::Charr, None), (Material::Charr, Some(Material::Dottr))] {
        let mk = || { let mut u = Ui::new(B { mat, bar }, barred, |_, _| {}, fonts(), W, H, 1.0).on_look(|s| Look { material: s.mat, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
        let (mut u, mut v) = (mk(), mk());
        let look = u.look();
        let _ = part.render(&mut u, &look);
        let _ = whole.render(&mut v, &look);
        let mut t = 5.0;
        let (mut on, mut off, mut held) = (0, 0, 0);
        let mut last = 0.0f32;
        for f in 0..48 {
            t += 1.0 / 60.0;
            // Gathering speed, then still while the trace fades.
            let dy = if f < 14 { 6.0 + 4.0 * f as f32 } else { 0.0 };
            for ui in [&mut u, &mut v] { if dy != 0.0 { ui.wheel(240.0, 220.0, dy); } ui.tick(t); }
            let a = part.render(&mut u, &look);
            let b = whole.render(&mut v, &look);
            let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
            assert_eq!(diff, 0, "{mat:?} bar {bar:?}, frame {f}: {diff} bytes differ from the frame drawn whole");
            let k = u.field().scrollo.iter().map(|o| o[2]).fold(0.0, f32::max);
            let path = u.perf().path;
            assert!(path.why != "latch on/off" && path.why != "latch: mixed lattices", "{mat:?} bar {bar:?}, frame {f}: {}", path.line());
            let crossed = (last > 0.02) != (k > 0.02);
            if crossed && path.moved { if k > 0.02 { on += 1 } else { off += 1 } }
            if k > 0.02 && path.moved && part.r.cost.moved > 0 { held += 1; }
            last = k;
        }
        assert!(on >= 1, "{mat:?} bar {bar:?}: the frame the hold came on in did not move the last frame");
        assert!(off >= 1, "{mat:?} bar {bar:?}: the frame the hold went off in did not move the last frame");
        assert!(held >= 6, "{mat:?} bar {bar:?}: only {held} frames with the rows held moved the last frame");
    }
}

/// Rows of other materials in the viewport (another lattice, or none) while the trace holds the rows:
/// the last frame still moves, as the ground's material does (held to its lattice, or with the content),
/// and what is drawn in the others is shaded again; each frame is the frame drawn whole, byte for byte.
#[test]
fn rows_of_another_lattice_in_the_viewport_still_move_the_last_frame() {
    struct M { ground: Material, rows: [Material; 2] }
    fn mixed(s: &M) -> El<()> {
        let rows = (0..60).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
            let r = row().key(format!("row{i}")).gap(10.0).child(space(14.0, 14.0).radius(3.0).fill(if i % 3 == 0 { Paint::ACCENT } else { Paint::DIM }))
                .child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT));
            c.child(match i % 5 { 1 => r.material(s.rows[0].id()), 3 => r.material(s.rows[1].id()), _ => r })
        });
        col().key("page").fill_width().fill_height().child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    let Ok(mut part) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let Ok(mut whole) = Offscreen::try_new(W, H) else { return };
    whole.r.partial = false;
    for (ground, rows) in [(Material::Dottr, [Material::Charr, Material::Plain]), (Material::Plain, [Material::Dottr, Material::Charr]), (Material::Charr, [Material::Dottr, Material::Halftonr])] {
        let mk = || { let mut u = Ui::new(M { ground, rows }, mixed, |_, _| {}, fonts(), W, H, 1.0).on_look(|s| Look { material: s.ground, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
        let (mut u, mut v) = (mk(), mk());
        let look = u.look();
        let _ = part.render(&mut u, &look);
        let _ = whole.render(&mut v, &look);
        let mut t = 5.0;
        let mut held = 0;
        for f in 0..40 {
            t += 1.0 / 60.0;
            let dy = if f < 14 { 6.0 + 4.0 * f as f32 } else { 0.0 };
            for ui in [&mut u, &mut v] { if dy != 0.0 { ui.wheel(240.0, 200.0, dy); } ui.tick(t); }
            let a = part.render(&mut u, &look);
            let b = whole.render(&mut v, &look);
            let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
            let path = u.perf().path;
            assert_eq!(diff, 0, "{ground:?} with {rows:?}, frame {f}: {diff} bytes differ from the frame drawn whole ({})", path.line());
            assert!(path.why != "latch: mixed lattices", "{ground:?} with {rows:?}, frame {f}: {}", path.line());
            let k = u.field().scrollo.iter().map(|o| o[2]).fold(0.0, f32::max);
            if k > 0.02 && path.moved && part.r.cost.moved > 0 { held += 1; }
        }
        assert!(held >= 6, "{ground:?} with {rows:?}: only {held} frames with the rows held moved the last frame");
    }
}

/// A page's ground drawn as a shape behind its scroll area (`fill(Paint::GROUND)` on the page, as the
/// components pages have it) is not the content's but lies under the whole viewport: plain, with its
/// edges outside it, it moves as the ground does, so a slow scroll still shades only the band and the
/// edges (it was shaded again over the whole viewport, where it is and where it went).
#[test]
fn a_ground_shape_behind_the_scroll_area_is_not_shaded_again() {
    fn filled(_: &S) -> El<()> {
        let rows = (0..60).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| c.child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT).key(format!("row{i}"))));
        col().key("page").fill_width().fill_height().fill(Paint::GROUND).material(Material::Plain.id())
            .child(text("A bar over the list that stays", 16.0, Paint::INK).key("bar"))
            .child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    let (Ok(mut part), Ok(mut whole)) = (Offscreen::try_new(W, H), Offscreen::try_new(W, H)) else { eprintln!("no GPU adapter here: skipped"); return };
    whole.r.partial = false;
    for mat in [Material::Plain, Material::Dottr] {
        let mk = || { let mut u = Ui::new(S { mat }, filled, |_, _| {}, fonts(), W, H, 1.0).on_look(|s| Look { material: s.mat, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
        let (mut u, mut v) = (mk(), mk());
        let look = u.look();
        let _ = part.render(&mut u, &look);
        let _ = whole.render(&mut v, &look);
        let mut t = 5.0;
        let mut moved = 0;
        for f in 0..16 {
            t += 1.0 / 60.0;
            let dy = if f < 8 { 6.0 } else { -6.0 };
            for ui in [&mut u, &mut v] { ui.wheel(240.0, 220.0, dy); ui.tick(t); }
            let a = part.render(&mut u, &look);
            let b = whole.render(&mut v, &look);
            let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
            assert_eq!(diff, 0, "{mat:?} frame {f}: {diff} bytes differ from the frame drawn whole");
            let c = part.r.cost;
            if c.moved == 0 { continue; }
            moved += 1;
            let share = c.stage1 as f64 / (W * H) as f64;
            assert!(share < 0.3, "{mat:?} frame {f}: stage 1 drew {:.0}% of the window ({})", share * 100.0, u.perf().path.line());
        }
        assert!(moved >= 12, "{mat:?}: only {moved} of 16 frames moved the last frame");
    }
}
