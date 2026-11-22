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
                // A scroll lays nothing out.
                assert_eq!(path.layouts, 0, "{mat:?} frame {f}: {}", path.line());
                // Stage 1's boxes share one pass for each image.
                assert_eq!(c.passes, 2, "{mat:?} frame {f}: stage 1 opened {} passes", c.passes);
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
/// moves the last frame and shades the stages again where they are (while the trace shows), and the frame is the frame drawn
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
            // A move with the trace showing shades the stages in the viewport again (the content held to a
            // lattice moves otherwise than the reduced grid, which follows the content).
            let path = u.perf().path;
            if path.moved && part.r.cost.trace > 0 { assert!(path.shade_why.iter().any(|w| w.0 == "stage moved"), "{mat:?} frame {f}: {}", path.line()); }
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

#[test]
fn a_frame_laid_out_names_where_the_layout_was_asked_for() {
    let Ok(mut r) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    // The state's tick changes the view every frame (coalesced input, as the web and desktop hosts run).
    let mut u = open(Material::Plain, 1.0).on_tick(|_, _| true);
    u.coalesce(true);
    let look = u.look();
    let mut t = 5.0;
    for _ in 0..3 { t += 1.0 / 60.0; u.tick(t); let _ = r.render(&mut u, &look); }
    let path = u.perf().path;
    assert_eq!(path.layouts, 1, "{}", path.line());
    assert!(path.layout_from.is_some_and(|l| l.file().ends_with("ui.rs")) && path.line().contains("  layout 1 (src"), "{}", path.line());
    // Scrolled, nothing is laid out again.
    let mut u = open(Material::Plain, 1.0);
    u.coalesce(true);
    for _ in 0..3 { t += 1.0 / 60.0; u.tick(t); u.wheel(240.0, 200.0, 6.0); u.tick(t); let _ = r.render(&mut u, &look); }
    let path = u.perf().path;
    assert_eq!(path.layouts, 0, "{}", path.line());
    assert!(!path.line().contains("layout"), "{}", path.line());
}


// Rows a scroll brings into view were there all along (DESIGN.md §1.8.3), with coalesced input as without
// (the web and desktop hosts coalesce: the scroll's paint waits for the frame, `Ui::settle`).
const MATS: [Material; 6] = [Material::Plain, Material::Dottr, Material::Halftonr, Material::Charr, Material::Smudgr, Material::Carvr];
/// The instances born (condensing) since `t0`.
fn young<S, M: Clone>(u: &Ui<S, M>, t0: f32) -> usize { u.scene().instances.iter().filter(|i| i.extra[3] > t0 - 1e-3).count() }
/// Ten thousand rows, built as they come into view.
fn rows_10k(_: &S) -> El<()> {
    col().key("page").fill_width().fill_height()
        .child(col().key("list").fill_width().grow(1.0).scroll().virtual_rows(10_000, 24.0, 4, None, |i| text(format!("Row {i} of ten thousand"), 14.0, Paint::TEXT).key(format!("v{i}"))))
}

#[test]
fn rows_a_scroll_brings_into_view_never_condense() {
    for big in [false, true] {
        for mat in MATS {
            for coalesce in [false, true] {
                // One device px a frame, 800 px/s and 3000 px/s, 60 frames each.
                for step in [1.0f32, 800.0 / 60.0, 50.0] {
                    let mut u = if big { Ui::new(S { mat }, rows_10k, |_, _| {}, fonts(), W, H, 1.0).on_look(|s| Look { material: s.mat, ..Look::default() }) } else { open(mat, 1.0) };
                    u.pointer_leave();
                    u.tick(5.0);
                    u.coalesce(coalesce);
                    let mut t = 10.0;
                    u.tick(t);
                    u.settle();
                    let t0 = t;
                    for f in 0..60 {
                        t += 1.0 / 60.0;
                        u.tick(t);
                        u.wheel(240.0, 200.0, step);
                        u.settle();
                        assert_eq!(young(&u, t0), 0, "{mat:?}, coalesced {coalesce}, {step} px a frame, {}: frame {f} has instances condensing", if big { "10,000 rows" } else { "60 rows" });
                    }
                    assert!(u.scroll_offset("list") > 50.0);
                }
            }
        }
    }
}

struct Grow { added: bool }
/// Whether the state's tick adds the rows.
static ADD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 60 rows; three more, new keys, in the middle once the state's tick adds them (`ADD`).
fn growing(s: &Grow) -> El<()> {
    let mut c = col().key("rows").gap(6.0).pad(12.0, 12.0);
    for i in 0..60 {
        if i == 14 && s.added { for k in 0..3 { c = c.child(text(format!("Added {k}"), 14.0, Paint::TEXT).key(format!("added{k}"))); } }
        c = c.child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT).key(format!("row{i}")));
    }
    col().key("page").fill_width().fill_height().child(col().key("list").fill_width().grow(1.0).scroll().child(c))
}

#[test]
fn rows_added_while_a_scroll_goes_on_condense_and_nothing_else_does() {
    // The rows come in the same frame as a scroll step, coalesced: one paint answers both, and what was in
    // the layout before (scrolled into view or not) does not condense; the three new keys do.
    for coalesce in [false, true] {
        let mut u = Ui::new(Grow { added: false }, growing, |_, _| {}, fonts(), W, H, 1.0)
            .on_tick(|s, _| { let add = ADD.load(std::sync::atomic::Ordering::Relaxed) && !s.added; if add { s.added = true; } add });
        ADD.store(false, std::sync::atomic::Ordering::Relaxed);
        u.pointer_leave();
        u.tick(5.0);
        u.coalesce(coalesce);
        let mut t = 10.0;
        u.tick(t);
        u.settle();
        let t0 = t;
        // 100 px a frame: every step brings rows into view that were never drawn, the step the rows are
        // added in too.
        for f in 0..5 {
            t += 1.0 / 60.0;
            if f == 2 { ADD.store(true, std::sync::atomic::Ordering::Relaxed); }
            u.tick(t);
            u.wheel(240.0, 200.0, 100.0);
            u.settle();
            let key = |k: usize| u.nodes()[k].key.as_str();
            let young: Vec<&str> = u.scene().instances.iter().zip(u.owners()).filter(|(i, _)| i.extra[3] > t0).map(|(_, o)| key(o.0)).collect();
            assert!(young.iter().all(|k| k.starts_with("added")), "coalesced {coalesce}, frame {f}: condensing {young:?}");
            if f == 2 { for k in 0..3 { assert!(young.contains(&format!("added{k}").as_str()), "coalesced {coalesce}: added{k} does not condense: {young:?}"); } }
        }
    }
}

#[test]
fn a_scroll_that_stops_wants_no_frames_from_settle_ms_on() {
    for coalesce in [false, true] {
        let mut u = open(Material::Dottr, 1.0);
        u.coalesce(coalesce);
        let look = u.look();
        let mut t = 10.0;
        for _ in 0..30 { t += 1.0 / 60.0; u.tick(t); u.wheel(240.0, 200.0, 50.0); u.settle(); u.frame_drawn(); }
        // Frames until it has settled, as a host draws them.
        let stop = t;
        while t < stop + look.scroll.settle_ms / 1000.0 { t += 1.0 / 60.0; u.tick(t); u.settle(); u.frame_drawn(); }
        for k in 0..30 {
            let now = t + k as f32 / 60.0;
            assert!(!u.needs_frame(now) && u.next_wake(now).is_none(), "coalesced {coalesce}: a frame is wanted {:.0} ms after the scroll stopped", (now - stop) * 1000.0);
        }
    }
}

#[test]
fn rows_a_coalesced_scroll_brings_into_view_were_there_all_along() {
    // The web and desktop hosts coalesce input: the scroll's paint waits for the frame (`Ui::settle`), and
    // is still a scroll's, so what comes into view does not condense as if it were new.
    for coalesce in [false, true] {
        let mut u = open(Material::Plain, 1.0);
        u.coalesce(coalesce);
        let t = 10.0;
        u.tick(t);
        let before = u.scene().instances.len();
        u.wheel(240.0, 200.0, 400.0);
        u.settle();
        let young = u.scene().instances.iter().filter(|i| i.extra[3] > t - 1.0).count();
        assert!(u.scroll_offset("list") > 0.0 && !u.scene().instances.is_empty() && before > 0);
        assert_eq!(young, 0, "coalesced {coalesce}: {young} instances scrolled into view condense");
    }
}

#[test]
fn coalesced_or_not_a_scroll_draws_the_same_frames_and_a_slow_one_moves_by_whole_pixels() {
    let Ok(_) = Offscreen::try_new(64, 64) else { eprintln!("no GPU adapter here: skipped"); return };
    for mat in MATS {
        // One device px a frame and 800 px/s.
        for (step, frames) in [(1.0f32, 10), (800.0 / 60.0, 6)] {
            let (mut ra, mut rb) = (Offscreen::new(W, H), Offscreen::new(W, H));
            let (mut a, mut b) = (open(mat, 1.0), open(mat, 1.0));
            b.coalesce(true);
            let look = a.look();
            let mut t = 10.0;
            let mut last: Option<Vec<u8>> = None;
            for f in 0..frames {
                t += 1.0 / 60.0;
                for u in [&mut a, &mut b] { u.tick(t); u.wheel(240.0, 200.0, step); u.tick(t); }
                let (fa, fb) = (ra.render(&mut a, &look), rb.render(&mut b, &look));
                let differ = fa.chunks(4).zip(fb.chunks(4)).filter(|(x, y)| x != y).count();
                assert_eq!(differ, 0, "{mat:?}, {step} px a frame, frame {f}: {differ} pixels differ with coalesced input");
                // `check` item 18, coalesced: the scroll area but 24 px at its edges is the last frame moved
                // up a pixel; control: unmoved, it is not.
                if step == 1.0 && let Some(p) = &last {
                    let r = b.node("list").expect("list").rect;
                    let (x0, x1, y0, y1) = (r.x as u32 + 24, (r.x + r.w) as u32 - 24, r.y as u32 + 24, (r.y + r.h) as u32 - 24);
                    let px = |v: &[u8], x: u32, y: u32| { let i = ((y * W + x) * 4) as usize; [v[i], v[i + 1], v[i + 2]] };
                    let (mut moved, mut still) = (0, 0);
                    for y in y0..y1 { for x in x0..x1 { moved += (px(&fb, x, y) != px(p, x, y + 1)) as u32; still += (px(&fb, x, y) != px(p, x, y)) as u32; } }
                    assert_eq!(moved, 0, "{mat:?}, frame {f}: {moved} pixels of the scroll area are not the last frame moved up a pixel");
                    assert!(still > 0, "{mat:?}, frame {f}: the control (unmoved) matches");
                }
                last = Some(fb);
            }
        }
    }
}

/// A small scroll area inside the list that does not scroll itself (a strip of chips in a card, as the
/// components index has): it is content the frame carries, so the trace's hold still moves the last
/// frame (it drew every such frame whole, "latch: not alone"), and each frame is the frame drawn whole.
#[test]
fn a_still_scroll_area_inside_the_list_does_not_stop_the_hold_moving_the_frame() {
    struct N { ground: Material }
    fn nested(_: &N) -> El<()> {
        let rows = (0..60).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
            let r = row().key(format!("row{i}")).gap(10.0).child(space(14.0, 14.0).radius(3.0).fill(if i % 3 == 0 { Paint::ACCENT } else { Paint::DIM }))
                .child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT));
            if i % 12 == 6 {
                let strip = (0..12).fold(row().key(format!("strip{i}/rows")).gap(8.0), |s, k| s.child(text(format!("chip {k}"), 13.0, Paint::TEXT).key(format!("chip{i}/{k}"))));
                c.child(r).child(row().key(format!("strip{i}")).width(220.0).height(24.0).scroll_x().child(strip))
            } else { c.child(r) }
        });
        col().key("page").fill_width().fill_height().child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    let Ok(mut part) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let Ok(mut whole) = Offscreen::try_new(W, H) else { return };
    whole.r.partial = false;
    for ground in [Material::Dottr, Material::Charr] {
        let mk = || { let mut u = Ui::new(N { ground }, nested, |_, _| {}, fonts(), W, H, 1.0).on_look(|s| Look { material: s.ground, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
        let (mut u, mut v) = (mk(), mk());
        let look = u.look();
        let _ = part.render(&mut u, &look);
        let _ = whole.render(&mut v, &look);
        let mut t = 5.0;
        let (mut held, mut alone) = (0, 0);
        for f in 0..40 {
            t += 1.0 / 60.0;
            let dy = if f < 14 { 6.0 + 4.0 * f as f32 } else { 0.0 };
            for ui in [&mut u, &mut v] { if dy != 0.0 { ui.wheel(240.0, 200.0, dy); } ui.tick(t); }
            let a = part.render(&mut u, &look);
            let b = whole.render(&mut v, &look);
            let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
            let path = u.perf().path;
            assert_eq!(diff, 0, "{ground:?}, frame {f}: {diff} bytes differ from the frame drawn whole ({})", path.line());
            if path.why == "latch: not alone" { alone += 1; }
            let k = u.field().scrollo.iter().map(|o| o[2]).fold(0.0, f32::max);
            if k > 0.02 && path.moved && part.r.cost.moved > 0 { held += 1; }
        }
        assert!(u.field().scrolls.iter().filter(|r| r[2] > r[0]).count() >= 2, "{ground:?}: no strip came into view");
        assert_eq!(alone, 0, "{ground:?}: {alone} frames were drawn whole for the strip");
        assert!(held >= 6, "{ground:?}: only {held} frames with the rows held moved the last frame");
    }
}

/// A page drawn in plain over a dotted look (the root's opaque ground shape overrides the material, as the
/// components pages do): the viewport's ground is plain, so while the trace shows the last frame moves with
/// the content and the plain rows are not shaded again as another lattice (they were, the whole viewport,
/// every frame); a dotted specimen row still is. Each frame is the frame drawn whole.
#[test]
fn a_plain_page_over_a_dotted_look_moves_its_frame_with_the_content() {
    struct P { specimen: bool }
    fn page(s: &P) -> El<()> {
        let rows = (0..60).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
            let r = row().key(format!("row{i}")).gap(10.0).child(space(14.0, 14.0).radius(3.0).fill(if i % 3 == 0 { Paint::ACCENT } else { Paint::DIM }))
                .child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT));
            c.child(if s.specimen && i % 7 == 3 { r.material(Material::Dottr.id()) } else { r })
        });
        col().key("page").fill_width().fill_height().fill(Paint::GROUND).material(Material::Plain.id())
            .child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    let Ok(mut part) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let Ok(mut whole) = Offscreen::try_new(W, H) else { return };
    whole.r.partial = false;
    for specimen in [false, true] {
        let mk = || { let mut u = Ui::new(P { specimen }, page, |_, _| {}, fonts(), W, H, 1.0).on_look(|_| Look { material: Material::Dottr, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
        let (mut u, mut v) = (mk(), mk());
        let look = u.look();
        let _ = part.render(&mut u, &look);
        let _ = whole.render(&mut v, &look);
        let mut t = 5.0;
        let (mut traced, mut lattice) = (0, 0.0f32);
        for f in 0..30 {
            t += 1.0 / 60.0;
            let dy = if f < 14 { 6.0 + 4.0 * f as f32 } else { 0.0 };
            for ui in [&mut u, &mut v] { if dy != 0.0 { ui.wheel(240.0, 200.0, dy); } ui.tick(t); }
            let a = part.render(&mut u, &look);
            let b = whole.render(&mut v, &look);
            let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
            let path = u.perf().path;
            assert_eq!(diff, 0, "specimen {specimen}, frame {f}: {diff} bytes differ from the frame drawn whole ({})", path.line());
            let k = u.field().scrollo.iter().map(|o| o[2]).fold(0.0, f32::max);
            if k > 0.02 && path.moved {
                traced += 1;
                lattice = lattice.max(path.shade_why.iter().filter(|w| w.0 == "lattice").map(|w| w.1).sum::<f32>());
            }
        }
        assert!(traced >= 6, "specimen {specimen}: only {traced} frames with the trace moved the last frame");
        if specimen { assert!(lattice > 0.0, "the dotted rows are not shaded again") } else { assert_eq!(lattice, 0.0, "plain rows on a plain page are shaded again as another lattice") }
    }
}

/// The reduced grid is anchored to the content of the largest scroll area (DESIGN.md §1.35): a slow
/// scroll over stages at their own scale moves the last frame and shades none of the stages it carried
/// again (they were shaded again every moved frame, the grid fixed to the window: 39–47% of the Lab's
/// frames); a stage outside the scroll area (over it, in the bar) is, as the grid moved under it. Each
/// frame is the frame drawn whole, byte for byte, at every reduced scale.
#[test]
fn a_slow_scroll_carries_the_stages_with_the_reduced_grid() {
    struct Q { mat: Material, q: f32 }
    fn staged(_: &Q) -> El<()> {
        let rows = (0..40).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
            let c = c.child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT));
            if i % 8 == 3 { c.child(col().key(format!("cell{i}")).stage().width(220.0).height(90.0).fill(Paint::FAINT).child(text(format!("a stage {i}"), 20.0, Paint::INK))) } else { c }
        });
        col().key("page").fill_width().fill_height()
            .child(col().key("bar").stage().height(40.0).fill_width().fill(Paint::FAINT).child(text("a stage over the list", 16.0, Paint::INK)))
            .child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    let (Ok(mut part), Ok(mut whole)) = (Offscreen::try_new(W * 2, H * 2), Offscreen::try_new(W * 2, H * 2)) else { eprintln!("no GPU adapter here: skipped"); return };
    whole.r.partial = false;
    // At scale 2 the stages go to 1.5, 1 and 0.75 (3/4, 1/2 and 3/8 of the device's).
    for q in [0.75f32, 0.5, 0.375] {
        for mat in [Material::Plain, Material::Dottr] {
            let mk = || { let mut u = Ui::new(Q { mat, q }, staged, |_, _| {}, fonts(), W * 2, H * 2, 2.0).on_look(|s| Look { material: s.mat, quality: s.q, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
            let (mut u, mut v) = (mk(), mk());
            let look = u.look();
            let _ = part.render(&mut u, &look);
            let _ = whole.render(&mut v, &look);
            let mut t = 5.0;
            let (mut carried, mut bar) = (0, 0);
            for f in 0..24 {
                t += 1.0 / 60.0;
                let dy = if f < 10 { 7.0 } else if f < 20 { -5.0 } else { 0.0 };
                for ui in [&mut u, &mut v] { if dy != 0.0 { ui.wheel(240.0, 200.0, dy); } ui.tick(t); }
                let a = part.render(&mut u, &look);
                let b = whole.render(&mut v, &look);
                let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
                let path = u.perf().path;
                assert_eq!(diff, 0, "q {q}, {mat:?} frame {f}: {diff} bytes differ from the frame drawn whole ({})", path.line());
                assert!(path.stage < 1.0, "q {q}, {mat:?} frame {f}: the stages are at their own scale ({})", path.line());
                let again = path.shade_why.iter().filter(|w| w.0 == "stage moved").map(|w| w.1).sum::<f32>();
                if path.moved && dy != 0.0 {
                    // Only the bar's stage: 40 px of the 360.
                    assert!(again < 0.15, "q {q}, {mat:?} frame {f}: the carried stages are shaded again ({})", path.line());
                    if again > 0.0 { bar += 1; } else { carried += 1; }
                }
            }
            assert!(bar >= 10, "q {q}, {mat:?}: the stage outside the list was shaded again in only {bar} moved frames (and {carried} without)");
        }
    }
}

/// The filter up from the reduced grid weighs its four texels itself (DESIGN.md §1.35); against the
/// sampler's filter for the same reduced pass it differs by one step of 8 bits at most in any channel, at
/// any offset of the grid; at full quality there is no reduced pass and the frames are the same.
#[test]
fn the_filter_up_differs_from_the_samplers_by_one_step_at_most() {
    struct Q { mat: Material, q: f32 }
    fn staged(_: &Q) -> El<()> {
        let rows = (0..40).fold(col().key("rows").gap(6.0).pad(12.0, 12.0), |c, i| {
            let c = c.child(text(format!("Row {i}: a line of text to read as it scrolls past"), 14.0, Paint::TEXT));
            if i % 8 == 3 { c.child(col().key(format!("cell{i}")).stage().width(220.0).height(90.0).fill(Paint::FAINT).child(text(format!("a stage {i}"), 20.0, Paint::INK))) } else { c }
        });
        col().key("page").fill_width().fill_height()
            .child(col().key("bar").stage().height(40.0).fill_width().fill(Paint::FAINT).child(text("a stage over the list", 16.0, Paint::INK)))
            .child(col().key("list").fill_width().grow(1.0).scroll().child(rows))
    }
    for scale in [1.0f32, 2.0] {
        let (w, h) = ((W as f32 * scale) as u32, (H as f32 * scale) as u32);
        let (Ok(mut own), Ok(mut samp)) = (Offscreen::try_new(w, h), Offscreen::try_new(w, h)) else { eprintln!("no GPU adapter here: skipped"); return };
        for o in [&mut own, &mut samp] { o.r.partial = false; o.r.always_lo = true; }
        samp.r.up_by_sampler(true);
        let mut seen = 0;
        for q in [1.0f32, 0.75, 0.5, 0.375] {
            for mat in [Material::Plain, Material::Dottr] {
                let mk = || { let mut u = Ui::new(Q { mat, q }, staged, |_, _| {}, fonts(), w, h, scale).on_look(|s| Look { material: s.mat, quality: s.q, ..Look::default() }); u.pointer_leave(); u.tick(5.0); u };
                let (mut u, mut v) = (mk(), mk());
                let look = u.look();
                let mut t = 5.0;
                // At rest, then at offsets that put the grid's texels between device px (7 px a step).
                for f in 0..3 {
                    if f > 0 { t += 1.0; for ui in [&mut u, &mut v] { ui.wheel(240.0, 200.0, 7.0 * f as f32); ui.tick(t); } }
                    let a = own.render(&mut u, &look);
                    let b = samp.render(&mut v, &look);
                    let most = a.iter().zip(&b).map(|(p, q)| p.abs_diff(*q)).max().unwrap_or(0);
                    seen = seen.max(most);
                    let reduced = u.perf().path.reduced;
                    if q < 1.0 {
                        assert!(reduced > 0.0, "scale {scale}, q {q}, {mat:?} step {f}: no reduced pass ({})", u.perf().path.line());
                        assert!(most <= 1, "scale {scale}, q {q}, {mat:?} step {f}: a channel differs by {most}/255 from the sampler's filter");
                    } else {
                        assert_eq!(most, 0, "scale {scale}, {mat:?} step {f}: at full quality a channel differs by {most}/255");
                    }
                }
            }
        }
        // 1 on lavapipe (8-bit weights in the sampler); a GPU that weighs in floats may differ by none.
        eprintln!("scale {scale}: a channel differs from the sampler's filter by {seen}/255 at most");
    }
}
