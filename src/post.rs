//! The finishing pass (DESIGN.md §1.21): what happens to the finished frame on its way to the screen,
//! in a fixed order — bloom → chromatic offset → scanlines → grain → vignette → curvature — and then
//! the frame's quantisation: an ordered dither and the nearest colour of a palette. Every effect is off
//! by default; with all of them off the frame is copied out as before (no extra pass, no extra cost).
//! The pass reads the kept frame and writes the target, so what the runtime compares, keeps and
//! commits from (DESIGN.md §1.19, §4.1) is the frame before it. Plain data, serialised in presets and
//! recipes. No GPU here.
use serde::{Deserialize, Serialize};

/// How the frame is dithered before it is quantised.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DithrKind {
    #[default]
    None,
    /// Ordered dither by the 2×2, 4×4 and 8×8 Bayer matrices.
    Bayer2,
    Bayer4,
    Bayer8,
    /// A low-discrepancy threshold (the R2 sequence, `fract(0.7549 x + 0.5698 y)`): even like blue
    /// noise, with no texture to ship. Not a true void-and-cluster mask.
    BlueNoise,
    /// An ordered stand-in for Atkinson's error diffusion: the 4×4 Bayer thresholds pulled in to
    /// 1/8–7/8, so as with Atkinson (which passes on only 3/4 of the error) the darkest and lightest
    /// quarters go solid and the mid-tones carry the pattern. True error diffusion runs pixel after
    /// pixel and does not fit one fragment pass.
    AtkinsonLike,
}
impl DithrKind {
    pub const ALL: [DithrKind; 6] = [DithrKind::None, DithrKind::Bayer2, DithrKind::Bayer4, DithrKind::Bayer8, DithrKind::BlueNoise, DithrKind::AtkinsonLike];
    pub fn id(self) -> f32 { Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as f32 }
    /// The threshold (0–1) at device pixel (x, y); the shader computes the same.
    pub fn threshold(self, x: u32, y: u32) -> f32 {
        // The recursive Bayer matrix: each bit of (x, y), lowest first, picks a quadrant (0 2 / 3 1)
        // that weighs most.
        let bayer = |n: u32| -> f32 {
            let levels = n.trailing_zeros();
            let mut v = 0u32;
            for i in 0..levels {
                let d = match ((x >> i) & 1, (y >> i) & 1) { (0, 0) => 0, (1, 0) => 2, (0, 1) => 3, _ => 1 };
                v += d << (2 * (levels - 1 - i));
            }
            (v as f32 + 0.5) / (n * n) as f32
        };
        match self {
            DithrKind::None => 0.5,
            DithrKind::Bayer2 => bayer(2),
            DithrKind::Bayer4 => bayer(4),
            DithrKind::Bayer8 => bayer(8),
            DithrKind::BlueNoise => (x as f32 * 0.754_877_7 + y as f32 * 0.569_840_3).fract(),
            DithrKind::AtkinsonLike => 0.125 + 0.75 * bayer(4),
        }
    }
}

/// The dither: its kind, how many levels each channel keeps when there is no palette (2–16), and how
/// strong the pattern is (0–1; 1 is a full quantisation step).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dithr { pub kind: DithrKind, pub levels: u8, pub strength: f32 }
impl Default for Dithr { fn default() -> Self { Self { kind: DithrKind::None, levels: 4, strength: 1.0 } } }

/// The fixed palettes, as data (sRGB bytes). All are this project's own values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteKind {
    #[default]
    Off,
    /// Near-black and paper.
    OneBit,
    /// Four mossy greens, dark to light.
    Moss4,
    /// A phosphor amber in four steps.
    Amber,
    /// Ink, cyan, magenta, white.
    CyanMagenta,
    /// Black, green, red and yellow, a four-colour adapter's other set.
    Cga4,
    /// Eight named originals: ink, paper, brick, ochre, moss, slate, plum, sky.
    Originals8,
    /// `Palette::colors`.
    User,
}
impl PaletteKind {
    pub const ALL: [PaletteKind; 8] = [PaletteKind::Off, PaletteKind::OneBit, PaletteKind::Moss4, PaletteKind::Amber, PaletteKind::CyanMagenta, PaletteKind::Cga4, PaletteKind::Originals8, PaletteKind::User];
    /// The colours of a fixed palette (empty for `Off` and `User`).
    pub fn colors(self) -> &'static [[u8; 3]] {
        match self {
            PaletteKind::Off | PaletteKind::User => &[],
            PaletteKind::OneBit => &[[18, 18, 20], [236, 234, 226]],
            PaletteKind::Moss4 => &[[22, 38, 26], [60, 92, 52], [132, 158, 80], [204, 212, 148]],
            PaletteKind::Amber => &[[18, 9, 2], [104, 48, 4], [214, 118, 12], [255, 194, 92]],
            PaletteKind::CyanMagenta => &[[14, 14, 22], [0, 196, 214], [226, 42, 166], [244, 244, 240]],
            PaletteKind::Cga4 => &[[0, 0, 0], [72, 200, 104], [214, 74, 66], [236, 212, 104]],
            PaletteKind::Originals8 => &ORIGINAL_COLORS,
        }
    }
}
/// The eight originals, by name.
pub const ORIGINALS: [(&str, [u8; 3]); 8] = [
    ("ink", [26, 24, 30]), ("paper", [238, 232, 218]), ("brick", [176, 72, 52]), ("ochre", [214, 160, 64]),
    ("moss", [92, 128, 72]), ("slate", [70, 96, 128]), ("plum", [120, 70, 110]), ("sky", [150, 196, 220]),
];
const ORIGINAL_COLORS: [[u8; 3]; 8] = [ORIGINALS[0].1, ORIGINALS[1].1, ORIGINALS[2].1, ORIGINALS[3].1, ORIGINALS[4].1, ORIGINALS[5].1, ORIGINALS[6].1, ORIGINALS[7].1];
/// The most colours a palette holds.
pub const PALETTE_MAX: usize = 16;

/// A palette: a fixed one, or the user's own colours (at most `PALETTE_MAX`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Palette { pub kind: PaletteKind, #[serde(default)] pub colors: Vec<[u8; 3]> }
impl Palette {
    /// The colours in use (none: no palette).
    pub fn resolved(&self) -> Vec<[u8; 3]> {
        match self.kind { PaletteKind::User => self.colors.iter().take(PALETTE_MAX).copied().collect(), k => k.colors().to_vec() }
    }
}

/// A glow around what is brighter than `threshold` (0–1), `radius` logical px, added by `strength`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bloom { pub strength: f32, pub threshold: f32, pub radius: f32 }
impl Default for Bloom { fn default() -> Self { Self { strength: 0.0, threshold: 0.7, radius: 8.0 } } }
/// Red and blue read `offset` logical px apart, outward from the centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Chroma { pub offset: f32 }
/// Dark lines every `period` logical px, `strength` deep (0–1).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scanlines { pub strength: f32, pub period: f32 }
impl Default for Scanlines { fn default() -> Self { Self { strength: 0.0, period: 3.0 } } }
/// A fixed film grain (it does not crawl: a still page is a still image), `amount` 0–1, in cells of
/// `size` logical px.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grain { pub amount: f32, pub size: f32 }
impl Default for Grain { fn default() -> Self { Self { amount: 0.0, size: 1.0 } } }
/// The corners darken by `strength` (0–1) from `radius` (a fraction of the half-diagonal) outward.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vignette { pub strength: f32, pub radius: f32 }
impl Default for Vignette { fn default() -> Self { Self { strength: 0.0, radius: 0.55 } } }
/// A barrel bend: a point at distance d from the centre (1 at the half-diagonal) shows what is at
/// d × (1 + amount × d²); beyond the frame is black.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Curvature { pub amount: f32 }

/// The finishing pass, as data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PostSpec {
    #[serde(default)] pub bloom: Bloom,
    #[serde(default)] pub chroma: Chroma,
    #[serde(default)] pub scanlines: Scanlines,
    #[serde(default)] pub grain: Grain,
    #[serde(default)] pub vignette: Vignette,
    #[serde(default)] pub curvature: Curvature,
    #[serde(default)] pub dithr: Dithr,
    #[serde(default)] pub palette: Palette,
    /// Two colours only (DESIGN.md §1.21): the 1-bit palette with no dither, whatever `dither` and
    /// `palette` say, and scribblr's threads a whole number of device px wide.
    #[serde(default)] pub binary: bool,
    /// The whole frame turned to its negative (`1 − colour`, in sRGB), the very last thing done.
    #[serde(default)] pub invert: bool,
    /// Text to read kept clear of the screen's artefacts: on its ink and within 2 device px of it the
    /// chroma offset is 0 and the scanlines are `CLEAR_SCANLINES` (pictures, shapes, the ground and text
    /// not to read keep the whole pass; curvature, vignette and grain apply to text as to the rest).
    #[serde(default)] pub clear_text: bool,
}
/// The scanlines' strength on text to read under `PostSpec::clear_text`.
pub const CLEAR_SCANLINES: f32 = 0.08;
impl PostSpec {
    /// The fixed-size form the look carries.
    pub fn look(&self) -> Post {
        let cols = if self.binary { PaletteKind::OneBit.colors().to_vec() } else { self.palette.resolved() };
        let mut colors = [[0u8; 3]; PALETTE_MAX];
        for (i, c) in cols.iter().enumerate() { colors[i] = *c; }
        let dithr = if self.binary { Dithr { kind: DithrKind::None, ..self.dithr } } else { self.dithr };
        Post { bloom: self.bloom, chroma: self.chroma, scanlines: self.scanlines, grain: self.grain, vignette: self.vignette, curvature: self.curvature, dithr, colors, count: cols.len() as u8, binary: self.binary, invert: self.invert, clear_text: self.clear_text, region: None, bloom_region: None }
    }
}

/// What `Look` carries: the same, the palette resolved to at most `PALETTE_MAX` colours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Post {
    pub bloom: Bloom, pub chroma: Chroma, pub scanlines: Scanlines, pub grain: Grain, pub vignette: Vignette, pub curvature: Curvature,
    pub dithr: Dithr, pub colors: [[u8; 3]; PALETTE_MAX], pub count: u8, pub binary: bool, pub invert: bool,
    /// `PostSpec::clear_text`.
    pub clear_text: bool,
    /// Where the grain is drawn (device px, x0 y0 x1 y1; set by the runtime from `El::grain`): `None`
    /// over the whole frame.
    pub region: Option<[f32; 4]>,
    /// Where the bloom glows and what it glows from (device px, x0 y0 x1 y1; set by the runtime from
    /// `El::bloom`): `None` over the whole frame.
    pub bloom_region: Option<[f32; 4]>,
}
impl Post {
    pub fn bloom_on(&self) -> bool { self.bloom.strength > 0.0 && self.bloom.radius > 0.0 }
    pub fn chroma_on(&self) -> bool { self.chroma.offset != 0.0 }
    pub fn scanlines_on(&self) -> bool { self.scanlines.strength > 0.0 && self.scanlines.period >= 1.0 }
    pub fn grain_on(&self) -> bool { self.grain.amount > 0.0 }
    pub fn vignette_on(&self) -> bool { self.vignette.strength > 0.0 }
    pub fn curvature_on(&self) -> bool { self.curvature.amount != 0.0 }
    pub fn dithr_on(&self) -> bool { self.dithr.kind != DithrKind::None && self.dithr.strength > 0.0 }
    pub fn palette_on(&self) -> bool { self.count > 0 }
    /// Whether the finishing pass runs at all (otherwise the frame is copied out unchanged).
    pub fn active(&self) -> bool {
        self.bloom_on() || self.chroma_on() || self.scanlines_on() || self.grain_on() || self.vignette_on() || self.curvature_on() || self.dithr_on() || self.palette_on() || self.invert
    }
    /// The uniform form (device px where lengths): seven vec4s (the fifth's x is `invert`, y `binary`, z
    /// whether the grain has a region, w whether the bloom has one; the sixth the grain's region, the
    /// seventh the bloom's, the eighth's x whether text to read is kept clear and y its scanlines), then
    /// the palette's sixteen.
    pub fn pack(&self, scale: f32) -> ([[f32; 4]; 8], [[f32; 4]; PALETTE_MAX]) {
        let f = |b: bool| if b { 1.0 } else { 0.0 };
        let d = &self.dithr;
        let mut p = [[0.0; 4]; 8];
        p[0] = [if self.dithr_on() { d.kind.id() } else { 0.0 }, d.levels.clamp(2, 16) as f32, self.count as f32, f(self.active())];
        p[1] = [if self.bloom_on() { self.bloom.strength } else { 0.0 }, self.bloom.threshold, self.bloom.radius * scale, d.strength.clamp(0.0, 1.0)];
        p[2] = [self.chroma.offset * scale, if self.scanlines_on() { self.scanlines.strength.clamp(0.0, 1.0) } else { 0.0 }, self.scanlines.period.max(1.0) * scale, self.grain.amount.max(0.0)];
        p[3] = [self.vignette.strength.clamp(0.0, 1.0), self.vignette.radius.clamp(0.0, 1.5), self.curvature.amount, (self.grain.size.max(0.25) * scale).max(1.0)];
        p[4] = [f(self.invert), f(self.binary), f(self.region.is_some()), f(self.bloom_region.is_some())];
        p[5] = self.region.unwrap_or([0.0; 4]);
        p[6] = self.bloom_region.unwrap_or([0.0; 4]);
        p[7] = [f(self.clear_text && (self.chroma_on() || self.scanlines_on())), CLEAR_SCANLINES, 0.0, 0.0];
        let mut pal = [[0.0; 4]; PALETTE_MAX];
        for (i, c) in self.colors.iter().take(self.count as usize).enumerate() { pal[i] = [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]; }
        (p, pal)
    }
}

/// The nearest palette colour to an sRGB colour (0–255), by squared distance with the eye's weights
/// (the shader uses the same).
pub fn nearest(colors: &[[u8; 3]], c: [f32; 3]) -> [u8; 3] {
    let w = [0.30, 0.59, 0.11];
    *colors.iter().min_by(|a, b| {
        let d = |p: &[u8; 3]| (0..3).map(|i| w[i] * (p[i] as f32 / 255.0 - c[i]).powi(2)).sum::<f32>();
        d(a).total_cmp(&d(b))
    }).unwrap_or(&[0, 0, 0])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bayer_thresholds_are_a_permutation() {
        for (k, n) in [(DithrKind::Bayer2, 2u32), (DithrKind::Bayer4, 4), (DithrKind::Bayer8, 8)] {
            let mut v: Vec<u32> = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).map(|(x, y)| (k.threshold(x, y) * (n * n) as f32 - 0.5).round() as u32).collect();
            v.sort();
            assert_eq!(v, (0..n * n).collect::<Vec<_>>(), "{k:?}");
            // Tiles: the pattern repeats every n.
            assert_eq!(k.threshold(1, 0), k.threshold(1 + n, n));
        }
        // The classic 2×2: 0 2 / 3 1 (of 4).
        assert_eq!([DithrKind::Bayer2.threshold(0, 0), DithrKind::Bayer2.threshold(1, 0), DithrKind::Bayer2.threshold(0, 1), DithrKind::Bayer2.threshold(1, 1)], [0.125, 0.625, 0.875, 0.375]);
        let a: Vec<f32> = (0..16).map(|i| DithrKind::AtkinsonLike.threshold(i % 4, i / 4)).collect();
        assert!(a.iter().all(|t| (0.125..=0.875).contains(t)));
        let b: Vec<f32> = (0..64).map(|i| DithrKind::BlueNoise.threshold(i % 8, i / 8)).collect();
        assert!(b.iter().all(|t| (0.0..1.0).contains(t)));
    }
    #[test]
    fn palettes_and_round_trip() {
        assert_eq!(PaletteKind::Originals8.colors().len(), 8);
        for k in PaletteKind::ALL { assert!(k.colors().len() <= PALETTE_MAX); }
        let p = PostSpec { palette: Palette { kind: PaletteKind::User, colors: vec![[1, 2, 3], [250, 250, 250]] }, dithr: Dithr { kind: DithrKind::Bayer4, ..Dithr::default() }, ..PostSpec::default() };
        let j = serde_json::to_string(&p).expect("json");
        assert_eq!(serde_json::from_str::<PostSpec>(&j).expect("parses"), p);
        let l = p.look();
        assert!(l.active() && l.palette_on() && l.count == 2);
        assert!(!PostSpec::default().look().active(), "everything off by default");
        assert_eq!(nearest(PaletteKind::OneBit.colors(), [0.9, 0.9, 0.9]), [236, 234, 226]);
    }
}
