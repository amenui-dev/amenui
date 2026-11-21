//! A wheel gesture stays with what took its first step (DESIGN.md §1.8.2): a page scrolled by a stream of
//! wheel steps (a trackpad's momentum) keeps scrolling when an element that takes the wheel comes under
//! the pointer, and nothing is laid out again; a new gesture there (after `WHEEL_LATCH`, or with the
//! pointer moved) goes to the element. An element whose handler makes nothing of a step leaves it to the
//! scroll.
use amenui::el::{El, Paint, col, space};
use amenui::ui::{Ui, WHEEL_LATCH};

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }

#[derive(Default)]
struct S { dolly: f32, takes: bool }
#[derive(Clone)]
enum M { Wheel(f32) }
/// A long page with a 3D-view-like box 400 px down that takes the wheel (or makes nothing of it).
fn view(s: &S) -> El<M> {
    let takes = s.takes;
    let wheel = space(300.0, 120.0).key("dolly").fill(Paint::DIM).on_wheel(move |_, dy| takes.then_some(M::Wheel(dy)));
    col().key("page").fill_width().fill_height().scroll()
        .child(space(300.0, 400.0)).child(wheel).child(space(300.0, 2000.0))
}
fn open(takes: bool) -> Ui<S, M> {
    let mut u = Ui::new(S { takes, ..S::default() }, view, |s: &mut S, M::Wheel(d)| s.dolly += d, fonts(), 400, 300, 1.0);
    u.coalesce(true);
    u.tick(1.0);
    u.frame_drawn();
    u
}

#[test]
fn a_gesture_scrolls_on_over_an_element_that_takes_the_wheel() {
    let mut u = open(true);
    let mut t = 1.0;
    // 60 steps of 20 px, a frame apart, the pointer still: the box passes under it at about 400 px.
    for _ in 0..60 {
        t += 1.0 / 60.0;
        u.tick(t);
        u.wheel(150.0, 150.0, 20.0);
        u.settle();
        assert_eq!(u.layouts_since_drawn().0, 0, "a scroll step laid out at {}", u.scroll_offset("page"));
        u.frame_drawn();
    }
    assert_eq!(u.scroll_offset("page"), 1200.0);
    assert_eq!(u.state.dolly, 0.0);
    // Back up to put the box under the pointer, then a new gesture there once the last one is over.
    for _ in 0..45 { t += 1.0 / 60.0; u.tick(t); u.wheel(150.0, 150.0, -20.0); }
    let at = u.scroll_offset("page");
    assert_eq!(at, 300.0);
    t += WHEEL_LATCH + 0.01;
    u.tick(t);
    u.wheel(150.0, 150.0, 10.0);
    assert_eq!((u.state.dolly, u.scroll_offset("page")), (10.0, at), "a new gesture over the box goes to it");
    // Its next step goes to it too.
    t += 1.0 / 60.0;
    u.tick(t);
    u.wheel(150.0, 150.0, 10.0);
    assert_eq!(u.state.dolly, 20.0);
    // The pointer moved: a new gesture, where the pointer is now (the page above the box).
    u.wheel(150.0, 10.0, 10.0);
    assert_eq!((u.state.dolly, u.scroll_offset("page")), (20.0, at + 10.0));
}

#[test]
fn a_step_the_element_makes_nothing_of_scrolls_the_page() {
    let mut u = open(false);
    let mut t = 1.0;
    // The box starts under the pointer.
    u.wheel(150.0, 150.0, 300.0);
    t += WHEEL_LATCH + 0.01;
    u.tick(t);
    u.wheel(150.0, 150.0, 20.0);
    assert_eq!(u.scroll_offset("page"), 320.0);
    assert_eq!(u.state.dolly, 0.0);
}
