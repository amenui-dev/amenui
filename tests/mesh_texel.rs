//! A 3D element's pixels stand for its target's texels exactly (DESIGN.md §1.18): each pixel reads the
//! texel from its whole position in the box, not from interpolated coordinates that land on a texel's
//! edge at whole scales and round either way by the driver. A card stack (supersampled twice) at whole
//! pixel boxes, odd and even, at scales 1, 1.5 and 2: every edge of the top card's outline is there
//! with its whole width of ink, and the stack moved by whole pixels draws the same pixels.
use amenui::el::{El, col};
use amenui::offscreen::Offscreen;
use amenui::style::{DARK, Look, Material};
use amenui::ui::Ui;
use amenui::widgets::{Stack, card_stack};

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }
const W: f32 = 400.0;
const H: f32 = 260.0;

struct S { w: f32, h: f32, at: f32 }
fn view(s: &S) -> El<()> { col().key("page").pad(s.at, s.at).child(card_stack("cs", &Stack::default(), s.w, s.h, |_| ())) }

/// The frame and its width.
fn shot(scale: f32, s: S) -> (Vec<u8>, u32) {
    let (w, h) = ((W * scale) as u32, (H * scale) as u32);
    let mut o = Offscreen::new(w, h);
    let mut u = Ui::new(s, view, |_, _| {}, fonts(), w, h, scale).on_look(|_| Look { material: Material::Plain, theme: DARK, ..Look::default() });
    u.tick(2.0);
    u.settle();
    let l = u.look();
    o.r.partial = false;
    (o.render(&mut u, &l), w)
}
/// How far the pixel at (x, y) of a frame `w` wide is from the card toward the ink (0: the card or the
/// ground, 1: the ink).
fn inkness(f: &[u8], w: u32, x: u32, y: u32) -> f32 {
    let (ink, raised, ground) = (DARK.ink[0] * 255.0, DARK.greys.raised[0] * 255.0, DARK.ground[0] * 255.0);
    let v = f[((y * w + x) * 4) as usize] as f32;
    ((v - raised) / (ink - raised)).max(((v - ground) / (ink - ground)).min(1.0)).max(0.0)
}

#[test]
fn every_row_of_a_supersampled_outline_is_drawn_at_whole_pixel_boxes() {
    if Offscreen::try_new(64, 64).is_err() { eprintln!("no GPU adapter here: skipped"); return; }
    for scale in [1.0f32, 1.5, 2.0] {
        for (bw, bh) in [(320.0, 200.0), (321.0, 201.0), (260.0, 170.0), (259.0, 169.0)] {
            // The box's corner on whole device pixels at every scale (at 1.5, an even logical number).
            let at = 20.0;
            let (f, fw) = shot(scale, S { w: bw, h: bh, at });
            let ink = |f: &[u8], x: u32, y: u32| inkness(f, fw, x, y);
            // Through the middle of the box, down and across: the ink each edge of the top card's
            // outline lays across the line, which is one logical px wide.
            let (cx, cy) = (((at + bw * 0.5) * scale) as u32, ((at + bh * 0.5) * scale) as u32);
            let (h, w) = (((at * 2.0 + bh) * scale) as u32, ((at * 2.0 + bw) * scale) as u32);
            let run = |pts: Vec<f32>| -> Vec<f32> {
                // Each run of pixels with any ink is one edge: its ink, summed.
                let mut edges = vec![];
                let mut sum = 0.0;
                for v in pts { if v > 0.1 { sum += v } else if sum > 0.0 { edges.push(sum); sum = 0.0; } }
                if sum > 0.0 { edges.push(sum); }
                edges
            };
            let down = run((0..h).map(|y| ink(&f, cx, y)).collect());
            let across = run((0..w).map(|x| ink(&f, x, cy)).collect());
            for (name, e) in [("top and bottom", &down), ("left and right", &across)] {
                assert_eq!(e.len(), 2, "scale {scale}, box {bw} × {bh}: the {name} edges, {e:?}");
                for v in e { assert!(*v > 0.85 * scale, "scale {scale}, box {bw} × {bh}: an edge ({name}) has {v:.2} px of ink, not {scale}: {e:?}"); }
            }
        }
    }
}

#[test]
fn a_3d_element_moved_by_whole_pixels_draws_the_same_pixels() {
    if Offscreen::try_new(64, 64).is_err() { eprintln!("no GPU adapter here: skipped"); return; }
    for scale in [1.0f32, 1.5, 2.0] {
        let (bw, bh) = (321.0, 201.0);
        let (a, w) = shot(scale, S { w: bw, h: bh, at: 20.0 });
        // Moved 2 logical px right and down: whole device pixels at each scale.
        let (b, _) = shot(scale, S { w: bw, h: bh, at: 22.0 });
        let d = (2.0 * scale) as u32;
        let (x0, y0, x1, y1) = ((20.0 * scale) as u32, (20.0 * scale) as u32, ((20.0 + bw) * scale) as u32, ((20.0 + bh) * scale) as u32);
        let mut differ = 0;
        for y in y0..y1 { for x in x0..x1 {
            let (i, j) = (((y * w + x) * 4) as usize, (((y + d) * w + x + d) * 4) as usize);
            if a[i..i + 3] != b[j..j + 3] { differ += 1; }
        } }
        assert_eq!(differ, 0, "scale {scale}: {differ} pixels of the stack differ once it is moved by {d} device px");
    }
}
