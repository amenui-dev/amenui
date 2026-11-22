//! A drawing's own colours (`Drawing::palette`, DESIGN.md §1.26): over a picture its marks are drawn in
//! them whatever the theme, a ramp between two mixed in OKLab; off a picture they are hue 0 and
//! reported; the drawings of a frame share one table, the same colour counted once.
use amenui::draw::Drawing;
use amenui::el::{El, Paint, canvas, col, picture};
use amenui::error::ViewProblem;
use amenui::offscreen::Offscreen;
use amenui::scene::SHAPE;
use amenui::style::{DARK, LIGHT, Look, Material, Theme};
use amenui::ui::Ui;

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }
const W: u32 = 440;
const H: u32 = 200;
const PIC: u32 = 52;
const SKY: [u8; 3] = [7, 11, 22];
const ROOT: [f32; 3] = [0xba as f32 / 255.0, 1.0, 0xd8 as f32 / 255.0];
const MID: [f32; 3] = [0x2f as f32 / 255.0, 0xae as f32 / 255.0, 0x8c as f32 / 255.0];
const TIP: [f32; 3] = [0x1f as f32 / 255.0, 0x5f as f32 / 255.0, 0x86 as f32 / 255.0];
/// W-04's ray in its own colours: root, middle at 0.3, tip; opacity 1 → 0.55 → 0.
const RAY: (Paint, (Paint, f32), Paint) = (Paint::hue(3), (Paint { tone: 1.0, accent: 1.0, alpha: 0.55, hue: 4 }, 0.3), Paint { tone: 1.0, accent: 1.0, alpha: 0.0, hue: 5 });

#[derive(Clone, Default)]
struct Page { over: bool, light: bool, second: bool }
fn rays(key: &str, palette: Vec<[f32; 3]>, y: f32) -> El<()> {
    let mut d = Drawing::new(W as f32, H as f32);
    d.palette(palette);
    let (p0, mid, far) = RAY;
    d.ramp((40.0, y), (400.0, y), 1.0, p0, Some(mid), far);
    d.line((40.0, y + 40.0), (400.0, y + 40.0), 2.0, Paint::hue(5));
    canvas(d).key(key).width(W as f32).pass(true).absolute(Some(0.0), Some(0.0), None, None)
}
fn view(p: &Page) -> El<()> {
    let root = col().key("page").width(W as f32).height(H as f32);
    let root = if p.over { root.child(picture(PIC, H as f32 / W as f32).key("sky").width(W as f32).absolute(Some(0.0), Some(0.0), None, None)) } else { root };
    let root = root.child(rays("rays", vec![ROOT, MID, TIP], 60.5));
    // A second drawing that shares the tip and has a colour of its own, and a hue past its palette.
    if p.second {
        let mut d = Drawing::new(W as f32, H as f32);
        d.palette([TIP, [1.0, 0.5, 0.0]]);
        d.line((40.0, 150.0), (400.0, 150.0), 1.0, Paint::hue(3));
        d.line((40.0, 160.0), (400.0, 160.0), 1.0, Paint::hue(4));
        d.line((40.0, 170.0), (400.0, 170.0), 1.0, Paint::hue(9));
        return root.child(canvas(d).key("more").width(W as f32).pass(true).absolute(Some(0.0), Some(0.0), None, None));
    }
    root
}
/// Two themes with nothing of the ray's colours in them.
fn theme(light: bool) -> Theme { if light { LIGHT } else { Theme { accent: [0.9, 0.2, 0.2], second: [0.2, 0.2, 0.9], third: [0.9, 0.9, 0.1], ..DARK } } }
fn open(p: Page) -> Ui<Page, ()> {
    let mut u = Ui::new(p, view, |_, _| {}, fonts(), W, H, 1.0).on_look(|p| Look { material: Material::Plain, theme: theme(p.light), ..Look::default() });
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
fn ray_at(t: f32) -> [f32; 3] {
    let (c0, c1, a0, a1, f) = if t < 0.3 { (ROOT, MID, 1.0, 0.55, t / 0.3) } else { (MID, TIP, 0.55, 0.0, (t - 0.3) / 0.7) };
    let (l0, l1) = (oklab(c0), oklab(c1));
    let c = from_oklab(std::array::from_fn(|k| l0[k] + (l1[k] - l0[k]) * f));
    let a = a0 + (a1 - a0) * f;
    std::array::from_fn(|k| (c[k] * a + SKY[k] as f32 / 255.0 * (1.0 - a)) * 255.0)
}

#[test]
fn over_a_picture_a_drawing_is_in_its_own_colours_whatever_the_theme() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    o.r.set_picture(PIC, W, H, &sky());
    let mut frames = vec![];
    for light in [false, true] {
        let mut u = open(Page { over: true, light, second: false });
        assert!(u.problems().is_empty(), "over a picture nothing to report: {:?}", u.problems());
        let f = render(&mut o, &mut u);
        // Root to middle to tip across three hues no theme has, in OKLab, the opacity after.
        for k in 0..=10 {
            let x = (40 + 36 * k).min(399);
            let t = (x as f32 + 0.5 - 40.0) / 360.0;
            let (got, want) = (px(&f, x, 60), ray_at(t));
            assert!((0..3).all(|c| (got[c] - want[c]).abs() <= 2.0), "light {light}, {k}0 % (x {x}): {got:?}, by hand {want:?}");
        }
        // The tip in full: #1f5f86.
        let got = px(&f, 200, 100);
        assert!((0..3).all(|c| (got[c] - TIP[c] * 255.0).abs() <= 1.0), "light {light}: the tip line is {got:?}");
        frames.push(f);
    }
    let row = |f: &[u8], y: u32| (0..W).map(|x| px(f, x, y)).collect::<Vec<_>>();
    assert_eq!(row(&frames[0], 60), row(&frames[1], 60), "the same ray under either theme");
}

#[test]
fn off_a_picture_its_own_colours_are_hue_0_and_reported() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let mut u = open(Page::default());
    assert!(u.problems().iter().any(|p| matches!(p, ViewProblem::OwnColourOffPicture { key } if key == "rays")), "reported: {:?}", u.problems());
    let f = render(&mut o, &mut u);
    let th = u.look().theme;
    let audit = amenui::audit::audit(&mut u, &f, &f, &th);
    assert!(audit.iter().any(|a| a.level == amenui::audit::Level::P1 && a.fix.contains("rays") && a.fix.contains("palette")), "the audit: {audit:?}");
    // The tip line in the theme's first accent.
    let got = px(&f, 200, 100);
    let want = theme(false).accent.map(|v| v * 255.0);
    assert!((0..3).all(|c| (got[c] - want[c]).abs() <= 2.0), "drawn in hue 0: {got:?}, {want:?}");
}

#[test]
fn the_drawings_of_a_frame_share_one_table() {
    let u = open(Page { over: true, light: false, second: true });
    // As first drawn: the ramp's start, its far stop, its middle; the second drawing's orange.
    assert_eq!(u.scene().own, vec![ROOT, TIP, MID, [1.0, 0.5, 0.0]], "the tip counted once");
    let hues: Vec<f32> = u.scene().instances.iter().filter(|i| i.paint[3] == SHAPE).map(|i| i.look[1]).collect();
    // The ramp starts at the root (3), the tip line (4); then the tip again (4), the orange (6), and
    // past the palette hue 0.
    assert_eq!(hues, vec![3.0, 4.0, 4.0, 6.0, 0.0]);
}

/// A band that clips a picture and the rays over it (W-04 in its phone band): a ray that runs out of
/// the band is cut at its edge, so what is drawn of it is on the picture; with a picture shorter than
/// the band, the ray's part under it is drawn off the picture.
fn band(pic_h: f32) -> El<()> {
    let mut d = Drawing::new(W as f32, H as f32);
    d.palette(vec![ROOT, MID, TIP]);
    let (p0, mid, far) = RAY;
    d.ramp((40.0, 20.0), (400.0, 180.0), 1.0, p0, Some(mid), far);
    d.line((40.0, 30.0), (400.0, 190.0), 2.0, Paint::hue(5));
    col().key("page").width(W as f32).height(H as f32).child(col().key("band").width(W as f32).height(100.0).clip()
        .child(picture(PIC, pic_h / W as f32).key("sky").width(W as f32).absolute(Some(0.0), Some(0.0), None, None))
        .child(canvas(d).key("rays").width(W as f32).pass(true).absolute(Some(0.0), Some(0.0), None, None)))
}

#[test]
fn a_mark_cut_by_its_clip_is_on_the_picture_when_what_is_drawn_of_it_is() {
    for (pic_h, reported) in [(100.0f32, false), (60.0, true)] {
        let mut u = Ui::new(pic_h, |h: &f32| band(*h), |_, _| {}, fonts(), W, H, 1.0).on_look(|_| Look { material: Material::Plain, theme: theme(false), ..Look::default() });
        u.tick(3.0);
        u.settle();
        let p = u.problems();
        let own = p.iter().any(|p| matches!(p, ViewProblem::OwnColourOffPicture { key } if key == "rays"));
        let ramp = p.iter().any(|p| matches!(p, ViewProblem::RampHue { key } if key == "rays"));
        assert_eq!((own, ramp), (reported, reported), "a picture {pic_h} px tall in a band 100 px tall: {p:?}");
    }
}
