//! Text to read kept clear of the screen's artefacts (`PostSpec::clear_text`, Glitch): inside the ink
//! boxes of text to read, red and blue are those of the same frame with no chroma offset (within 1/255);
//! a page with no text is drawn the same with it on or off, and text not to read keeps its fringe. Needs
//! a GPU adapter (lavapipe will do); without one the tests say so and pass. The frames are compared with
//! no curvature, which moves what is drawn away from the ink boxes as laid out.
use amenui::drive::Driven;
use amenui::el::{El, Paint, col, space, text};
use amenui::offscreen::Offscreen;
use amenui::preset::presets;
use amenui::style::Look;
use amenui::ui::Ui;

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }
const W: u32 = 520;
const H: u32 = 300;

/// Each line fits at 2x (260 logical px wide).
fn page(_: &()) -> El<()> {
    col().pad(16.0, 12.0).gap(8.0)
        .child(text("A heading", 26.0, Paint::INK).key("head"))
        .child(text("Text to read, kept clear.", 15.0, Paint::TEXT).key("body"))
        .child(text("kept fringed", 15.0, Paint::TEXT).legible(false).key("deco"))
        .child(space(200.0, 16.0).fill(Paint::ACCENT).key("bar"))
}
fn shapes(_: &()) -> El<()> { col().pad(40.0, 40.0).gap(14.0).child(space(220.0, 40.0).fill(Paint::ACCENT).key("a")).child(space(120.0, 90.0).fill(Paint::INK).radius(12.0).key("b")) }

fn open(view: fn(&()) -> El<()>, scale: f32) -> Ui<(), ()> {
    let mut ui = Ui::new((), view, |_, _| {}, fonts(), W, H, scale);
    for k in 0..=60 { ui.tick(k as f32 / 60.0); }
    ui.settle();
    ui
}
fn glitch(ui: &Ui<(), ()>) -> Look { presets().into_iter().find(|p| p.hash == "glitch").expect("glitch").apply(&Driven::look(ui)) }
/// The ink boxes (device px) of `key`'s glyphs.
fn boxes(ui: &Ui<(), ()>, key: &str) -> Vec<[f32; 4]> {
    let n = ui.nodes().iter().position(|n| n.key == key).expect("node");
    ui.glyphs().log.iter().filter(|g| g.node == n).filter_map(|g| ui.scene().instances.get(g.inst)).map(amenui::form::ink_box).filter(|b| b[2] > b[0]).collect()
}
/// The most red or blue differs between `a` and `b` within `bs`.
fn rb(a: &[u8], b: &[u8], bs: &[[f32; 4]]) -> u8 {
    let mut most = 0;
    for bx in bs {
        for y in bx[1].floor().max(0.0) as u32..(bx[3].ceil() as u32).min(H) {
            for x in bx[0].floor().max(0.0) as u32..(bx[2].ceil() as u32).min(W) {
                let o = ((y * W + x) * 4) as usize;
                for c in [0, 2] { most = most.max(a[o + c].abs_diff(b[o + c])); }
            }
        }
    }
    most
}

#[test]
fn text_to_read_has_no_fringe_under_glitch_and_the_rest_keeps_it() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    for scale in [1.0, 2.0] {
        let mut ui = open(page, scale);
        let mut look = glitch(&ui);
        assert!(look.post.clear_text && look.post.chroma_on(), "Glitch keeps text clear and has a chroma offset");
        // Flat, so the ink boxes are where the glyphs are drawn (the bend moves them by up to 9 px).
        look.post.curvature.amount = 0.0;
        let on = o.render(&mut ui, &look);
        let mut still = look;
        still.post.chroma.offset = 0.0;
        let off = o.render(&mut ui, &still);
        for key in ["head", "body"] {
            let d = rb(&on, &off, &boxes(&ui, key));
            assert!(d <= 1, "{key} at {scale}x: red or blue {d}/255 from the frame with no offset");
        }
        // Text not to read keeps the full pass.
        let deco = rb(&on, &off, &boxes(&ui, "deco"));
        assert!(deco > 16, "decoration keeps its fringe at {scale}x: {deco}");
        // With it off, text to read has the fringe as before.
        let mut plain = look;
        plain.post.clear_text = false;
        let fringed = o.render(&mut ui, &plain);
        assert!(rb(&fringed, &off, &boxes(&ui, "body")) > 16, "without it text has the fringe at {scale}x");
    }
}

#[test]
fn a_page_with_no_text_is_drawn_the_same_with_it_on_or_off() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let mut ui = open(shapes, 1.0);
    let look = glitch(&ui);
    let on = o.render(&mut ui, &look);
    let mut off = look;
    off.post.clear_text = false;
    assert!(on == o.render(&mut ui, &off), "the same bytes");
    // No other preset keeps text clear.
    for p in presets() { assert_eq!(p.post.clear_text, p.hash == "glitch", "{}", p.name); }
}
