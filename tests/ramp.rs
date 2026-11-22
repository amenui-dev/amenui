//! A stroke with a ramp (`Drawing::ramp`, DESIGN.md §1.26): over a picture its stops' colours mix in
//! OKLab along the stroke, the opacity linear and applied after, each round end its end's colour; a
//! line without one is drawn as before; 540 of them are one draw; off a picture a ramp from one hue to
//! another keeps its first hue and is reported (`Ui::problems`).
use amenui::draw::Drawing;
use amenui::el::{El, Paint, canvas, col, picture};
use amenui::offscreen::Offscreen;
use amenui::scene::SHAPE;
use amenui::style::{DARK, Greys, Look, Material, Theme};
use amenui::ui::Ui;

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }
const W: u32 = 440;
const H: u32 = 200;
const PIC: u32 = 51;
/// The picture under the rays: one colour.
const SKY: [u8; 3] = [16, 26, 44];
const ROOT: [f32; 3] = [0xba as f32 / 255.0, 1.0, 0xd8 as f32 / 255.0];
const MID: [f32; 3] = [0x2f as f32 / 255.0, 0xae as f32 / 255.0, 0x8c as f32 / 255.0];
const TIP: [f32; 3] = [0x1f as f32 / 255.0, 0x5f as f32 / 255.0, 0x86 as f32 / 255.0];
/// W-04's ray: three stops, the middle at 0.3, opacity 1 → 0.55 → 0.
const RAY: (Paint, (Paint, f32), Paint) = (Paint::hue(0), (Paint { tone: 1.0, accent: 1.0, alpha: 0.55, hue: 1 }, 0.3), Paint { tone: 1.0, accent: 1.0, alpha: 0.0, hue: 2 });

/// A stroke: its ends, width, ramp (middle stop, far paint) if any, and its paint at the start.
type Stroke = ((f32, f32), (f32, f32), f32, Option<(Option<(Paint, f32)>, Paint)>, Paint);
#[derive(Clone, Default)]
struct Page { lines: Vec<Stroke>, over: bool }
fn view(p: &Page) -> El<()> {
    let mut d = Drawing::new(W as f32, H as f32);
    for &(a, b, w, ramp, paint) in &p.lines {
        match ramp { Some((mid, far)) => d.ramp(a, b, w, paint, mid, far), None => d.line(a, b, w, paint) }
    }
    let root = col().key("page").width(W as f32).height(H as f32);
    let root = if p.over { root.child(picture(PIC, H as f32 / W as f32).key("sky").width(W as f32).absolute(Some(0.0), Some(0.0), None, None)) } else { root };
    root.child(canvas(d).key("rays").width(W as f32).pass(true).absolute(Some(0.0), Some(0.0), None, None))
}
fn theme() -> Theme { Theme { accent: ROOT, second: MID, third: TIP, ground: [0.02, 0.03, 0.06], ground2: [0.02, 0.03, 0.06], greys: Greys::mixed([0.02, 0.03, 0.06], DARK.ink), ..DARK } }
fn open(p: Page) -> Ui<Page, ()> {
    let mut u = Ui::new(p, view, |_, _| {}, fonts(), W, H, 1.0).on_look(|_| Look { material: Material::Plain, theme: theme(), ..Look::default() });
    u.tick(3.0);
    u.settle();
    u
}
fn render(o: &mut Offscreen, u: &mut Ui<Page, ()>) -> Vec<u8> { let l = u.look(); o.r.partial = false; o.render(u, &l) }
fn px(f: &[u8], x: u32, y: u32) -> [f32; 3] { let k = ((y * W + x) * 4) as usize; [f[k] as f32, f[k + 1] as f32, f[k + 2] as f32] }
fn sky() -> Vec<u8> { (0..W * H).flat_map(|_| [SKY[0], SKY[1], SKY[2], 255]).collect() }

fn lin(c: f32) -> f32 { if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } }
fn srgb(c: f32) -> f32 { let c = c.clamp(0.0, 1.0); if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 } }
fn oklab(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}
fn from_oklab(c: [f32; 3]) -> [f32; 3] {
    let l = (c[0] + 0.396_337_78 * c[1] + 0.215_803_76 * c[2]).powi(3);
    let m = (c[0] - 0.105_561_346 * c[1] - 0.063_854_17 * c[2]).powi(3);
    let s = (c[0] - 0.089_484_18 * c[1] - 1.291_485_5 * c[2]).powi(3);
    [srgb(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s), srgb(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s), srgb(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s)]
}
/// W-04's ray at `t` over the sky, worked out by hand: OKLab between the stops, then the opacity.
fn ray_at(t: f32) -> [f32; 3] {
    let (c0, c1, a0, a1, f) = if t < 0.3 { (ROOT, MID, 1.0, 0.55, t / 0.3) } else { (MID, TIP, 0.55, 0.0, (t - 0.3) / 0.7) };
    let (l0, l1) = (oklab(c0), oklab(c1));
    let c = from_oklab(std::array::from_fn(|k| l0[k] + (l1[k] - l0[k]) * f));
    let a = a0 + (a1 - a0) * f;
    std::array::from_fn(|k| (c[k] * a + SKY[k] as f32 / 255.0 * (1.0 - a)) * 255.0)
}

#[test]
fn over_a_picture_a_ramp_mixes_its_stops_in_oklab_and_its_ends_are_flat() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    o.r.set_picture(PIC, W, H, &sky());
    let (p0, mid, far) = RAY;
    // 1 px wide, 360 px long, along a pixel row's centre; and 4 px wide for its ends.
    let page = Page { lines: vec![((40.0, 60.5), (400.0, 60.5), 1.0, Some((Some(mid), far)), p0), ((40.0, 140.0), (400.0, 140.0), 4.0, Some((Some(mid), far)), p0)], over: true };
    let mut u = open(page);
    let f = render(&mut o, &mut u);
    for k in 0..=10 {
        let x = (40 + 36 * k).min(399);
        let t = (x as f32 + 0.5 - 40.0) / 360.0;
        let (got, want) = (px(&f, x, 60), ray_at(t));
        assert!((0..3).all(|c| (got[c] - want[c]).abs() <= 2.0), "at {}0 % (x {x}): {got:?}, by hand {want:?}", k);
    }
    // Each round end is its end's colour, flat: the left cap is the root's, the right the tip's.
    let (cap0, cap1) = (ray_at(0.0), ray_at(1.0));
    for (x, want) in [(39, cap0), (400, cap1)] {
        let got = px(&f, x, 140);
        assert!((0..3).all(|c| (got[c] - want[c]).abs() <= 2.0), "the cap at x {x}: {got:?}, its end {want:?}");
    }
    assert!(u.problems().is_empty(), "over a picture nothing to report: {:?}", u.problems());
}

#[test]
fn a_line_without_a_ramp_is_the_instance_it_was_and_540_rays_are_one_draw() {
    let plain = open(Page { lines: vec![((10.0, 10.0), (100.0, 40.0), 1.5, None, Paint::INK)], over: false });
    let i = plain.scene().instances.iter().find(|i| i.paint[3] == SHAPE).copied().expect("the line");
    assert_eq!((i.uv[1], i.uv[2], i.uv[3], i.extra[0], i.extra[1]), (0.0, 0.0, 0.0, 0.0, 0.0), "a line without a ramp carries nothing more");
    let (p0, mid, far) = RAY;
    let lines = (0..540).map(|k| { let x = 20.0 + k as f32 * 0.74; ((x, 180.0), (x + 3.0, 180.0 - 60.0 - (k % 300) as f32 * 0.4), 1.0, Some((Some(mid), far)), p0) }).collect();
    let u = open(Page { lines, over: true });
    let rays = u.scene().instances.iter().filter(|i| i.paint[3] == SHAPE).count();
    assert_eq!(rays, 540, "one instance a ray");
    let draws = u.scene().batches.iter().filter(|b| (b.start..b.end).any(|k| u.scene().instances[k as usize].paint[3] == SHAPE)).count();
    assert_eq!(draws, 1, "540 rays in one draw");
}

#[test]
fn off_a_picture_a_ramp_across_hues_keeps_its_first_hue_and_is_reported() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let (p0, mid, far) = RAY;
    let mut u = open(Page { lines: vec![((40.0, 60.5), (400.0, 60.5), 1.0, Some((Some(mid), far)), p0)], over: false });
    assert!(u.problems().iter().any(|p| matches!(p, amenui::error::ViewProblem::RampHue { key } if key == "rays")), "reported: {:?}", u.problems());
    // Drawn in the root's hue all the way: no seam where a stop changes hue, only the opacity ramps.
    let f = render(&mut o, &mut u);
    let th = u.look().theme;
    let audit = amenui::audit::audit(&mut u, &f, &f, &th);
    assert!(audit.iter().any(|a| a.level == amenui::audit::Level::P1 && a.kind == amenui::audit::Kind::Structure && a.fix.contains("rays")), "the audit: {audit:?}");
    let g = theme().ground.map(|v| v * 255.0);
    for x in (41..399).step_by(6) {
        let (a, b) = (px(&f, x, 60), px(&f, x + 1, 60));
        assert!((0..3).all(|c| (a[c] - b[c]).abs() <= 3.0), "a step between x {x} and {}: {a:?} {b:?}", x + 1);
        let t = (x as f32 + 0.5 - 40.0) / 360.0;
        let al = if t < 0.3 { 1.0 - 0.45 * t / 0.3 } else { 0.55 * (1.0 - (t - 0.3) / 0.7) };
        let want: [f32; 3] = std::array::from_fn(|c| ROOT[c] * 255.0 * al + g[c] * (1.0 - al));
        // (Under 0.1 opaque the ink's 8 bits round a faint stroke's colour by more, as any faint stroke's.)
        if al >= 0.1 { assert!((0..3).all(|c| (a[c] - want[c]).abs() <= 3.0), "x {x}: {a:?}, the root's hue at {al}: {want:?}"); }
    }
    // Within one hue, nothing to report.
    let one = open(Page { lines: vec![((40.0, 60.5), (400.0, 60.5), 1.0, Some((None, Paint::hue(0).alpha(0.0))), Paint::hue(0))], over: false });
    assert!(one.problems().is_empty(), "{:?}", one.problems());
}
