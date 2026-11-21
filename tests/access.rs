//! Less motion and what cannot be seen (DESIGN.md §1.14): with less motion asked for, what the
//! framework brings in fades in over 150 ms in place, its ink brightening frame by frame and its text
//! readable when the fade ends; an element at opacity 0 takes no pointer and a screen reader still
//! finds it.
use amenui::el::{El, Paint, col, text};
use amenui::offscreen::Offscreen;
use amenui::style::{Look, Material};
use amenui::ui::Ui;

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }

struct S { shown: bool, op: f32, pressed: u32 }
fn view(s: &S) -> El<u32> {
    let mut page = col().key("page").pad(40.0, 40.0).gap(16.0).child(text("Notes", 22.0, Paint::INK).key("title"));
    if s.shown { page = page.child(text("A line that comes in", 16.0, Paint::TEXT).key("line").opacity(s.op).on_press(1)); }
    page
}
fn look(_: &S) -> Look { Look { material: Material::Dottr, ..Look::default() } }

#[test]
fn with_less_motion_a_new_element_fades_in_where_it_will_stay() {
    let (w, h) = (420u32, 220u32);
    let Ok(mut o) = Offscreen::try_new(w, h) else { eprintln!("no GPU adapter here: skipped"); return };
    let mut ui = Ui::new(S { shown: false, op: 1.0, pressed: 0 }, view, |s, m| s.pressed += m, fonts(), w, h, 1.0).on_look(look);
    ui.set_reduced_motion(true);
    let mut t = 1.0;
    ui.tick(t);
    let look = ui.look();
    let before = o.render(&mut ui, &look);
    ui.state.shown = true;
    ui.relayout();
    let r = ui.node("line").expect("line").rect;
    // Five frames 30 ms apart: the mean distance of its box from the ground only grows, and the box
    // stays where it is.
    let mut seen = vec![];
    for _ in 0..6 {
        t += 0.03;
        ui.tick(t);
        let px = o.render(&mut ui, &look);
        assert_eq!(ui.node("line").expect("line").rect, r, "it does not move");
        let (mut d, mut n) = (0.0f64, 0usize);
        for y in r.y as u32..(r.bottom() as u32).min(h) {
            for x in r.x as u32..(r.right() as u32).min(w) {
                let i = ((y * w + x) * 4) as usize;
                d += (0..3).map(|k| (px[i + k] as f64 - before[i + k] as f64).abs()).sum::<f64>();
                n += 1;
            }
        }
        seen.push(d / n.max(1) as f64);
    }
    assert!(seen.windows(2).all(|p| p[1] >= p[0] - 0.05), "it brightens frame by frame: {seen:?}");
    assert!(seen[0] < seen[4] * 0.8, "it fades in, not all at once: {seen:?}");
    // Where the fade ends the line reads: no P0 or P1 on it.
    let px = o.render(&mut ui, &look);
    let found = amenui::drive::Driven::audit(&mut ui, &mut o, &look, &px);
    let bad: Vec<_> = found.iter().filter(|f| f.level <= amenui::audit::Level::P1 && f.key == "line").collect();
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn at_opacity_zero_it_takes_no_pointer_and_a_screen_reader_finds_it() {
    let mut ui = Ui::new(S { shown: true, op: 0.0, pressed: 0 }, view, |s, m| s.pressed += m, fonts(), 420, 220, 1.0);
    ui.tick(1.0);
    assert!(ui.instances_of("line").is_empty(), "it is not drawn");
    assert!(ui.a11y().iter().any(|n| n.key == "line"), "a screen reader finds it");
    let (x, y) = ui.node("line").expect("line").rect.center();
    ui.press(x, y);
    ui.release(x, y);
    ui.tick(1.1);
    assert_eq!(ui.state.pressed, 0, "a press goes through it");
    ui.state.op = 1.0;
    ui.relayout();
    ui.tick(1.2);
    ui.press(x, y);
    ui.release(x, y);
    ui.tick(1.3);
    assert_eq!(ui.state.pressed, 1, "seen, it takes the press");
}

struct L;
fn long(_: &L) -> El<()> {
    let list = (0..40).fold(col().key("list").fill_width().grow(1.0).scroll(), |c, i| c.child(text(format!("Row {i}"), 14.0, Paint::TEXT).key(format!("r{i}"))));
    col().key("page").fill_width().fill_height().child(text("A title that stays", 18.0, Paint::INK).key("title")).child(list)
}
#[test]
fn while_a_scroll_goes_on_the_mirror_is_sent_once_it_rests() {
    use amenui::ui::A11yOp;
    let mut ui = Ui::new(L, long, |_, _| {}, fonts(), 400, 300, 1.0);
    ui.pointer_leave();
    let mut t = 1.0;
    ui.tick(t);
    assert!(!ui.a11y_diff().is_empty(), "the first call sends the mirror");
    let before: Vec<_> = ui.a11y();
    // Scrolling: nothing is sent while it goes on, and a wake is asked for when it rests.
    for _ in 0..5 {
        t += 1.0 / 120.0;
        ui.tick(t);
        ui.wheel(200.0, 200.0, 8.0);
        // The frame after it (the host ticks at each event and at the frame).
        t += 1.0 / 120.0;
        ui.tick(t);
        assert!(ui.a11y_diff().is_empty(), "nothing sent while the scroll goes on");
        ui.frame_drawn();
    }
    let last = t;
    let wake = ui.next_wake(t).expect("a wake for the mirror");
    assert!(wake <= last + 0.11, "woken by the time the scroll rests: {wake} after {last}");
    // Frames go on without a scroll: the first after it rests sends the rows' new places.
    let mut ops = vec![];
    while ops.is_empty() && t < last + 0.5 { t += 1.0 / 60.0; ui.tick(t); ops = ui.a11y_diff(); ui.frame_drawn(); }
    assert!(t >= last + 0.1 && t < last + 0.14, "sent as it rests: {t} after {last}");
    let after = ui.a11y();
    let moved = after.iter().filter(|n| before.iter().any(|b| b.key == n.key && b.rect != n.rect)).count();
    assert!(moved > 10 && ops.iter().filter(|o| matches!(o, A11yOp::Set(_))).count() >= moved, "the rows' new places are sent: {moved} moved, {} ops", ops.len());
    assert!(ui.a11y_diff().is_empty(), "and only once");
}
