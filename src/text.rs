//! Text: shaping and line breaking by parley; every glyph is then kept as its own instance.
//! Each glyph is rasterised once into an atlas that stores two things per texel: coverage, and how
//! close that texel is to the glyph's outline, signed (128 on it, 255 at `SPREAD` pixels inside, 0 at
//! `SPREAD` pixels outside).
//!
//! Several font faces can be registered; each text element picks one. A face can be marked as a pixel
//! face: it is then drawn only at whole multiples of its native size, on whole device pixels, with
//! coverage cut at one half, so its edges are ink or ground and nothing in between.
#![warn(missing_docs)]
use crate::scene::{GLYPH, Instance};
use crate::wrap::Around;
use parley::fontique::{Blob, Collection, CollectionOptions};
use parley::layout::PositionedLayoutItem;
use parley::style::{FontFamily, FontFamilyName, StyleProperty};
use parley::{FontContext, Layout, LayoutContext, LineHeight};
use crate::fasthash::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};

/// How far the glyph atlas's nearness-to-ink reaches beyond the outline, px (the padding every
/// glyph slot carries).
pub const SPREAD: i32 = 12;
/// The glyph atlas's side, px.
pub const ATLAS: u32 = 2048;
/// The whole atlas, as `Text::dirty` marks it.
pub const WHOLE: [u32; 4] = [0, 0, ATLAS, ATLAS];
/// Paragraph layouts kept per generation of the layout cache (DESIGN.md §1.13). A page's source listing is
/// a paragraph per line, so this is well above the longest page.
pub const LAYOUT_CACHE: usize = 8192;

/// The faces that ship in `assets/`, in the order `bundled` registers them: Noto Sans SC (outline).
pub const SANS: u8 = 0;
/// Fusion Pixel 12 px (pixel; Latin and Chinese).
pub const PIXEL: u8 = 1;
/// Departure Mono 11 px (pixel, monospaced).
pub const MONO: u8 = 2;
/// `SANS`'s file in `assets/`.
pub const SANS_FILE: &str = "NotoSansSC-Regular.otf";
/// `PIXEL`'s file in `assets/`.
pub const PIXEL_FILE: &str = "FusionPixel-12px-Proportional-zh_hans.otf";
/// `MONO`'s file in `assets/`.
pub const MONO_FILE: &str = "DepartureMono-Regular.otf";

/// Geist Pixel's five shapes (Latin only, outline faces): Square, Grid, Circle, Triangle, Line, after
/// the three above. Selectable as `Font::Face(GEIST_PIXEL[i])`.
pub const GEIST_PIXEL: [u8; 5] = [3, 4, 5, 6, 7];
/// Geist Pixel's shapes by name, in `GEIST_PIXEL` order.
pub const GEIST_NAMES: [&str; 5] = ["Square", "Grid", "Circle", "Triangle", "Line"];
/// Geist Pixel's files in `assets/`, in `GEIST_PIXEL` order.
pub const GEIST_FILES: [&str; 5] = ["GeistPixel-Square.otf", "GeistPixel-Grid.otf", "GeistPixel-Circle.otf", "GeistPixel-Triangle.otf", "GeistPixel-Line.otf"];
/// Source Serif 4 at its 8 pt optical size and weight 400, Latin only (OFL; `assets/licenses/source-serif`),
/// after Geist Pixel: `Font::Whisper`, tracked small lowercase type and edge notes (DESIGN.md §4.2).
pub const SERIF: u8 = 8;
/// A face number with this bit set (`face | FINE`): text drawn into a cached subtree (`El::cached`, a
/// page shown small and read through a loupe). A pixel face asked for there under its native size is
/// drawn at the size asked, its outline's coverage (a box filter of its native raster), not at the
/// native size; at its native size and above it is a pixel face as anywhere.
pub const FINE: u8 = 0x80;
/// `SERIF`'s file in `assets/`.
pub const SERIF_FILE: &str = "SourceSerif4-8pt-subset.ttf";

/// A font file to register. `pixel` is the native size (px) of a pixel face. A `fallback` face is
/// tried, after the face asked for and the first face, for characters they lack (Arabic, Hebrew,
/// Thai, Devanagari, emoji…), in the order given.
pub struct Face {
    /// The font file's bytes (empty for `Face::system`).
    pub data: Vec<u8>,
    /// The native size of a pixel face, px; `None` for an outline face.
    pub pixel: Option<f32>,
    /// Used only for characters the other faces lack.
    pub fallback: bool,
    /// A face declared to come later (`Face::declared`): its name and stand-in.
    declared: Option<(String, FaceDecl)>,
}
impl Face {
    /// An outline face, drawn at any size.
    pub fn outline(data: Vec<u8>) -> Self { Self { data, pixel: None, fallback: false, declared: None } }
    /// A pixel face whose glyphs are drawn at whole multiples of `size` px.
    pub fn pixel(data: Vec<u8>, size: f32) -> Self { Self { data, pixel: Some(size), fallback: false, declared: None } }
    /// A face used only for characters the others lack.
    pub fn fallback(data: Vec<u8>) -> Self { Self { data, pixel: None, fallback: true, declared: None } }
    /// A face whose bytes come later (`Ui::declare_face`, DESIGN.md §1.6.5), numbered here in order:
    /// text in it is set in `decl.stand_in` until `Ui::add_face` brings it.
    pub fn declared(name: &str, decl: FaceDecl) -> Self { Self { data: vec![], pixel: None, fallback: false, declared: Some((name.to_owned(), decl)) } }
    /// The system's installed fonts, after every given face, for characters none of them has (native
    /// builds with the `system-fonts` feature; elsewhere it is ignored). The bundled faces still decide
    /// everything they cover, so pages look the same on every machine for the text they were made with.
    pub fn system() -> Self { Self { data: vec![], pixel: None, fallback: true, declared: None } }
    /// Put it last: faces are numbered in the order given, and this one takes no number.
    fn is_system(&self) -> bool { self.fallback && self.data.is_empty() }
}
/// Noto Sans SC, Fusion Pixel 12 px, Departure Mono (11 px), as `SANS`, `PIXEL`, `MONO`.
pub fn bundled(sans: Vec<u8>, pixel: Vec<u8>, mono: Vec<u8>) -> Vec<Face> {
    vec![Face::outline(sans), Face::pixel(pixel, 12.0), Face::pixel(mono, 11.0)]
}
/// The bundled faces with Geist Pixel's five shapes after them (`GEIST_PIXEL`).
pub fn with_geist(mut faces: Vec<Face>, geist: impl IntoIterator<Item = Vec<u8>>) -> Vec<Face> {
    faces.extend(geist.into_iter().map(Face::outline));
    faces
}
/// The faces with the serif after them (`SERIF`; call after `with_geist`).
pub fn with_serif(mut faces: Vec<Face>, serif: Vec<u8>) -> Vec<Face> {
    faces.push(Face::outline(serif));
    faces
}
/// The bundled faces with Geist Pixel's five shapes and the serif declared to come later, numbered as
/// `with_geist` and `with_serif` number them: what the web page starts with, so the module carries only
/// the three main faces. Each is named by its file in `assets/` (`GEIST_FILES`, `SERIF_FILE`), which is
/// what `Ui::faces_wanted` asks for; Geist Pixel stands in as the pixel face, the serif as the sans.
pub fn with_declared(mut faces: Vec<Face>) -> Vec<Face> {
    faces.extend(GEIST_FILES.iter().map(|f| Face::declared(f, FaceDecl { stand_in: PIXEL, scale: 1.0 })));
    faces.push(Face::declared(SERIF_FILE, FaceDecl { stand_in: SANS, scale: 1.0 }));
    faces
}
/// Where the bundled fonts are on disk: this crate's `assets/` (a checkout, or Cargo's copy of it), for
/// tests and tools of applications built on amenui (`load_bundled(text::ASSETS)`).
#[cfg(not(target_arch = "wasm32"))]
pub const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
/// Read the bundled faces from a directory, Geist Pixel and the serif included (native only; the web
/// page passes the three main faces' bytes in and adds Geist Pixel and the serif when they are wanted,
/// `with_declared`).
#[cfg(not(target_arch = "wasm32"))]
pub fn load_bundled(dir: &str) -> Result<Vec<Face>, crate::error::FontError> {
    let r = |f: &str| { let path = format!("{dir}/{f}"); std::fs::read(&path).map_err(|source| crate::error::FontError::Missing { path, source }) };
    let geist = GEIST_FILES.iter().map(|f| r(f)).collect::<Result<Vec<_>, _>>()?;
    Ok(with_serif(with_geist(bundled(r(SANS_FILE)?, r(PIXEL_FILE)?, r(MONO_FILE)?), geist), r(SERIF_FILE)?))
}
/// Every bundled face, compiled into the binary (native only): what a native application ships with,
/// so it finds its fonts on any machine. About 14 MB, most of it the two Chinese faces.
#[cfg(not(target_arch = "wasm32"))]
pub fn embedded() -> Vec<Face> {
    let main = bundled(include_bytes!("../assets/NotoSansSC-Regular.otf").to_vec(), include_bytes!("../assets/FusionPixel-12px-Proportional-zh_hans.otf").to_vec(),
        include_bytes!("../assets/DepartureMono-Regular.otf").to_vec());
    with_serif(with_geist(main, geist_bytes()), serif_bytes())
}
/// The serif, compiled in (native builds; the web page adds it when it is wanted, `Ui::add_face`).
#[cfg(not(target_arch = "wasm32"))]
pub fn serif_bytes() -> Vec<u8> { include_bytes!("../assets/SourceSerif4-8pt-subset.ttf").to_vec() }
/// Geist Pixel's five faces, compiled in (native builds; the web page adds them when they are wanted).
#[cfg(not(target_arch = "wasm32"))]
pub fn geist_bytes() -> [Vec<u8>; 5] {
    [include_bytes!("../assets/GeistPixel-Square.otf").to_vec(), include_bytes!("../assets/GeistPixel-Grid.otf").to_vec(), include_bytes!("../assets/GeistPixel-Circle.otf").to_vec(),
     include_bytes!("../assets/GeistPixel-Triangle.otf").to_vec(), include_bytes!("../assets/GeistPixel-Line.otf").to_vec()]
}

/// A paragraph to draw: text, face, font size, line height and the width it was laid out at
/// (logical px; infinite for text kept on one line). `track` is extra letter spacing in em
/// (`El::tracking`), laid out by parley so measuring, drawing, carets and selections all see it.
#[derive(Clone, Copy)]
pub struct Para<'a> {
    /// The text.
    pub s: &'a str,
    /// The face's number.
    pub face: u8,
    /// Font size.
    pub size: f32,
    /// Line height.
    pub line: f32,
    /// The width it is broken at.
    pub width: f32,
    /// The pitch of the lattice the glyphs are printed through (`El::print`; 0: none).
    pub print: f32,
    /// A shape the lines flow around (`El::around`).
    pub around: Option<&'a Around>,
    /// Extra letter spacing, em.
    pub track: f32,
}
impl<'a> Para<'a> {
    /// How it is set beyond its size and width.
    pub fn setting(&self) -> Setting<'a> { Setting { around: self.around, track: self.track } }
}
/// How a paragraph is set beyond its size and width: what it flows around and its tracking (em).
#[derive(Clone, Copy, Default)]
pub struct Setting<'a> {
    /// A shape the lines flow around.
    pub around: Option<&'a Around>,
    /// Extra letter spacing, em.
    pub track: f32,
}

#[derive(Clone, Copy)]
struct Slot { x: u32, y: u32, w: u32, h: u32, left: i32, top: i32 }

/// Text, face, size bits, line bits, width bits, what it flows around, tracking bits.
type MeasureKey = (String, u8, u32, u32, u32, u64, u32);
/// A layout's key: `MeasureKey` with the text's hash in place of the text, so a look-up allocates nothing;
/// the entry keeps the text, and one whose text is not the one asked for is a miss.
type LayoutKey = (u64, u8, u32, u32, u32, u64, u32);
type Laid = (Box<str>, Rc<Layout<()>>);
/// A text's pieces for `El::reveal_by`: each piece's boxes.
pub type Pieces = Rc<Vec<Vec<crate::ui::Rect>>>;

/// A registered face: its family name in the collection, what the host calls it, and, while its bytes
/// have not come (`Text::declare`), the face standing in for it.
struct Family { name: String, called: String, pixel: Option<f32>, data: Arc<Vec<u8>>, blob: u64, fallback: bool, pending: Option<FaceDecl>, broken: bool }

/// A face that is declared before its bytes come (`Ui::declare_face`): text set in it is set in
/// `stand_in` at `scale` times its size (the line height stays the theme's) until it arrives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceDecl {
    /// The face already there that stands in for it.
    pub stand_in: u8,
    /// The stand-in's size as a multiple of the size asked for (1 by default; a pixel stand-in still
    /// keeps to whole multiples of its native size).
    pub scale: f32,
}
impl Default for FaceDecl { fn default() -> Self { Self { stand_in: SANS, scale: 1.0 } } }

/// One glyph rasterised on its own (for the mark atlas): coverage rows, placement, and the face's
/// advance and line box at that size, px.
pub struct Raster {
    /// Coverage, one byte a pixel, `w` × `h`, row by row.
    pub data: Vec<u8>,
    /// Width, px.
    pub w: u32,
    /// Height, px.
    pub h: u32,
    /// From the pen position to the bitmap's left edge, px.
    pub left: i32,
    /// From the baseline up to the bitmap's top edge, px.
    pub top: i32,
    /// How far the pen moves after the glyph, px.
    pub advance: f32,
    /// The face's ascent at this size, px.
    pub ascent: f32,
    /// The face's descent at this size, px (positive, below the baseline).
    pub descent: f32,
}

/// A ramp cut into cells (`Text::ramp`): face, characters, size (bits), cell width and height.
type RampKey = (u8, String, u32, u32, u32);

/// The text system: the registered faces, shaping and line breaking (parley), the layout cache, and
/// the glyph atlas the renderer uploads.
pub struct Text {
    shape_ms: f64,
    fonts: FontContext,
    layouts: LayoutContext<()>,
    scaler: ScaleContext,
    /// Which ellipsis each face and device size ends a clamped text in (`Text::ellipsis`).
    dots: HashMap<(u8, u32), &'static str>,
    families: Vec<Family>,
    /// Device px per logical px: pixel faces snap their size in device px.
    scale: f32,
    slots: HashMap<(u64, u32, u32, u8), Option<Slot>>,
    /// Character ramps cut into cells (`ramp`): where each strip is in the atlas.
    ramps: HashMap<RampKey, Option<(u32, u32)>>,
    /// A glyph did not fit: the atlas is full (`Text::refill`).
    full: bool,
    /// Layout asks for the same paragraph at the same width many times; keyed by text, face, size bits,
    /// line bits, width bits.
    measured: HashMap<MeasureKey, (f32, f32)>,
    /// Shaped and broken paragraphs by text, face, size, line height, width and obstacles: this
    /// generation's and the last one's (a paragraph not used for a whole generation is dropped).
    cache: [HashMap<LayoutKey, Laid>; 2],
    /// Each text's pieces for `El::reveal_by` from its content box's corner, by the layout's key and
    /// how it is cut; kept in two generations as the layouts are.
    pieces: [HashMap<(MeasureKey, u8), Pieces>; 2],
    /// Pieces found (cache misses), for the work counters.
    reveals: u64,
    /// RG8: coverage, nearness-to-ink.
    pub pixels: Vec<u8>,
    /// Where the atlas changed since the renderer last uploaded it (x0, y0, x1, y1 texels; empty when
    /// x1 <= x0): only that box goes up.
    pub dirty: [u32; 4],
    pen: (u32, u32, u32),
    /// Paragraphs shaped and broken by parley so far (cache misses), for the work counters; with
    /// `log_shapes`, which texts they were (for tests).
    shaped: u64,
    /// Record each shaped text in `shape_log`.
    pub log_shapes: bool,
    /// The texts shaped while `log_shapes` was on.
    pub shape_log: Vec<String>,
    /// Declared faces that have not come and were asked for since `begin_pass`.
    asked: std::collections::BTreeSet<u8>,
}

impl Text {
    /// Register `faces`, numbered in order (`Face::system` takes no number).
    pub fn new(faces: Vec<Face>) -> Self {
        let system = cfg!(all(feature = "system-fonts", not(target_arch = "wasm32"))) && faces.iter().any(Face::is_system);
        let mut collection = Collection::new(CollectionOptions { shared: false, system_fonts: system });
        let families: Vec<Family> = faces.into_iter().filter(|f| !f.is_system()).map(|f| {
            if let Some((called, d)) = f.declared { return Family { name: String::new(), called, pixel: None, data: Arc::new(vec![]), blob: 0, fallback: false, pending: Some(d), broken: false }; }
            let data = Arc::new(f.data);
            let blob = Blob::new(data.clone());
            let id = blob.id();
            let reg = collection.register_fonts(blob, None);
            let name = reg.first().and_then(|(id, _)| collection.family_name(*id).map(str::to_owned)).unwrap_or_default();
            Family { called: name.clone(), name, pixel: f.pixel, data, blob: id, fallback: f.fallback, pending: None, broken: false }
        }).collect();
        Self {
            fonts: FontContext { collection, source_cache: Default::default() },
            layouts: LayoutContext::new(), scaler: ScaleContext::new(), dots: HashMap::default(), families, scale: 1.0,
            slots: HashMap::default(), ramps: HashMap::default(), full: false, measured: HashMap::default(), cache: [HashMap::default(), HashMap::default()], pieces: [HashMap::default(), HashMap::default()], reveals: 0, pixels: vec![0; (ATLAS * ATLAS * 2) as usize], dirty: WHOLE, pen: (1, 1, 0), shaped: 0, shape_ms: 0.0, log_shapes: false, shape_log: vec![], asked: Default::default(),
        }
    }

    /// How many faces are registered.
    pub fn faces(&self) -> usize { self.families.len() }
    /// Whether a glyph found the atlas full since the last `refill`.
    pub fn is_full(&self) -> bool { self.full }
    /// Start the glyph atlas over (every glyph is drawn into it again as it is next emitted): for a
    /// long-running page that has used many sizes (the film at several scales). Glyph instances made
    /// before point at the old atlas and must be made again.
    pub fn refill(&mut self) {
        self.slots.clear();
        self.ramps.clear();
        self.pixels.iter_mut().for_each(|p| *p = 0);
        self.pen = (1, 1, 0);
        self.full = false;
        self.dirty = WHOLE;
    }
    /// How many paragraphs have been shaped (parley layouts built) so far.
    pub fn shaped(&self) -> u64 { self.shaped }
    /// How many texts' pieces (`El::reveal_by`) have been found so far.
    pub fn reveals(&self) -> u64 { self.reveals }
    /// How much of the glyph atlas is used, 0..1 (by rows).
    pub fn atlas_use(&self) -> f32 { ((self.pen.1 + self.pen.2) as f32 / ATLAS as f32).min(1.0) }
    /// Whether `face` is a pixel face.
    pub fn is_pixel(&self, face: u8) -> bool { self.family(face).pixel.is_some() }
    /// Whether text in `face` at `size` px is a pixel face drawn at the size asked, under its native size
    /// (`FINE`).
    pub fn fine(&self, face: u8, size: f32) -> bool { face & FINE != 0 && self.family(face).pixel.is_some_and(|n| size * self.real(face).1 < n - 1e-4) }
    /// Native size of a pixel face, px.
    pub fn native(&self, face: u8) -> Option<f32> { self.family(face).pixel }
    fn family(&self, face: u8) -> &Family { &self.families[self.real(face).0] }
    /// The family text in `face` is really set in, and the size factor: a declared face that has not
    /// come is set in its stand-in.
    fn real(&self, face: u8) -> (usize, f32) {
        let fi = ((face & !FINE) as usize).min(self.families.len() - 1);
        match self.families[fi].pending { Some(d) => ((d.stand_in as usize).min(self.families.len() - 1), d.scale), None => (fi, 1.0) }
    }
    /// Note that text in `face` was asked for (`wanted`).
    fn note(&mut self, face: u8) {
        let fi = ((face & !FINE) as usize).min(self.families.len() - 1);
        if self.families[fi].pending.is_some() && !self.families[fi].broken { self.asked.insert(fi as u8); }
    }
    /// Declare a face whose bytes come later (`Ui::declare_face`): its number now, text in it set in the
    /// stand-in meanwhile. A name already registered or declared keeps its number.
    pub fn declare(&mut self, name: &str, decl: FaceDecl) -> u8 {
        if let Some(i) = self.face_named(name) { return i; }
        let stand_in = (decl.stand_in as usize).min(self.families.len().saturating_sub(1)) as u8;
        self.families.push(Family { name: String::new(), called: name.to_owned(), pixel: None, data: Arc::new(vec![]), blob: 0, fallback: false, pending: Some(FaceDecl { stand_in, scale: if decl.scale > 0.0 { decl.scale } else { 1.0 } }), broken: false });
        (self.families.len() - 1) as u8
    }
    /// The number of the face the host calls `name` (as declared or added; a bundled face by its family
    /// name).
    pub fn face_named(&self, name: &str) -> Option<u8> { self.families.iter().position(|f| f.called == name).map(|i| i as u8) }
    /// Whether `face` was declared and has not come (it is drawn in its stand-in).
    pub fn is_pending(&self, face: u8) -> bool { self.families.get((face & !FINE) as usize).is_some_and(|f| f.pending.is_some()) }
    /// What stands in for `face` while it has not come.
    pub fn stand_in(&self, face: u8) -> Option<FaceDecl> { self.families.get((face & !FINE) as usize).and_then(|f| f.pending) }
    /// The bytes of the face `name` came (`Ui::add_face`): a declared face is set in itself from now on,
    /// a new name is registered. Bytes that are not a font are refused: the face keeps its stand-in and
    /// is not asked for again. Every layout is made again.
    pub fn add(&mut self, name: &str, bytes: Vec<u8>) -> Result<u8, crate::error::FontError> {
        let at = self.face_named(name);
        if at.is_some_and(|i| self.families[i as usize].pending.is_none()) { return Ok(at.unwrap_or_default()); }
        if swash::FontRef::from_index(&bytes, 0).is_none() {
            if let Some(i) = at { self.families[i as usize].broken = true; self.asked.remove(&i); }
            return Err(crate::error::FontError::Bad { name: name.to_owned() });
        }
        let data = Arc::new(bytes);
        let blob = Blob::new(data.clone());
        let id = blob.id();
        let reg = self.fonts.collection.register_fonts(blob, None);
        let fam = reg.first().and_then(|(id, _)| self.fonts.collection.family_name(*id).map(str::to_owned)).unwrap_or_default();
        let f = Family { name: fam, called: name.to_owned(), pixel: None, data, blob: id, fallback: false, pending: None, broken: false };
        let i = match at { Some(i) => { self.families[i as usize] = f; i } None => { self.families.push(f); (self.families.len() - 1) as u8 } };
        self.asked.remove(&i);
        self.measured.clear();
        self.cache = [HashMap::default(), HashMap::default()];
        Ok(i)
    }
    /// Start a layout pass: what is asked for from now on is what the page uses.
    pub fn begin_pass(&mut self) { self.asked.clear(); }
    /// The declared faces that have not come, are not broken, and were asked for since the pass began,
    /// by the names they were declared with.
    pub fn wanted(&self) -> Vec<String> { self.asked.iter().map(|&i| self.families[i as usize].called.clone()).collect() }

    /// Device px per logical px (pixel faces snap in device px); a change empties the layout cache.
    pub fn set_scale(&mut self, scale: f32) {
        if scale != self.scale { self.scale = scale; self.measured.clear(); self.cache = [HashMap::default(), HashMap::default()]; self.pieces = [HashMap::default(), HashMap::default()]; }
    }

    /// The logical size a face is really drawn at: pixel faces go to the nearest whole multiple of their
    /// native size in device px, since anything between would smear their pixels, and never below what
    /// a screen of one device px per logical px draws (its nearest multiple, at least one, times the
    /// device pixel ratio, rounded down): 7 px of an 11 px face is 11 device px at 1 and 22 at 2.
    pub fn size_for(&self, face: u8, size: f32) -> f32 {
        if self.fine(face, size) { return size * self.real(face).1; }
        let size = size * self.real(face).1;
        match self.family(face).pixel {
            Some(native) => {
                let at_one = (size / native).round().max(1.0);
                let m = (size * self.scale / native).round().max((at_one * self.scale + 1e-4).floor()).max(1.0);
                (m * native) / self.scale
            }
            None => size,
        }
    }

    /// The size to set a face at so that it is drawn no larger than `size` would be: for a pixel face, the
    /// largest whole multiple (at least one) of its native size at or under `size`, in device px.
    pub fn size_down(&self, face: u8, size: f32) -> f32 {
        if self.fine(face, size) { return size; }
        let r = self.real(face).1;
        match self.family(face).pixel {
            Some(native) => ((size * r * self.scale / native + 1e-4).floor().max(1.0) * native) / (self.scale * r),
            None => size,
        }
    }

    /// The paragraph shaped and broken, from the cache when it was laid out before (same text, face,
    /// size, line height, width and obstacles); only a miss shapes (and counts in `shaped`).
    fn layout(&mut self, s: &str, face: u8, size: f32, line: f32, max_width: Option<f32>, set: Setting<'_>) -> Rc<Layout<()>> {
        let Setting { around, track } = set;
        self.note(face);
        let max_width = max_width.filter(|w| w.is_finite());
        let h = { use std::hash::{Hash, Hasher}; let mut h = crate::fasthash::FastHasher::default(); s.hash(&mut h); h.finish() };
        let k = (h, face, size.to_bits(), line.to_bits(), max_width.unwrap_or(-1.0).to_bits(), around.map_or(0, Around::id), track.to_bits());
        let of = |e: Option<&Laid>| e.filter(|e| &*e.0 == s).map(|e| e.1.clone());
        if let Some(l) = of(self.cache[0].get(&k)) { return l; }
        if let Some(e) = self.cache[1].remove(&k) && &*e.0 == s { let l = e.1.clone(); self.cache[0].insert(k, e); return l; }
        // Text that fits on its line without breaking is laid out the same at any width it fits in: the
        // unbounded layout serves (what layout measured is what is drawn).
        let free = (k.0, k.1, k.2, k.3, (-1.0f32).to_bits(), k.5, k.6);
        if let Some(w) = max_width && around.is_none() {
            let l = of(self.cache[0].get(&free)).or_else(|| of(self.cache[1].get(&free)));
            if let Some(l) = l && l.width() <= w && !s.contains('\n') {
                self.cache[0].insert(k, (s.into(), l.clone()));
                return l;
            }
        }
        if self.cache[0].len() >= LAYOUT_CACHE { self.cache.swap(0, 1); self.cache[0].clear(); }
        let l = Rc::new(self.shape(s, face, size, line, max_width, set));
        // One line that did not need breaking is also the unbounded layout (see above).
        if max_width.is_some() && around.is_none() && l.lines().count() <= 1 && !s.contains('\n') && of(self.cache[0].get(&free)).is_none() {
            self.cache[0].insert(free, (s.into(), l.clone()));
        }
        self.cache[0].insert(k, (s.into(), l.clone()));
        l
    }
    /// Shape and break `s` at `size` logical px. Measuring and drawing use this same layout, so the
    /// line breaks on screen are exactly the ones layout planned for.
    fn shape(&mut self, s: &str, face: u8, size: f32, line: f32, max_width: Option<f32>, set: Setting<'_>) -> Layout<()> {
        self.shaped += 1;
        let t0 = crate::perf::now_ms();
        let l = self.shape_inner(s, face, size, line, max_width, set);
        self.shape_ms += crate::perf::now_ms() - t0;
        l
    }
    /// Time spent shaping, ms, since the start (the readout takes differences).
    pub fn shape_ms(&self) -> f64 { self.shape_ms }
    /// Paragraph layouts held in the cache.
    pub fn cached(&self) -> usize { self.cache[0].len() + self.cache[1].len() }
    fn shape_inner(&mut self, s: &str, face: u8, size: f32, line: f32, max_width: Option<f32>, set: Setting<'_>) -> Layout<()> {
        let Setting { around, track } = set;
        if self.log_shapes { self.shape_log.push(s.to_owned()); }
        let asked = size * self.real(face).1;
        let size = self.size_for(face, size);
        // A pixel face asked for under its native size is drawn at it (or at what one device px a px
        // draws): its lines are that much further apart, so wrapped lines never overlap.
        let under = self.family(face).pixel.is_some_and(|native| asked > 0.0 && asked < native - 1e-4);
        let line = if under && size > asked + 1e-4 { line * size / asked } else { line };
        let fi = self.real(face).0;
        let track = track * size;
        let mut l = self.build(s, fi, size, line, track, &[]);
        match around {
            None => l.break_all_lines(max_width),
            Some(a) => {
                let hy = if a.hyphenate { self.hyphen_advance(fi, size) } else { 0.0 };
                let plan = plan_around(&mut l, s, a, max_width.unwrap_or(1e5), line, if a.min > 0.0 { a.min } else { crate::wrap::MIN_EM * size }, size, hy);
                // Justified pieces: the same text again, with the extra advance as letter spacing on the
                // clusters it follows, broken at exactly the planned places.
                let spans: Vec<(std::ops::Range<usize>, f32)> = plan.iter().flat_map(|p| p.after.iter().map(|r| (r.clone(), p.extra + track)).chain(p.letters.iter().map(|r| (r.clone(), p.track + track)))).collect();
                if !spans.is_empty() { l = self.build(s, fi, size, line, track, &spans); }
                set_planned(&mut l, s, &plan, max_width.unwrap_or(1e5));
            }
        }
        l
    }
    /// The advance of a hyphen in family `fi` at `size` logical px.
    fn hyphen_advance(&self, fi: usize, size: f32) -> f32 {
        let fam = &self.families[fi.min(self.families.len() - 1)];
        let Some(fref) = swash::FontRef::from_index(&fam.data, 0) else { return size * 0.33 };
        fref.glyph_metrics(&[]).scale(size).advance_width(fref.charmap().map('-'))
    }
    /// Whether line `ln` of `p` ends in a hyphenated break (it flows around obstacles with hyphens on).
    fn hyphenated(p: &Para<'_>, end: usize) -> bool { p.around.is_some_and(|a| a.hyphenate) && crate::wrap::hyphenated_at(p.s, end) }
    /// The paragraph shaped (not yet broken), with `track` letter spacing (logical px) throughout and
    /// other spacing on the given byte ranges.
    fn build(&mut self, s: &str, fi: usize, size: f32, line: f32, track: f32, spacing: &[(std::ops::Range<usize>, f32)]) -> Layout<()> {
        let mut b = self.layouts.ranged_builder(&mut self.fonts, s, 1.0, true);
        // The first registered face (the proportional one, with CJK) backs up any face that lacks a
        // character, so a monospaced face can still show Chinese.
        // Then the fallback faces, in order; with `Face::system`, parley falls back to the system's fonts
        // for the script after all of them.
        let mut stack = vec![FontFamilyName::named(&self.families[fi].name), FontFamilyName::named(&self.families[0].name)];
        stack.extend(self.families.iter().filter(|f| f.fallback).map(|f| FontFamilyName::named(&f.name)));
        b.push_default(StyleProperty::FontFamily(FontFamily::List(std::borrow::Cow::Owned(stack))));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(LineHeight::Absolute(line));
        if track != 0.0 { b.push_default(StyleProperty::LetterSpacing(track)); }
        for (r, v) in spacing { b.push(StyleProperty::LetterSpacing(*v), r.clone()); }
        b.build(s)
    }

    /// Width and height of a paragraph in logical px when limited to `max_width`.
    pub fn measure(&mut self, s: &str, face: u8, size: f32, line: f32, max_width: Option<f32>) -> (f32, f32) { self.measure_around(s, face, size, line, max_width, Setting::default()) }
    /// The same, flowing around obstacles (the width is the whole column's, the height reaches the last
    /// line) and tracked (`Setting`).
    pub fn measure_around(&mut self, s: &str, face: u8, size: f32, line: f32, max_width: Option<f32>, set: Setting<'_>) -> (f32, f32) {
        let Setting { around, track } = set;
        self.note(face);
        let k = (s.to_owned(), face, size.to_bits(), line.to_bits(), max_width.unwrap_or(-1.0).to_bits(), around.map_or(0, Around::id), track.to_bits());
        if let Some(v) = self.measured.get(&k) { return *v; }
        if self.measured.len() > 4096 { self.measured.clear(); }
        let l = self.layout(s, face, size, line, max_width, set);
        let v = match around {
            None => (l.width().ceil(), l.height().ceil()),
            Some(_) => (max_width.unwrap_or(l.width()).ceil(), l.lines().map(|ln| ln.metrics().block_max_coord).fold(0.0f32, f32::max).ceil()),
        };
        self.measured.insert(k, v);
        v
    }

    /// How many glyphs of a paragraph no face has (drawn as the empty box): 0 when its scripts are covered.
    pub fn missing_glyphs(&mut self, p: Para<'_>) -> usize {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        l.lines().flat_map(|ln| ln.items().collect::<Vec<_>>()).map(|item| match item {
            PositionedLayoutItem::GlyphRun(gr) => gr.positioned_glyphs().filter(|g| g.id == 0).count(),
            _ => 0,
        }).sum()
    }
    /// Where a caret at byte `index` of a paragraph goes: x, top and height, logical px. `origin` is
    /// where the paragraph is drawn.
    pub fn caret(&mut self, p: Para<'_>, origin: (f32, f32), index: usize) -> Option<crate::ui::Rect> {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        let at = index.min(p.s.len());
        let b = parley::Cursor::from_byte_index(&l, at, parley::Affinity::Downstream).geometry(&l, 1.0);
        let h = (b.y1 - b.y0) as f32;
        (h > 0.0).then(|| crate::ui::Rect::new(origin.0 + b.x0 as f32, origin.1 + b.y0 as f32, 1.0, h))
    }
    /// The byte index (on a cluster boundary) nearest to the point (x, y), logical px; `origin` is where
    /// the paragraph is drawn. A point beyond the text goes to the nearest line's end.
    pub fn hit(&mut self, p: Para<'_>, origin: (f32, f32), x: f32, y: f32) -> usize {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        parley::Cursor::from_point(&l, x - origin.0, y - origin.1).index().min(p.s.len())
    }
    /// The boxes (one per line, logical px) that bytes `a..b` cover: where a selection is.
    pub fn boxes(&mut self, p: Para<'_>, origin: (f32, f32), a: usize, b: usize) -> Vec<crate::ui::Rect> {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        let (a, b) = (a.min(p.s.len()), b.min(p.s.len()));
        if a >= b { return vec![]; }
        let sel = parley::Selection::new(parley::Cursor::from_byte_index(&l, a, parley::Affinity::Downstream), parley::Cursor::from_byte_index(&l, b, parley::Affinity::Upstream));
        sel.geometry(&l).into_iter().map(|(bb, _)| crate::ui::Rect::new(origin.0 + bb.x0 as f32, origin.1 + bb.y0 as f32, (bb.x1 - bb.x0) as f32, (bb.y1 - bb.y0) as f32)).collect()
    }
    /// Each laid-out line's box, logical px from `origin`: from the line's start to the end of its last
    /// visible character (trailing spaces hang and are left out), over the line's full height.
    pub fn line_boxes(&mut self, p: Para<'_>, origin: (f32, f32)) -> Vec<crate::ui::Rect> {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        let hy = self.hyphen_advance(self.real(p.face).0, self.size_for(p.face, p.size));
        l.lines().map(|ln| {
            let m = ln.metrics();
            // A hyphenated line ends with its hyphen.
            let h = if Self::hyphenated(&p, ln.text_range().end) { hy } else { 0.0 };
            crate::ui::Rect::new(origin.0 + m.offset + m.inline_min_coord, origin.1 + m.block_min_coord, (m.advance - m.hanging_advance).max(0.0) + h, m.block_max_coord - m.block_min_coord)
        }).collect()
    }
    /// The pieces of [`Text::reveal_groups`] from the content box's corner, found once for the same text,
    /// face, size, line height, width and setting (each paint of a page asks again).
    pub fn reveal_pieces(&mut self, p: Para<'_>, by: crate::el::Reveal) -> Pieces {
        let w = Some(p.width).filter(|w| w.is_finite()).unwrap_or(-1.0);
        let k = ((p.s.to_owned(), p.face, p.size.to_bits(), p.line.to_bits(), w.to_bits(), p.around.map_or(0, Around::id), p.track.to_bits()), by as u8);
        if let Some(g) = self.pieces[0].get(&k) { return g.clone(); }
        if let Some(g) = self.pieces[1].remove(&k) { self.pieces[0].insert(k, g.clone()); return g; }
        let g = Rc::new(self.reveal_groups(p, (0.0, 0.0), by));
        self.reveals += 1;
        if self.pieces[0].len() >= LAYOUT_CACHE { self.pieces.swap(0, 1); self.pieces[0].clear(); }
        self.pieces[0].insert(k, g.clone());
        g
    }
    /// The text a paragraph clamped to `lines` lines shows (`El::clamp`), or `None` when it fits: the
    /// longest start of it that, with the face's own ellipsis (U+2026, or three full stops where the
    /// face has none) after it, takes no more lines and whose last line is no wider than `p.width`. It
    /// breaks after a word, unless that leaves more than a quarter of the last line empty, then between
    /// grapheme clusters; no space, comma, full stop, colon, dash or opening bracket is left before the
    /// ellipsis.
    pub fn clamp_lines(&mut self, p: Para<'_>, lines: u16) -> Option<String> {
        let n = lines as usize;
        if n == 0 { return None; }
        let w = p.width;
        let full = self.layout(p.s, p.face, p.size, p.line, Some(w), p.setting());
        // Within its lines and its width: a word longer than the width is cut too.
        if full.len() <= n && full.lines().all(|ln| { let m = ln.metrics(); m.advance - m.hanging_advance <= w + 0.01 }) { return None; }
        let dots = self.ellipsis(p.face, p.size);
        // Whether `s[..end]` with the ellipsis fits, and how full its last line is.
        let mut fits = |t: &mut Self, end: usize| -> Option<f32> {
            let mut c = p.s[..end].trim_end().to_owned();
            while c.ends_with([',', '.', ':', ';', '\u{2014}', '\u{2013}', '-', '(', '[', '{', '\u{ff0c}', '\u{3002}', '\u{ff1a}', '\u{ff08}']) { c.pop(); c = c.trim_end().to_owned(); }
            c.push_str(dots);
            let l = t.layout(&c, p.face, p.size, p.line, Some(w), p.setting());
            let last = l.lines().last().map_or(0.0, |ln| { let m = ln.metrics(); m.advance - m.hanging_advance });
            (l.len() <= n && l.lines().all(|ln| { let m = ln.metrics(); m.advance - m.hanging_advance <= w + 0.01 })).then_some(last / w.max(1.0))
        };
        // The longest fitting end among `ends` (fitting is monotone in the length), by halving.
        let best = |t: &mut Self, ends: &[usize], fits: &mut dyn FnMut(&mut Self, usize) -> Option<f32>| -> Option<(usize, f32)> {
            let (mut lo, mut hi) = (0usize, ends.len());
            let mut got = None;
            while lo < hi {
                let mid = (lo + hi) / 2;
                match fits(t, ends[mid]) { Some(f) => { got = Some((ends[mid], f)); lo = mid + 1; } None => hi = mid }
            }
            got
        };
        let graphemes: Vec<usize> = p.s.grapheme_indices(true).map(|(i, _)| i).filter(|&i| i > 0).collect();
        let words: Vec<usize> = p.s.split_word_bound_indices().filter(|(_, x)| x.chars().all(char::is_whitespace)).map(|(i, _)| i).filter(|&i| i > 0).collect();
        let by_word = best(self, &words, &mut fits);
        let by_char = best(self, &graphemes, &mut fits);
        let end = match (by_word, by_char) {
            (Some((a, f)), Some((b, _))) if f < 0.75 && b > a => b,
            (Some((a, _)), _) => a,
            (None, Some((b, _))) => b,
            (None, None) => 0,
        };
        let mut c = p.s[..end].trim_end().to_owned();
        while c.ends_with([',', '.', ':', ';', '\u{2014}', '\u{2013}', '-', '(', '[', '{', '\u{ff0c}', '\u{3002}', '\u{ff1a}', '\u{ff08}']) { c.pop(); c = c.trim_end().to_owned(); }
        c.push_str(dots);
        Some(c)
    }
    /// The ellipsis a clamped text in `face` at `size` ends in: the face's own U+2026 when its ink sits
    /// on the baseline, else three full stops (a face without one, or whose one floats more than a
    /// device px above the baseline, as some pixel faces' do).
    pub fn ellipsis(&mut self, face: u8, size: f32) -> &'static str {
        let key = (face, (self.size_for(face, size) * self.scale * 64.0).round() as u32);
        if let Some(d) = self.dots.get(&key) { return d; }
        let d = self.ellipsis_now(face, size);
        self.dots.insert(key, d);
        d
    }
    fn ellipsis_now(&mut self, face: u8, size: f32) -> &'static str {
        if !self.has_char(face, '\u{2026}') { return "..."; }
        // The glyph as it is set (shaping may pick another than the character map's), rasterised as the
        // page draws it.
        let l = self.layout("\u{2026}", face, size, size * 1.5, None, Setting { around: None, track: 0.0 });
        let scale = self.scale;
        let mut above: Option<f32> = None;
        for ln in l.lines() { for item in ln.items() {
            let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
            let run = gr.run();
            let font = run.font();
            let Some(fref) = swash::FontRef::from_index(font.data.as_ref(), font.index as usize) else { continue };
            let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
            let mut scaler = self.scaler.builder(fref).size(run.font_size() * scale).hint(false).normalized_coords(coords.iter().copied()).build();
            for g in gr.glyphs() {
                let Some(img) = Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut scaler, g.id as u16) else { continue };
                let w = img.placement.width as usize;
                // The lowest row drawn more than half, in device px above the baseline.
                let Some(row) = (0..img.placement.height as usize).rev().find(|&y| img.data[y * w..(y + 1) * w].iter().any(|&c| c >= 128)) else { continue };
                let a = img.placement.top as f32 - (row as f32 + 1.0) - g.y * scale;
                above = Some(above.map_or(a, |b: f32| b.min(a)));
            }
        } }
        if above.is_none_or(|a| a > 1.0) { "..." } else { "\u{2026}" }
    }
    /// Whether face `face` has a glyph for `ch`.
    pub fn has_char(&self, face: u8, ch: char) -> bool {
        let f = self.family(face);
        swash::FontRef::from_index(f.data.as_ref(), 0).is_some_and(|r| r.charmap().map(ch) != 0)
    }
    /// Each laid-out line's ink (`Ui::line_inks`): its box as `line_boxes` gives it, the box round the
    /// pixels its glyphs cover more than half at rest (true outlines, out to whole device px), its
    /// baseline, and where its face's capitals and lowercase reach up to (cap height, x height), all
    /// logical px from `origin`. A line with no ink has its ink box empty at its start on the baseline.
    pub fn line_inks(&mut self, p: Para<'_>, origin: (f32, f32), scale: f32) -> Vec<[f32; 11]> {
        let boxes = self.line_boxes(p, origin);
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        let fine = self.fine(p.face, p.size);
        let para_pixel = self.is_pixel(p.face) && !fine;
        let (ox, oy) = if para_pixel { ((origin.0 * scale).round(), (origin.1 * scale).round()) } else { (origin.0 * scale, origin.1 * scale) };
        let mut out = vec![];
        for (li, ln) in l.lines().enumerate() {
            let m = ln.metrics();
            let base = origin.1 + m.baseline;
            let (mut cap, mut xh) = (base - 0.7 * p.size, base - 0.5 * p.size);
            let mut ink = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            let hyphen = Self::hyphenated(&p, ln.text_range().end);
            let items: Vec<_> = ln.items().collect();
            let last_run = items.iter().rposition(|i| matches!(i, PositionedLayoutItem::GlyphRun(_)));
            let mut first = true;
            for (ii, item) in items.into_iter().enumerate() {
                let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                let run = gr.run();
                let font = run.font();
                let Some(fref) = swash::FontRef::from_index(font.data.as_ref(), font.index as usize) else { continue };
                let pixel = !fine && self.families.iter().any(|f| f.pixel.is_some() && f.blob == font.data.id());
                let fsize = run.font_size() * scale;
                let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                if first {
                    // The face's own numbers, at the size the run is set in.
                    let fm = fref.metrics(&coords).scale(run.font_size());
                    if fm.cap_height > 0.0 { cap = base - fm.cap_height; }
                    if fm.x_height > 0.0 { xh = base - fm.x_height; }
                    first = false;
                }
                let mut scaler = self.scaler.builder(fref).size(fsize).hint(false).normalized_coords(coords.iter().copied()).build();
                let mut glyphs: Vec<parley::Glyph> = gr.positioned_glyphs().collect();
                if hyphen && Some(ii) == last_run && let Some(g) = glyphs.last().copied() {
                    glyphs.push(parley::Glyph { id: fref.charmap().map('-') as u32, x: g.x + g.advance, ..g });
                }
                for g in glyphs {
                    let (gx, gy) = (ox + g.x * scale, (oy + g.y * scale).round());
                    let gx = if pixel { gx.round() } else { gx };
                    let sub = if pixel { 0 } else { ((gx.fract() * 4.0) as u8).min(3) };
                    let Some(img) = Render::new(&[Source::Outline]).format(Format::Alpha).offset(Vector::new(sub as f32 / 4.0, 0.0)).render(&mut scaler, g.id as _) else { continue };
                    let (w, h) = (img.placement.width as usize, img.placement.height as usize);
                    for y in 0..h { for x in 0..w {
                        if img.data[y * w + x] < 128 { continue; }
                        let (px, py) = (gx.floor() + (img.placement.left + x as i32) as f32, gy - img.placement.top as f32 + y as f32);
                        ink = [ink[0].min(px), ink[1].min(py), ink[2].max(px + 1.0), ink[3].max(py + 1.0)];
                    } }
                }
            }
            let b = boxes.get(li).copied().unwrap_or_default();
            let ink = if ink[0] > ink[2] { [b.x, base, 0.0, 0.0] } else { [ink[0].floor() / scale, ink[1].floor() / scale, (ink[2].ceil() - ink[0].floor()) / scale, (ink[3].ceil() - ink[1].floor()) / scale] };
            out.push([b.x, b.y, b.w, b.h, ink[0], ink[1], ink[2], ink[3], base, cap, xh]);
        }
        out
    }
    /// The pieces text is shown or taken away by under `El::reveal_by` (logical px boxes from `origin`,
    /// one or more per piece): each line, or each word with the punctuation that follows it (each CJK
    /// ideograph is a word of its own); `Reveal::Glyph` has none.
    pub fn reveal_groups(&mut self, p: Para<'_>, origin: (f32, f32), by: crate::el::Reveal) -> Vec<Vec<crate::ui::Rect>> {
        match by {
            crate::el::Reveal::Glyph => vec![],
            crate::el::Reveal::Line => self.line_boxes(p, origin).into_iter().map(|r| vec![r]).collect(),
            crate::el::Reveal::Word => {
                let mut words: Vec<(usize, usize)> = vec![];
                for (i, w) in p.s.split_word_bound_indices() {
                    if w.chars().all(char::is_whitespace) { continue; }
                    match words.last_mut() {
                        // Punctuation goes with the word just before it.
                        Some(last) if !wordy(w) && last.1 == i => last.1 = i + w.len(),
                        _ => words.push((i, i + w.len())),
                    }
                }
                words.into_iter().map(|(a, b)| self.boxes(p, origin, a, b)).collect()
            }
        }
    }
    /// Where each laid-out line's text ends (byte index).
    pub fn line_ends(&mut self, p: Para<'_>) -> Vec<usize> {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        l.lines().map(|ln| ln.text_range().end).collect()
    }
    /// Where the caret goes from byte `at` by the layout: a line up or down (keeping its x), or the start
    /// or end of its visual line.
    pub fn nav(&mut self, p: Para<'_>, at: usize, to: Nav) -> usize {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        let s = parley::Selection::from_byte_index(&l, at.min(p.s.len()), parley::Affinity::Downstream);
        let s = match to { Nav::Up => s.previous_line(&l, false), Nav::Down => s.next_line(&l, false), Nav::LineStart => s.line_start(&l, false), Nav::LineEnd => s.line_end(&l, false) };
        s.focus().index().min(p.s.len())
    }

    /// Every face's bytes by its number (a face declared and not come: its stand-in's), and whether it is a
    /// pixel face: for glyph outlines outside the text system (`pair::Faces`).
    pub(crate) fn face_bytes(&self) -> Vec<(Arc<Vec<u8>>, bool)> {
        (0..self.families.len()).map(|f| { let fam = &self.families[self.real(f as u8).0]; (fam.data.clone(), fam.pixel.is_some()) }).collect()
    }
    /// Rasterise one character of a face at `px` device px, outside any layout (for the mark atlas).
    /// Pixel faces are cut at half coverage as they are on the page.
    pub fn raster(&mut self, face: u8, ch: char, px: f32) -> Option<Raster> {
        let fam = self.family(face);
        let (data, pixel) = (fam.data.clone(), fam.pixel.is_some());
        let fref = swash::FontRef::from_index(&data, 0)?;
        let gid = fref.charmap().map(ch);
        let m = fref.metrics(&[]).scale(px);
        let advance = fref.glyph_metrics(&[]).scale(px).advance_width(gid);
        let mut scaler = self.scaler.builder(fref).size(px).hint(false).build();
        let img = Render::new(&[Source::Outline]).format(Format::Alpha).render(&mut scaler, gid)?;
        let mut data = img.data;
        if pixel { for c in &mut data { *c = if *c >= 128 { 255 } else { 0 }; } }
        Some(Raster { data, w: img.placement.width, h: img.placement.height, left: img.placement.left, top: img.placement.top, advance, ascent: m.ascent, descent: m.descent })
    }

    /// `chars` drawn at `px` (device px) into cells of `w` × `h` texels side by side, each centred in its
    /// cell on the face's line box, in one strip of the atlas (`el::Cells`): the texel where the strip's
    /// first cell starts. A part of a glyph outside its cell is cut off. Made once per face, characters,
    /// size and cell.
    pub fn ramp(&mut self, face: u8, chars: &[char], px: f32, w: u32, h: u32) -> Option<(u32, u32)> {
        let key = (face, chars.iter().collect::<String>(), px.to_bits(), w, h);
        if let Some(r) = self.ramps.get(&key) { return *r; }
        let n = chars.len() as u32;
        let mut strip = vec![0u8; (n * w * h) as usize];
        for (k, &ch) in chars.iter().enumerate() {
            let Some(g) = self.raster(face, ch, px) else { continue };
            let base = ((h as f32 - (g.ascent + g.descent)) * 0.5 + g.ascent).round() as i32;
            let ox = ((w as f32 - g.advance) * 0.5).round() as i32 + g.left;
            for y in 0..g.h as i32 { for x in 0..g.w as i32 {
                let (tx, ty) = (ox + x, base - g.top + y);
                if tx < 0 || ty < 0 || tx >= w as i32 || ty >= h as i32 { continue; }
                strip[(ty as u32 * n * w + k as u32 * w + tx as u32) as usize] = g.data[(y as u32 * g.w + x as u32) as usize];
            } }
        }
        let made = if n == 0 { None } else { store(&mut self.pixels, &mut self.pen, &mut self.dirty, &strip, n * w, h, 0, 0).map(|sl| (sl.x + SPREAD as u32, sl.y + SPREAD as u32)) };
        if made.is_none() && n > 0 { self.full = true; }
        self.ramps.insert(key, made);
        made
    }

    /// Call `out` once per visible glyph with its instance (device px) and glyph id. `origin` is
    /// logical px.
    pub fn emit(&mut self, p: Para<'_>, origin: (f32, f32), proto: Instance, scale: f32, out: impl FnMut(Instance, u32)) {
        self.emit_in(p, origin, proto, scale, None, out)
    }
    /// The same, leaving out lines entirely outside `rows` (device px, top and bottom): what a clip
    /// hides is not even made into instances.
    pub fn emit_in(&mut self, p: Para<'_>, origin: (f32, f32), proto: Instance, scale: f32, rows: Option<(f32, f32)>, mut out: impl FnMut(Instance, u32)) {
        self.emit_pens(p, origin, proto, scale, rows, |i, g, _| out(i, g))
    }
    /// The same, with where each glyph's pen is and its line (device px): `[x, baseline, line's start x,
    /// line's width, advance]` (a face arriving sweeps along its lines by them; a pose is told the pen).
    /// The pen's x is as laid out, before the glyph's image is put on a quarter pixel.
    pub fn emit_pens(&mut self, p: Para<'_>, origin: (f32, f32), proto: Instance, scale: f32, rows: Option<(f32, f32)>, mut out: impl FnMut(Instance, u32, [f32; 5])) {
        let l = self.layout(p.s, p.face, p.size, p.line, Some(p.width), p.setting());
        let fine = self.fine(p.face, p.size);
        let para_pixel = self.is_pixel(p.face) && !fine;
        // A pixel face starts on a whole device pixel so its pixels land on the screen's.
        let (ox, oy) = if para_pixel { ((origin.0 * scale).round(), (origin.1 * scale).round()) } else { (origin.0 * scale, origin.1 * scale) };
        for ln in l.lines() {
            if let Some((top, bottom)) = rows {
                let m = ln.metrics();
                let (y0, y1) = (oy + m.block_min_coord * scale, oy + m.block_max_coord * scale);
                // A glyph's ink can reach a little past its line box, and the spread pads it further.
                let pad = (SPREAD as f32 + p.size) * scale;
                if y1 + pad < top || y0 - pad > bottom { continue; }
            }
            let hyphen = Self::hyphenated(&p, ln.text_range().end);
            let lm = ln.metrics();
            let (lx, lw) = (ox + lm.offset * scale, (lm.advance * scale).max(1.0));
            let last_run = ln.items().enumerate().filter(|(_, i)| matches!(i, PositionedLayoutItem::GlyphRun(_))).last().map(|(k, _)| k);
            for (ii, item) in ln.items().enumerate() {
                let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                let run = gr.run();
                let font = run.font();
                let Some(fref) = swash::FontRef::from_index(font.data.as_ref(), font.index as usize) else { continue };
                // Pixel treatment follows the face a run is really set in: a fallback outline face inside
                // pixel text is drawn as an outline.
                let pixel = !fine && self.families.iter().any(|f| f.pixel.is_some() && f.blob == font.data.id());
                let fsize = run.font_size() * scale;
                let mut scaler = self.scaler.builder(fref).size(fsize).hint(false).normalized_coords(run.normalized_coords().iter().map(|c| c.to_bits())).build();
                // The hyphen of a hyphenated line: one more glyph after the run's last.
                let hy = if hyphen && Some(ii) == last_run { gr.positioned_glyphs().last().map(|g| parley::Glyph { id: fref.charmap().map('-') as u32, x: g.x + g.advance, ..g }) } else { None };
                for g in gr.positioned_glyphs().chain(hy) {
                    let (gx, gy) = (ox + g.x * scale, (oy + g.y * scale).round());
                    let gx = if pixel { gx.round() } else { gx };
                    let sub = if pixel { 0 } else { ((gx.fract() * 4.0) as u8).min(3) };
                    let key = (font.data.id(), g.id, fsize.to_bits(), sub);
                    let slot = match self.slots.get(&key) {
                        Some(s) => *s,
                        None => {
                            let mut img = Render::new(&[Source::Outline]).format(Format::Alpha).offset(Vector::new(sub as f32 / 4.0, 0.0)).render(&mut scaler, g.id as _);
                            if pixel && let Some(i) = img.as_mut() { for c in &mut i.data { *c = if *c >= 128 { 255 } else { 0 }; } }
                            let made = img.and_then(|i| { let m = store(&mut self.pixels, &mut self.pen, &mut self.dirty, &i.data, i.placement.width, i.placement.height, i.placement.left, i.placement.top); if m.is_none() && i.placement.width > 0 && i.placement.height > 0 { self.full = true; } m });
                            self.slots.insert(key, made);
                            made
                        }
                    };
                    let Some(sl) = slot else { continue };
                    let a = ATLAS as f32;
                    let mut inst = proto;
                    inst.rect = [gx.floor() + sl.left as f32 - SPREAD as f32, gy - sl.top as f32 - SPREAD as f32, sl.w as f32, sl.h as f32];
                    inst.uv = [sl.x as f32 / a, sl.y as f32 / a, (sl.x + sl.w) as f32 / a, (sl.y + sl.h) as f32 / a];
                    inst.paint[3] = GLYPH;
                    inst.extra[1] = p.print;
                    out(inst, g.id, [gx, gy, lx, lw, g.advance * scale]);
                }
            }
        }
    }
}

/// Where `Text::nav` moves the caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    /// To the line above, at the same x.
    Up,
    /// To the line below, at the same x.
    Down,
    /// To the start of the line.
    LineStart,
    /// To the end of the line.
    LineEnd,
}

/// The grapheme boundary before byte `at` (0 at the start).
pub fn prev_grapheme(s: &str, at: usize) -> usize { s.grapheme_indices(true).rev().map(|(i, _)| i).find(|&i| i < at).unwrap_or(0) }
/// The grapheme boundary after byte `at` (the end at the end).
pub fn next_grapheme(s: &str, at: usize) -> usize { s.grapheme_indices(true).map(|(i, g)| i + g.len()).find(|&e| e > at).unwrap_or(s.len()) }
/// Is this piece a word (letters, digits, or an ideograph) rather than spaces or punctuation?
fn wordy(w: &str) -> bool { w.chars().any(char::is_alphanumeric) }
/// The start of the word before byte `at` (skipping spaces and punctuation first). Each CJK ideograph
/// is a word of its own (UAX #29).
pub fn prev_word(s: &str, at: usize) -> usize {
    s.split_word_bound_indices().rev().find(|(i, w)| *i < at && wordy(w)).map_or(0, |(i, _)| i)
}
/// The end of the word after byte `at`.
pub fn next_word(s: &str, at: usize) -> usize {
    s.split_word_bound_indices().map(|(i, w)| (i + w.len(), w)).find(|(e, w)| *e > at && wordy(w)).map_or(s.len(), |(e, _)| e)
}
/// The word (or run of spaces, or punctuation) that byte `at` is in: what a double click selects.
pub fn word_range(s: &str, at: usize) -> (usize, usize) {
    let at = at.min(s.len());
    let mut last = (s.len(), s.len());
    for (i, w) in s.split_word_bound_indices() {
        if at >= i && at < i + w.len() { return (i, i + w.len()); }
        last = (i, i + w.len());
    }
    last
}

/// One line as planned around obstacles: where it starts, its band, its text, and the extra advance
/// (negative: tightening) put after each cluster in `after` to justify it.
/// `track` is put after each cluster in `letters` (Latin letters inside words) when the spaces alone
/// would need more than their cap.
struct Planned { x: f32, y: f32, range: std::ops::Range<usize>, extra: f32, after: Vec<std::ops::Range<usize>>, track: f32, letters: Vec<std::ops::Range<usize>> }

/// Is this character set without spaces between words (Han, kana, Hangul, CJK punctuation, fullwidth
/// forms)? Justification puts its extra advance after each of them.
fn cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

/// The clusters of `s[range]` that justification puts extra advance after: every cluster before the
/// last visible one that is CJK or a space; when there is none and `letters` is on, every one.
fn opportunities(s: &str, range: std::ops::Range<usize>, letters: bool) -> Vec<std::ops::Range<usize>> {
    let text = &s[range.clone()];
    let vis = text.trim_end_matches(char::is_whitespace).len();
    let gs: Vec<(usize, &str)> = text[..vis].grapheme_indices(true).collect();
    let Some((_, inner)) = gs.split_last() else { return vec![] };
    let at = |&(i, g): &(usize, &str)| range.start + i..range.start + i + g.len();
    let picked: Vec<_> = inner.iter().filter(|(_, g)| g.chars().next().is_some_and(|c| cjk(c) || c == ' ' || c == '\u{a0}')).map(at).collect();
    if picked.is_empty() && letters { inner.iter().map(at).collect() } else { picked }
}

/// The Latin letters of `s[range]` followed by another letter of the same word: where tracking goes.
fn letter_gaps(s: &str, range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
    let text = &s[range.clone()];
    let cs: Vec<(usize, char)> = text.char_indices().collect();
    cs.windows(2).filter(|w| w[0].1.is_ascii_alphabetic() && w[1].1.is_ascii_alphabetic()).map(|w| range.start + w[0].0..range.start + w[0].0 + w[0].1.len_utf8()).collect()
}
/// How `slack` px are shared: up to `cap` after each of `after`, the rest after each of `letters` up to
/// `track` (Latin pieces only: none when `after` already has letters). `None` when that is not enough.
fn share(slack: f32, after: usize, letters: usize, cap: f32, track: f32) -> Option<(f32, f32)> {
    if after == 0 { return (letters > 0 && track > 0.0 && slack / letters as f32 <= track).then(|| (0.0, slack / letters as f32)); }
    let e = slack / after as f32;
    if e <= cap { return Some((e, 0.0)); }
    let rest = slack - cap * after as f32;
    (letters > 0 && track > 0.0 && rest / letters as f32 <= track).then(|| (cap, rest / letters as f32))
}

/// One pass of breaking `l` around the obstacles in `a`: down the column a line height at a time, each
/// band's free pieces (left to right) take a line each, starting at the piece's start; a band with no
/// free piece is skipped a few px at a time. `more` lists lines (by index) given extra room so that they
/// take one cluster more than fits. Returns each line's piece `(x0, x1)` and band top.
/// `split` lists lines (by index) that end after exactly so many characters (a hyphenated break).
fn flow_pass(l: &mut Layout<()>, a: &Around, width: f32, line: f32, min: f32, more: &[(usize, f32)], split: &[(usize, u32)]) -> Vec<(f32, f32, f32)> {
    let mut br = l.break_lines();
    br.state_mut().set_layout_max_advance(width);
    let mut y = 0.0f32;
    let mut pieces: std::collections::VecDeque<(f32, f32)> = Default::default();
    let mut out = vec![];
    let mut guard = 0;
    while !br.is_done() && guard < 100_000 {
        guard += 1;
        if pieces.is_empty() {
            // The band as the line box will be: parley puts a line's top on a whole px.
            pieces = a.free(width, y.round(), (y + line).round(), min).into();
            if pieces.is_empty() { y += 4.0; continue; }
        }
        let (x0, x1) = pieces.pop_front().expect("a free piece");
        let w = (x1 - x0).min(width);
        let room = more.iter().find(|m| m.0 == out.len()).map_or(0.0, |m| m.1);
        let before = br.state().clone();
        let st = br.state_mut();
        st.set_line_x(x0);
        st.set_line_max_advance(w + room);
        st.set_line_y(y as f64);
        if let Some(&(_, n)) = split.iter().find(|s| s.0 == out.len()) {
            if br.break_next_with_length(n).is_none() { break; }
            out.push((x0, x0 + w, y));
            if pieces.is_empty() { y += line; }
            continue;
        }
        let Some(done) = br.break_next() else { break };
        // A word too long for a piece with an obstacle after it would run into the obstacle: leave the
        // piece empty.
        if room == 0.0 && !pieces.is_empty() && let parley::YieldData::LineBreak(d) = done && d.metrics.advance - d.metrics.hanging_advance > w + 0.25 {
            br.revert_to(before);
            continue;
        }
        out.push((x0, x0 + w, y));
        if pieces.is_empty() { y += line; }
    }
    br.finish();
    out
}

/// Break `l` around the obstacles in `a` and plan each line: a piece with an obstacle on either side
/// (and not the paragraph's last line) is justified to fill it, within `Around::justify_max` per gap;
/// one that would need more is tried with one cluster more (tightened by at most `wrap::SHRINK_EM` per
/// gap, `wrap::SHRINK_SPACE_EM` when every gap is a space), else left start-aligned.
/// `hy` is the hyphen's advance, for a piece that ends inside a word (`Around::hyphenate`).
#[allow(clippy::too_many_arguments)]
fn plan_around(l: &mut Layout<()>, s: &str, a: &Around, width: f32, line: f32, min: f32, size: f32, hy: f32) -> Vec<Planned> {
    /// The most lines per paragraph tried with one cluster more (each try breaks the paragraph again).
    const TRIES: usize = 16;
    let cap = a.justify_max * size;
    // Spaces give more than the gaps between CJK glyphs, which are only their side bearings.
    let shrink = |after: &[std::ops::Range<usize>]| {
        let spaces = after.iter().all(|r| s[r.clone()].starts_with([' ', '\u{a0}']));
        (if spaces { crate::wrap::SHRINK_SPACE_EM } else { crate::wrap::SHRINK_EM }).min(a.justify_max) * size
    };
    // Each line: piece, band top, text, ink advance, whether it may be justified.
    let lines = |l: &Layout<()>, placed: &[(f32, f32, f32)]| -> Vec<(f32, f32, f32, std::ops::Range<usize>, f32, bool)> {
        l.lines().zip(placed).map(|(ln, &(x0, x1, y))| {
            let m = ln.metrics();
            let last = matches!(ln.break_reason(), parley::BreakReason::None | parley::BreakReason::Explicit);
            let bounded = x0 > 0.5 || x1 < width - 0.5;
            (x0, x1, y, ln.text_range(), m.advance - m.hanging_advance, bounded && !last)
        }).collect()
    };
    let mut more: Vec<(usize, f32)> = vec![];
    let mut split: Vec<(usize, u32)> = vec![];
    let placed = flow_pass(l, a, width, line, min, &more, &split);
    let mut cur = lines(l, &placed);
    let mut plan = Vec::with_capacity(cur.len());
    let mut i = 0;
    while i < cur.len() {
        let (x0, x1, y, range, used, may) = cur[i].clone();
        let mut p = Planned { x: x0, y, range: range.clone(), extra: 0.0, after: vec![], track: 0.0, letters: vec![] };
        let track = if a.justify_latin_letters { 0.0 } else { a.track_max * size };
        if may {
            let after = opportunities(s, range.clone(), a.justify_latin_letters);
            let slack = x1 - x0 - used;
            let gaps = letter_gaps(s, range.clone());
            if (!after.is_empty() || (!gaps.is_empty() && track > 0.0)) && slack > 0.01 {
                let extra = if after.is_empty() { f32::INFINITY } else { slack / after.len() as f32 };
                if extra <= cap { p.extra = extra; p.after = after; }
                else if let Some((e, t)) = share(slack, after.len(), gaps.len(), cap, track) { (p.extra, p.after, p.track, p.letters) = (e, after, t, gaps); }
                else if more.len() < TRIES && !after.is_empty() {
                    // One cluster more, tightened to fit, if that stays within the shrink allowed.
                    let mut trial = more.clone();
                    trial.push((i, shrink(&after) * (after.len() + 1) as f32));
                    let tp = flow_pass(l, a, width, line, min, &trial, &split);
                    let t = lines(l, &tp);
                    if let Some((tx0, tx1, ty, tr, tu, _)) = t.get(i).cloned() && tu > tx1 - tx0 + 0.01 && tx0 == x0 && ty == y {
                        let ta = opportunities(s, tr.clone(), a.justify_latin_letters);
                        let e = (tx1 - tx0 - tu) / ta.len().max(1) as f32;
                        if !ta.is_empty() && -e <= shrink(&ta) {
                            more = trial;
                            cur = t;
                            p = Planned { x: tx0, y: ty, range: tr, extra: e, after: ta, track: 0.0, letters: vec![] };
                        }
                    }
                }
                // Still start-aligned: break the next word, at the longest hyphenation point whose part
                // (and the hyphen) justifies the piece within the cap.
                if p.after.is_empty() && p.letters.is_empty() && a.hyphenate && split.len() < TRIES {
                    let start = range.end;
                    let word_len = s[start..].bytes().take_while(u8::is_ascii_alphabetic).count();
                    let word = &s[start..start + word_len];
                    for b in crate::wrap::hyphen_points(word).into_iter().rev() {
                        let mut trial = split.clone();
                        trial.push((i, s[range.start..start + b].chars().count() as u32));
                        let tp = flow_pass(l, a, width, line, min, &more, &trial);
                        let tl = lines(l, &tp);
                        let Some((tx0, tx1, ty, tr, tu, _)) = tl.get(i).cloned() else { continue };
                        if tr.end != start + b || tx0 != x0 || ty != y { continue; }
                        let ta = opportunities(s, tr.clone(), a.justify_latin_letters);
                        if ta.is_empty() { continue; }
                        let slack = tx1 - tx0 - tu - hy;
                        let e = slack / ta.len() as f32;
                        let fit = if e < 0.0 { (-e <= shrink(&ta)).then_some((e, 0.0)) } else { share(slack, ta.len(), letter_gaps(s, tr.clone()).len(), cap, track) };
                        if let Some((e, tk)) = fit {
                            split = trial;
                            cur = tl;
                            let letters = if tk > 0.0 { letter_gaps(s, tr.clone()) } else { vec![] };
                            p = Planned { x: tx0, y: ty, range: tr, extra: e, after: ta, track: tk, letters };
                            break;
                        }
                    }
                }
            }
        }
        plan.push(p);
        i += 1;
    }
    plan
}

/// Break `l` again exactly as planned: each line takes the planned text from the planned place.
fn set_planned(l: &mut Layout<()>, s: &str, plan: &[Planned], width: f32) {
    let mut br = l.break_lines();
    br.state_mut().set_layout_max_advance(width);
    for p in plan {
        let st = br.state_mut();
        st.set_line_x(p.x);
        st.set_line_y(p.y as f64);
        if br.break_next_with_length(s[p.range.clone()].chars().count() as u32).is_none() { break; }
    }
    br.finish();
}

/// Put one glyph image, padded by the spread, into the atlas. `None` when it is empty or the atlas is
/// full (the glyph is then left out rather than drawn wrong). `dirty` grows by the box written.
#[allow(clippy::too_many_arguments)]
fn store(pixels: &mut [u8], pen: &mut (u32, u32, u32), dirty: &mut [u32; 4], data: &[u8], gw: u32, gh: u32, left: i32, top: i32) -> Option<Slot> {
    if gw == 0 || gh == 0 { return None; }
    let sp = SPREAD as u32;
    let (w, h) = (gw + sp * 2, gh + sp * 2);
    if pen.0 + w + 1 > ATLAS { *pen = (1, pen.1 + pen.2 + 1, 0); }
    if pen.1 + h + 1 > ATLAS { return None; }
    let (ax, ay) = (pen.0, pen.1);
    pen.0 += w + 1; pen.2 = pen.2.max(h);
    let mut cov = vec![0u8; (w * h) as usize];
    for y in 0..gh { for x in 0..gw { cov[((y + sp) * w + x + sp) as usize] = data[(y * gw + x) as usize]; } }
    let near = nearness(&cov, w as usize, h as usize);
    for y in 0..h { for x in 0..w {
        let i = (y * w + x) as usize; let o = (((ay + y) * ATLAS + ax + x) * 2) as usize;
        pixels[o] = cov[i]; pixels[o + 1] = near[i];
    } }
    *dirty = if dirty[2] <= dirty[0] { [ax, ay, ax + w, ay + h] } else { [dirty[0].min(ax), dirty[1].min(ay), dirty[2].max(ax + w), dirty[3].max(ay + h)] };
    Some(Slot { x: ax, y: ay, w, h, left, top })
}

/// Signed nearness to the glyph's outline: 128 on the outline, rising to 255 at `SPREAD` pixels inside
/// the ink and falling to 0 at `SPREAD` pixels outside (exact Euclidean distances, Felzenszwalb &
/// Huttenlocher, half a texel from the edge of the inked texels).
fn nearness(cov: &[u8], w: usize, h: usize) -> Vec<u8> {
    let outside = edt(cov, w, h, |c| c >= 128);
    let inside = edt(cov, w, h, |c| c < 128);
    cov.iter().enumerate().map(|(i, &c)| {
        let sd = if c >= 128 { inside[i].sqrt() - 0.5 } else { 0.5 - outside[i].sqrt() };
        ((0.5 + 0.5 * sd / SPREAD as f32).clamp(0.0, 1.0) * 255.0).round() as u8
    }).collect()
}

/// Squared distance from every texel to the nearest texel where `seed` holds.
fn edt(cov: &[u8], w: usize, h: usize, seed: impl Fn(u8) -> bool) -> Vec<f32> {
    const INF: f32 = 1e20;
    let mut d: Vec<f32> = cov.iter().map(|&c| if seed(c) { 0.0 } else { INF }).collect();
    let m = w.max(h);
    let (mut f, mut out, mut v, mut z) = (vec![0.0f32; m], vec![0.0f32; m], vec![0usize; m], vec![0.0f32; m + 1]);
    let pass = |f: &[f32], n: usize, out: &mut [f32], v: &mut [usize], z: &mut [f32]| {
        let mut k = 0usize; v[0] = 0; z[0] = -INF; z[1] = INF;
        for q in 1..n {
            loop {
                let p = v[k];
                let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
                if s <= z[k] && k > 0 { k -= 1; } else { k += 1; v[k] = q; z[k] = s; z[k + 1] = INF; break; }
            }
        }
        k = 0;
        for (q, o) in out.iter_mut().enumerate().take(n) {
            while z[k + 1] < q as f32 { k += 1; }
            let p = v[k]; *o = (q as f32 - p as f32).powi(2) + f[p];
        }
    };
    for x in 0..w {
        for y in 0..h { f[y] = d[y * w + x]; }
        pass(&f, h, &mut out, &mut v, &mut z);
        for y in 0..h { d[y * w + x] = out[y]; }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&d[y * w..y * w + w]);
        pass(&f, w, &mut out, &mut v, &mut z);
        d[y * w..y * w + w].copy_from_slice(&out[..w]);
    }
    d
}
