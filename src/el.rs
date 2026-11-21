//! Describe: the interface is a tree of plain values. A view is a function from state to this tree.
#![warn(missing_docs)]
use std::rc::Rc;
use crate::wrap::Around;
pub use taffy::Style;
use taffy::prelude::*;

/// How an instance is inked. `tone` runs from the ground through the theme's greys to the ink
/// (`Theme::grey`), `accent` mixes that toward one of the theme's accent colours: `hue` 0 is `accent`,
/// 1 `second`, 2 `third`, `Paint::EXTREME_HUE` the scheme's extreme (`Theme::extreme`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    /// From the ground (0) to the ink (1), through the theme's greys at their tones (`Paint::RAISED`,
    /// `HAIRLINE`, `DIM`, `TEXT`).
    pub tone: f32,
    /// From that toward the accent colour `hue` (0 none, 1 all).
    pub accent: f32,
    /// Opacity, 0–1.
    pub alpha: f32,
    /// Which accent: 0 `accent`, 1 `second`, 2 `third`, `EXTREME_HUE` the extreme; in a drawing, 3 and
    /// up its own colours (`Drawing::palette`: 3 is its first), and hue 0 anywhere else.
    pub hue: u8,
}
impl Paint {
    /// A mix of ground and ink, no accent.
    pub const fn tone(tone: f32) -> Self { Self { tone, accent: 0.0, alpha: 1.0, hue: 0 } }
    /// Full colour in the theme's accent number `hue` (0, 1, 2).
    pub const fn hue(hue: u8) -> Self { Self { tone: 1.0, accent: 1.0, alpha: 1.0, hue } }
    /// The same paint at opacity `alpha`.
    pub const fn alpha(self, alpha: f32) -> Self { Self { alpha, ..self } }
    /// Its hue as the shaders take it: a theme accent (0, 1, 2), the extreme (`EXTREME_CODE`); one of a
    /// drawing's own colours is hue 0 outside a drawing.
    pub(crate) fn theme_hue(self) -> f32 { if self.hue == Self::EXTREME_HUE { Self::EXTREME_CODE } else if self.hue > 2 { 0.0 } else { self.hue as f32 } }
    /// The hue that is the scheme's extreme: white where the ink is lighter than the ground, black
    /// otherwise (`Theme::extreme`).
    pub const EXTREME_HUE: u8 = 255;
    /// The extreme's hue code in an instance (after the theme's three and the drawings' own colours).
    pub(crate) const EXTREME_CODE: f32 = 3.0 + crate::draw::OWN_COLOURS as f32;
    /// Full ink: headings and what must stand out.
    pub const INK: Self = Self::tone(1.0);
    /// Running text and idle control labels: the theme's `text` grey.
    pub const TEXT: Self = Self::tone(219.0 / 255.0);
    /// Secondary text (captions, hints, labels, numbers): the theme's `dim` grey.
    pub const DIM: Self = Self::tone(168.0 / 255.0);
    /// Card frames, rules, dividers and control outlines at rest: the theme's `hairline` grey.
    pub const HAIRLINE: Self = Self::tone(51.0 / 255.0);
    /// Faint fills and rules: the hairline grey.
    pub const FAINT: Self = Self::HAIRLINE;
    /// Surfaces raised off the ground (a stage, a preview box): the theme's `raised` grey.
    pub const RAISED: Self = Self::tone(20.0 / 255.0);
    /// The page's own ground colour: invisible, but it keeps the ground units out.
    pub const GROUND: Self = Self::tone(0.0);
    /// The theme's first accent, in full.
    pub const ACCENT: Self = Self { tone: 1.0, accent: 1.0, alpha: 1.0, hue: 0 };
    /// The scheme's extreme in full: white on a dark scheme, black on a light one, past the ink.
    pub const EXTREME: Self = Self { tone: 1.0, accent: 1.0, alpha: 1.0, hue: Self::EXTREME_HUE };
}

/// What an element's fill and border turn to under the pointer, while held, and with focus
/// (`El::fill_hover`, `El::fill_held`, `El::border_hover`); each falls back to the paint at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaintOn {
    /// The fill under the pointer.
    pub hover: Option<Paint>,
    /// The fill pressed or dragged.
    pub held: Option<Paint>,
    /// The border under the pointer or with focus.
    pub border_hover: Option<Paint>,
}
impl PaintOn {
    /// The fill for a state (0 at rest, 1 under the pointer, 2 held), from `rest`.
    pub fn at(&self, rest: Option<Paint>, state: u8) -> Option<Paint> {
        let hover = if state >= 1 { self.hover.or(rest) } else { rest };
        if state >= 2 { self.held.or(hover) } else { hover }
    }
    /// The border, from `rest`, under the pointer or with focus (`lit`).
    pub fn border_at(&self, rest: Option<(f32, Paint)>, lit: bool) -> Option<(f32, Paint)> {
        rest.map(|(w, p)| (w, if lit { self.border_hover.unwrap_or(p) } else { p }))
    }
    /// It changes nothing.
    pub fn none(&self) -> bool { self.hover.is_none() && self.held.is_none() && self.border_hover.is_none() }
}

/// A key, as the runtime and handlers see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    /// A character key (with the layout's shift applied), Space included.
    Char(char),
    /// Enter or Return.
    Enter,
    /// Backspace.
    Backspace,
    /// Delete (forward).
    Delete,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// A function key, F1–F24.
    F(u8),
}

impl Key {
    /// From a DOM / winit style key name ("Enter", "ArrowUp", "a", " ").
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "Enter" => Key::Enter, "Backspace" => Key::Backspace, "Delete" => Key::Delete, "Escape" => Key::Escape, "Tab" => Key::Tab,
            "Home" => Key::Home, "End" => Key::End,
            "ArrowUp" => Key::Up, "ArrowDown" => Key::Down, "ArrowLeft" => Key::Left, "ArrowRight" => Key::Right,
            "PageUp" => Key::PageUp, "PageDown" => Key::PageDown,
            _ if name.len() >= 2 && name.starts_with('F') && name[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => Key::F(name[1..].parse().unwrap_or(1)),
            _ => {
                let mut cs = name.chars();
                let c = cs.next()?;
                if cs.next().is_some() { return None; }
                Key::Char(c)
            }
        })
    }
}

/// Which face a text element is set in: a role that the nearest `fonts` ancestor (normally the
/// theme, at the root) resolves, or a face index directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `Whisper` is small tracked type in the bundled serif (`text::SERIF`, the same in every theme;
/// `El::whisper` sets it up).
pub enum Font {
    /// Headings (the theme's display face).
    Display,
    /// Running text (its body face).
    Body,
    /// Figures and code (its mono face).
    Mono,
    /// Small tracked type in the bundled serif.
    Whisper,
    /// A face by its number.
    Face(u8),
}
impl Font {
    /// The face's number, given what display, body and mono resolve to.
    pub fn resolve(self, roles: [u8; 3]) -> u8 {
        match self { Font::Display => roles[0], Font::Body => roles[1], Font::Mono => roles[2], Font::Whisper => crate::text::SERIF, Font::Face(f) => f }
    }
}

/// How `El::fit` fits a subtree into its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FitMode {
    /// Wholly inside the box: the smaller of the two scales.
    #[default]
    Contain,
    /// As wide as the box.
    Width,
    /// As tall as the box.
    Height,
    /// Laid out again at the box's width first, with its type at its own size and wrapping there; then,
    /// only if it still does not fit (too tall, or something in it that does not wrap is too wide),
    /// zoomed down as `Contain` does, to `Fit::min` at least. It never grows (`Fit::max` is not used).
    Reflow,
}
/// A subtree fitted into its container's box (`El::fit`): how, the least and the most it may be scaled
/// (by default it only shrinks, to a quarter at least), and where it goes in the box when it does not
/// fill it (fractions across and down: 0.5, 0.5 centres it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// How it fits.
    pub mode: FitMode,
    /// The least scale; smaller than that, it is drawn at this and clipped.
    pub min: f32,
    /// The most scale.
    pub max: f32,
    /// Where it goes in the box, across and down (0..1).
    pub align: (f32, f32),
}
/// A plate (`El::plate`, DESIGN.md §1.33): how long its ink lasts, s (0–60 in steps of 0.5; 0: for
/// good): a pixel is as drawn until four fifths of it have passed, then thins out, and is gone at the
/// end of it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Plate {
    /// How long a stroke lasts after it was last drawn, s (0: it stays until `Cmd::ClearPlate`).
    pub fade_s: f32,
}
impl Default for Fit { fn default() -> Self { Self { mode: FitMode::Contain, min: 0.25, max: 1.0, align: (0.5, 0.5) } } }

/// What an element shows.
pub enum Content {
    /// Nothing of its own (a container; its fill and border still draw).
    None,
    /// `print` > 0 prints the glyphs through a lattice of that pitch (logical px): each cell is filled
    /// when the glyph covers more than half of it.
    /// `caret` is a byte index into `s`: a caret is drawn there while the nearest focusable ancestor
    /// has focus.
    /// `weight` moves the outline threshold out by that many logical px (bolder) or in (lighter), so a
    /// heading can be heavy without a bold face.
    /// `around` makes the paragraph flow around obstacles (see `wrap`).
    /// `select` is a byte range of `s` drawn as selected (by nearness to the glyphs, not as a box) while
    /// the field it belongs to has focus; `mark` a byte range underlined (text being composed by an input
    /// method). `nowrap` keeps it on one line whatever its width (a single-line field that scrolls). `track` is extra
    /// letter spacing in em (`El::tracking`).
    Text {
        /// The text.
        s: String,
        /// Font size, logical px.
        size: f32,
        /// Line height, logical px.
        line: f32,
        /// How it is inked.
        paint: Paint,
        /// Its face.
        font: Font,
        /// The pitch of the lattice it is printed through (0: none).
        print: f32,
        /// Where the caret is, a byte index.
        caret: Option<usize>,
        /// How much bolder (positive) or lighter, logical px.
        weight: f32,
        /// Obstacles it flows around.
        around: Option<Rc<Around>>,
        /// The selected byte range.
        select: Option<(usize, usize)>,
        /// The byte range underlined while an input method composes.
        mark: Option<(usize, usize)>,
        /// Kept on one line.
        nowrap: bool,
        /// Where each glyph is drawn instead of where the layout put it (`El::pose`).
        pose: Option<GlyphPose>,
        /// Extra letter spacing, em.
        track: f32,
    },
    /// A picture by id, with its height as a fraction of its width, and the `Markr` slot it uses.
    Picture {
        /// The picture's id (`Renderer::set_picture`).
        id: u32,
        /// Height over width.
        ratio: f32,
        /// The `Markr` slot it uses.
        fx: u8,
    },
    /// Meshes, a camera and a light, drawn into the element's box (DESIGN.md §1.18).
    Scene3d(Rc<crate::mesh::Scene3d>),
    /// Marks in their own space, scaled to the element's width (`el::canvas`, DESIGN.md §1.26).
    Canvas(Rc<crate::draw::Drawing>),
    /// Character marks over a picture at a pitch of its own (`el::cells`, DESIGN.md §1.6.1).
    Cells(Rc<Cells>),
    /// What a cached subtree drew, read back as coverage (`el::cached_view`, DESIGN.md §1.36).
    CachedView {
        /// The key of the element marked `El::cached`.
        over: String,
        /// The part of the page it shows, logical px: x, y, width, height.
        from: [f32; 4],
        /// The ink its coverage is drawn in.
        ink: Paint,
    },
}

/// The picture beneath an element as character marks (`el::cells`, DESIGN.md §1.6.1): the element's box
/// cut into cells of `pitch` (logical px, width × height, each a whole number of device px), each cell one
/// character of `ramp` by the brightness of the picture under it, in the cell's own colour mixed `mix`
/// toward `ink`. Between the characters the picture shows as it is. The GPU reads the picture: the
/// element is one instance and one draw however many cells it has, and moving it lays nothing out.
#[derive(Clone, Debug, PartialEq)]
pub struct Cells {
    /// The key of the picture element it reads (`el::picture`), drawn before it.
    pub over: String,
    /// The cell, logical px: width, height.
    pub pitch: [f32; 2],
    /// The characters, least ink first, up to 63 of them. A cell takes character `n` of `N` when it is in
    /// the `n`th of `N` equal bands of brightness, dark to light, or light to dark with `light_dense`. Put
    /// a character twice to give it two bands; leave the blank out for a grid that is full everywhere.
    pub ramp: String,
    /// Their size, logical px, and their face. Each is drawn whole into its cell, centred.
    pub size: f32,
    /// Their face.
    pub font: Font,
    /// What the cells' colours are mixed toward, and how far (0: the picture's own colour, 1: the ink).
    pub ink: Paint,
    /// How far toward `ink`.
    pub mix: f32,
    /// Brighter cells take the later (denser) characters: for light marks over a dark picture.
    pub light_dense: bool,
}
impl Default for Cells {
    fn default() -> Self { Self { over: String::new(), pitch: [7.0, 14.0], ramp: ".:-=+*#".into(), size: 11.0, font: Font::Mono, ink: Paint::INK, mix: 0.62, light_dense: false } }
}

/// Where one laid-out glyph is, for a pose: its number in the text and how many there are, its centre
/// in the element's content box (logical px), that box's size, and the box (x0, y0, x1, y1) all the
/// glyphs' centres span (so a pose can map the text block onto a path or a sphere), and its pen as laid
/// out: where its origin is on the baseline and how far it advances, kerning and all (a pose that moves a
/// glyph by `target − pen` puts its pen at `target` exactly).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlyphAt {
    /// Its number in the text, from 0.
    pub i: usize,
    /// How many glyphs there are.
    pub n: usize,
    /// Its centre, x.
    pub x: f32,
    /// Its centre, y.
    pub y: f32,
    /// The content box's width.
    pub w: f32,
    /// The content box's height.
    pub h: f32,
    /// The box the glyphs' centres span: x0, y0, x1, y1.
    pub span: [f32; 4],
    /// Its pen's x as laid out (not put on a pixel), in the content box.
    pub pen: f32,
    /// Its baseline's y, as drawn (on a whole device pixel), in the content box.
    pub base: f32,
    /// Its advance, logical px.
    pub advance: f32,
}
/// How one glyph is drawn instead of where the layout put it: moved by (dx, dy) logical px, turned by
/// `angle` (radians, clockwise), scaled by `sx`, `sy` about its centre; its tone and alpha multiplied,
/// its accent raised to at least `accent`, drawn in accent `hue` (0, 1, 2; negative keeps the text's).
/// Alpha 0 leaves it out. The glyph keeps its true outline.
/// `z` is its depth (logical px, positive toward the viewer) and `facing` how it faces the viewer (1
/// straight on, 0 edge on, negative turned away): the glyphs of one element are drawn far to near, and
/// `El::form` thins and fades them by these (`form::FormFx`). The defaults change nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Moved right by, logical px.
    pub dx: f32,
    /// Moved down by, logical px.
    pub dy: f32,
    /// Turned by, radians, clockwise.
    pub angle: f32,
    /// Scaled across by.
    pub sx: f32,
    /// Scaled up and down by.
    pub sy: f32,
    /// Its tone multiplied by.
    pub tone: f32,
    /// Its alpha multiplied by (0 leaves it out).
    pub alpha: f32,
    /// Its accent raised to at least this.
    pub accent: f32,
    /// The accent hue it is drawn in (negative keeps the text's).
    pub hue: f32,
    /// Depth, logical px, positive toward the viewer.
    pub z: f32,
    /// How it faces the viewer: 1 straight on, 0 edge on, negative turned away.
    pub facing: f32,
}
impl Default for Pose { fn default() -> Self { Self { dx: 0.0, dy: 0.0, angle: 0.0, sx: 1.0, sy: 1.0, tone: 1.0, alpha: 1.0, accent: 0.0, hue: -1.0, z: 0.0, facing: 1.0 } } }
/// A text element's glyph poses (`El::pose`): asked once per glyph each time the scene is made, never
/// for layout, so moving glyphs costs no layout or shaping.
pub type GlyphPose = Rc<dyn Fn(GlyphAt) -> Pose>;
/// The pointer over or on an element (`El::on_point`): where (local px, or a canvas's own space) and
/// whether it is pressed; over posed text, the glyph drawn under it (its number in the text, `Ui::glyph_at`)
/// and that glyph's depth (while dragging, `None`: ask `Ui::glyph_at`, or `form::Form::hit` for the surface).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Where, x (local logical px, or the canvas's space).
    pub x: f32,
    /// Where, y.
    pub y: f32,
    /// The pointer is pressed.
    pub down: bool,
    /// Over posed text: the glyph drawn under it.
    pub glyph: Option<usize>,
    /// That glyph's depth.
    pub z: f32,
    /// When it happened, s on the runtime's clock: the event's own time when the host gives it
    /// (`Ui::event_time`), so the moves a frame brings together are each at their own time.
    pub t: f32,
}

/// How a clip, a pose's alpha or the edge of the window takes text away (`El::reveal_by`): glyph by
/// glyph (the default), or whole words or whole lines, so nothing is left as half a word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Reveal {
    /// Glyph by glyph.
    #[default]
    Glyph,
    /// Whole words.
    Word,
    /// Whole lines.
    Line,
}
/// An element coming into view (`true`, any part inside its clip) or leaving it.
pub type OnVisible<M> = Rc<dyn Fn(bool) -> Option<M>>;
/// A text's pens as the shaper set them (`El::on_pens`).
pub type OnPens<M> = Rc<dyn Fn(Rc<Vec<f32>>) -> Option<M>>;
/// Asks for the true outline of an element's subtree as coverage on a grid (`El::on_outline`).
pub struct Outline<M> {
    /// Grid columns.
    pub cols: usize,
    /// Grid rows.
    pub rows: usize,
    /// Called with the coverage when it changes.
    pub f: Rc<dyn Fn(Rc<crate::field::Grid>) -> Option<M>>,
}

/// Handles the pointer over a `scene3d` element: which copy it is on, or pressed (DESIGN.md §1.18).
pub type OnPick<M> = Rc<dyn Fn(crate::mesh::Pick) -> Option<M>>;
/// Handles the wheel over an element before any scroll node does (dx, dy, logical px).
pub type OnWheel<M> = Rc<dyn Fn(f32, f32) -> Option<M>>;
/// The pointer over an element (`Some`: local logical px, and the glyph under it) or leaving it (`None`).
pub type OnMove<M> = Rc<dyn Fn(Option<Point>) -> Option<M>>;
/// Handles a key while the element has focus; the flag is Shift.
pub type OnKey<M> = Rc<dyn Fn(Key, bool) -> Option<M>>;
/// Handles text input while the element has focus (a text field, DESIGN.md §1.8.1).
pub type OnText<M> = Rc<dyn Fn(TextInput) -> Option<M>>;

/// Modifier keys with a key press: Shift, the command key (Ctrl, or ⌘ on macOS), Alt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    /// Shift.
    pub shift: bool,
    /// Ctrl, or ⌘ on macOS.
    pub cmd: bool,
    /// Alt (Option).
    pub alt: bool,
}

/// What a text field is told. Byte indices are into the text the field showed (its text leaf's string,
/// which includes text being composed).
#[derive(Clone, Debug, PartialEq)]
pub enum TextInput {
    /// A key that is not text: arrows, Home / End, Backspace, Delete, Enter; with Cmd: A, Z, Y.
    Key {
        /// The key.
        key: Key,
        /// Its modifiers.
        mods: Mods,
    },
    /// Text typed, committed by an input method, or dropped in: inserted at the caret over the selection.
    Insert(String),
    /// Text being composed by an input method, shown at the caret and underlined; `cursor` is a byte
    /// range in it. Empty text ends the composition without inserting anything (cancelled).
    Compose {
        /// The composition so far.
        text: String,
        /// The input method's cursor in it, a byte range.
        cursor: Option<(usize, usize)>,
    },
    /// The clipboard's text, to insert.
    Paste(String),
    /// The selection was copied to the clipboard by the runtime and is to be removed.
    Cut,
    /// The pointer: pressed (with the click count: 2 selects a word, 3 everything), dragged, let go, at a
    /// byte index found from the laid-out text; Shift extends the selection.
    Pointer {
        /// The byte index under the pointer.
        at: usize,
        /// Pressed, dragged or let go.
        phase: Phase,
        /// The click count.
        clicks: u8,
        /// Shift is held.
        shift: bool,
    },
    /// Put the caret at `at` (the runtime moved it by the layout: a line up or down, a line's start or
    /// end in a multi-line field), extending the selection if asked.
    Goto {
        /// The byte index.
        at: usize,
        /// Extend the selection to it.
        extend: bool,
    },
}
/// Handles a press and the drag that follows. The handler is captured when the press starts and the
/// pointer stays captured until release, wherever it goes.
pub type OnDrag<M> = Rc<dyn Fn(Drag) -> Option<M>>;

/// Where a drag or a pointer gesture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The press.
    Start,
    /// A move while pressed.
    Move,
    /// The release.
    End,
}

/// One step of a drag, logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    /// Press, move or release.
    pub phase: Phase,
    /// The pointer from the element's left edge.
    pub x: f32,
    /// The pointer from the element's top edge.
    pub y: f32,
    /// The element's width as it is now.
    pub w: f32,
    /// Its height as it is now.
    pub h: f32,
    /// How far the pointer has moved across since the press.
    pub dx: f32,
    /// How far it has moved down since the press.
    pub dy: f32,
    /// When it happened, s on the runtime's clock (as `Point::t`).
    pub t: f32,
}
impl Drag {
    /// The pointer as a fraction of the element, clamped to 0..1.
    pub fn fx(&self) -> f32 { (self.x / self.w.max(1.0)).clamp(0.0, 1.0) }
    /// The same, down.
    pub fn fy(&self) -> f32 { (self.y / self.h.max(1.0)).clamp(0.0, 1.0) }
}

/// How an element answers the field's fronts, lens and pulls (DESIGN.md §1.15). Inherited down the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Response {
    /// Fronts pass it by: no push, no looseness (the caret, a selection, the focus marks).
    Ignore,
    /// Pushed a little and offset by a burst as a front passes, but its looseness stays 0 (text to read,
    /// controls).
    Nudge,
    /// Pushed and loosened (decoration, pictures, a scene's headline).
    Loosen,
}
impl Response {
    /// Every response.
    pub const ALL: [Response; 3] = [Response::Ignore, Response::Nudge, Response::Loosen];
    /// Its name in presets.
    pub fn name(self) -> &'static str { match self { Response::Ignore => "ignore", Response::Nudge => "nudge", Response::Loosen => "loosen" } }
}

/// The trail a moving element leaves in the field map (DESIGN.md §1.29): `energy` times the look's
/// `MapFx::stirr`, over `radius` (logical px; `None`: the look's `stirr_radius` plus half the element's
/// shorter side), only on what `scope` reaches (the default: the nearest `ripple_scope`, else the window).
#[derive(Clone, Debug, PartialEq)]
pub struct Stirr {
    /// How much it stirs, times `MapFx::stirr`.
    pub energy: f32,
    /// How far it reaches, logical px.
    pub radius: Option<f32>,
    /// What it reaches.
    pub scope: Scope,
}
impl Default for Stirr { fn default() -> Self { Self { energy: 1.0, radius: None, scope: Scope::Container(None) } } }

/// What a front reaches (DESIGN.md §1.15).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Only the pressed element's own subtree (the default for presses on controls).
    Own,
    /// The nearest ancestor marked `El::ripple_scope()` (`None`), or the element with this key.
    Container(Option<String>),
    /// The whole window (theme, material, page and scene changes; an explicit global front).
    Global,
    /// No front at all.
    Off,
}

/// A material, a response and a scope for a subtree, put on in one line with `El::effect`. `None`
/// leaves that part as it is (inherited, or the look's).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effect {
    /// The material the subtree is drawn with.
    pub material: Option<crate::style::Material>,
    /// How it answers fronts, the lens and pulls.
    pub response: Option<Response>,
    /// What a press inside it sends a front to.
    pub scope: Option<Scope>,
}

/// Ripple and recomposition settings of an element (DESIGN.md §1.15, §1.16). `None` means inherited
/// (or the default for what the element is).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ripple {
    /// How it answers fronts.
    pub respond: Option<Response>,
    /// Text to read or a control: field effects loosen it at most `Look::legible_cap`.
    pub legible: Option<bool>,
    /// What a press here sends a front to.
    pub scope: Option<Scope>,
    /// Fronts from presses inside with `Scope::Container(None)` stop at this element's box.
    pub container: bool,
    /// Text whose string changes recomposes glyph by glyph (on by default for keyed text).
    pub recompose: Option<bool>,
    /// The focus marks do not frame this element when the pointer is over it (its box is not its
    /// shape: text runs around its true outline, and corners at its box would sit in the text).
    pub unframed: bool,
}

/// An element that opens a layer over everything else (a dialog). While it is there, only it can be
/// hit or focused, everything beneath it is held at `beneath` looseness, and Esc sends `dismiss`.
pub struct Modal<M> {
    /// Sent when Esc is pressed while the layer is open.
    pub dismiss: M,
    /// How loose everything beneath it is held.
    pub beneath: f32,
}

/// An element: a box with its layout, what it shows, how it answers input and the field, and its
/// children. Built with `row`, `col`, `text` and the other constructors and the builder methods below.
pub struct El<M> {
    /// What it is apart from what it sends (`Props`): an `El` reads and writes these as its own fields
    /// (`el.style`, `el.key`).
    pub props: Props,
    /// Sent on a press, Enter or Space (`El::on_press`).
    pub on_press: Option<M>,
    /// Keys while it has focus (`El::on_key`).
    pub on_key: Option<OnKey<M>>,
    /// A press and the drag that follows (`El::on_drag_with`).
    pub on_drag: Option<OnDrag<M>>,
    /// Sent when focus leaves this element.
    pub on_blur: Option<M>,
    /// Opens a layer over everything else (`El::modal`).
    pub modal: Option<Modal<M>>,
    /// A text field: takes text input while focused (see `TextInput`); `multiline` lets the runtime move
    /// the caret by lines and Enter insert a line break.
    pub on_text: Option<OnText<M>>,
    /// A virtual list (DESIGN.md §1.13): the runtime builds only the rows it can see (and a few around).
    pub virt: Option<Virtual<M>>,
    /// What this element is to assistive technology (DESIGN.md §1.14): role, name, state.
    pub access: Option<Access>,
    /// A `scene3d` element: what the pointer is over, and presses, by the copy under it.
    pub on_pick: Option<OnPick<M>>,
    /// The wheel over this element (taken before scrolling).
    pub on_wheel: Option<OnWheel<M>>,
    /// The pointer moving over this element, in its own coordinates, and leaving it (`El::on_move`).
    pub on_move: Option<OnMove<M>>,
    /// Told when it comes into view and leaves it (`El::on_visible`).
    pub on_visible: Option<OnVisible<M>>,
    /// Told its subtree's true outline as coverage on a grid, whenever it changes (`El::on_outline`).
    pub on_outline: Option<Outline<M>>,
    /// Told its glyphs' pens whenever they change (`El::on_pens`).
    pub on_pens: Option<OnPens<M>>,
    /// What it holds, in order.
    pub children: Vec<El<M>>,
}
/// What an element is apart from the messages it sends: its layout, what it shows and how, and how it
/// answers input. Every `El<M>` has one (`El::props`) and reads it as its own fields, so code that
/// changes an element's style or key is the same for every message type.
pub struct Props {
    /// Names the element across views: focus, scroll offsets, births, recomposition and the accessibility mirror follow it (`El::key`).
    pub key: String,
    /// Its layout (taffy's flexbox).
    pub style: Style,
    /// What it shows.
    pub content: Content,
    /// Its background.
    pub fill: Option<Paint>,
    /// What its fill and border turn to under the pointer, while held and with focus (`El::fill_hover`,
    /// `El::fill_held`, `El::border_hover`).
    pub fill_on: PaintOn,
    /// Its border: width (logical px) and paint.
    pub border: Option<(f32, Paint)>,
    /// Corner radius of the fill and border, logical px.
    pub radius: f32,
    /// Tab, presses and `Ui::focus_key` can give it focus.
    pub focusable: bool,
    /// Where Tab puts it: positive numbers first, lowest first; 0 (the default) in document order after
    /// them; negative never (it still takes focus from a press, a skip link or `Ui::focus_key`).
    pub tab_order: i32,
    /// Activating it moves focus to the element with this key, or the first one inside it that takes
    /// focus (a skip link).
    pub skip_to: Option<String>,
    /// Drawn only while it or something inside it has focus (it stays in the accessibility mirror).
    pub focus_shown: bool,
    /// Scrolls its children and clips them: vertically, or horizontally when `horizontal`.
    pub scroll: bool,
    /// With `scroll`: scrolls across rather than down.
    pub horizontal: bool,
    /// A scroll node that shows where it is with a thin bar.
    pub bar: bool,
    /// After a key changes the state, scroll so this element is visible (a selected row).
    pub reveal: bool,
    /// This subtree takes no input and is drawn faint.
    pub disabled: bool,
    /// Pressing this commits a new state (theme, material): the renderer keeps the old frame and shows
    /// it beyond a front that travels out from the press.
    pub commit: bool,
    /// Face for each font role in this subtree (display, body, mono).
    pub fonts: Option<[u8; 3]>,
    /// How loose this element is at rest, 0..1 (a picture at 1 shows its marks).
    pub loose: f32,
    /// A compare wipe over this element: the line's position as a fraction of its width. Right of it
    /// the element is fully loosened, left of it untouched.
    pub wipe: Option<f32>,
    /// Identity across pages: on a page change (`Cmd::Flow`) the instances of a subtree with the same
    /// flow id on both pages move from their old place to the new one instead of being replaced.
    pub flow: Option<String>,
    /// Laid out but not drawn and takes no input, unless a morph is showing it (DESIGN.md §1.10).
    pub hidden: bool,
    /// Moved by (dx, dy) logical px after layout, with everything inside it: for motion that follows
    /// the pointer (parallax), which then needs no layout. Hit testing and drawing see the moved box.
    pub shift: (f32, f32),
    /// Marks this element sheds while it moves (DESIGN.md §1.22).
    pub shed: Option<crate::motes::Shed>,
    /// Marks carried along a noise field over this element's box (DESIGN.md §1.22).
    pub flow_field: Option<crate::motes::FlowField>,
    /// Moving, it stirs the field map behind it (DESIGN.md §1.29).
    pub stirr: Option<Stirr>,
    /// A long path drawn by progress (DESIGN.md §1.30).
    pub trace: Option<crate::strand::Trace>,
    /// An explicit callout for the annotation layer (DESIGN.md §1.7).
    pub annotate: Option<String>,
    /// The annotation layer only looks inside subtrees marked as its scope (when any is).
    pub gaze_scope: bool,
    /// Per-element material (collage, §1.6): the material id for this subtree, inherited.
    pub material: Option<u8>,
    /// Looseness for the whole subtree (inherited; the larger of this and each element's own wins).
    pub loosen: f32,
    /// Placed just below the element with this key (as laid out in the same pass) instead of where
    /// the layout puts it: for menus in a modal layer.
    pub anchor: Option<String>,
    /// Clips its children to its border box (without scrolling).
    pub clips: bool,
    /// Fronts pass this subtree by: nothing in its box is pushed or loosened by them (a scene's
    /// controls while the page is shaken).
    pub calm: bool,
    /// What is put in the field map inside this subtree's box is back at rest after lives of its own
    /// (energy, flow; s, 0.1–30): `El::map_life`.
    pub map_life: Option<(f32, f32)>,
    /// A text field of several lines (Enter inserts a line break).
    pub multiline: bool,
    /// The text in this subtree can be selected with the pointer and copied (read-only).
    pub selectable: bool,
    /// The pointer's shape over this element (else derived: text, pointer, grab).
    pub cursor: Option<Cursor>,
    /// Ripple scope, response, legibility and recomposition (DESIGN.md §1.15, §1.16).
    pub ripple: Ripple,
    /// A closed outline (local logical px) the annotation layer draws over this element (`El::guide`).
    pub guide: Option<Rc<Vec<(f32, f32)>>>,
    /// This subtree is drawn as its own loading skeleton (`El::skeleton`), inherited.
    pub skeleton: bool,
    /// Lines and rows run right to left in this subtree (inherited; `None` follows the parent).
    pub rtl: Option<bool>,
    /// Appears and disappears by decoding (`El::decode`, DESIGN.md §1.28).
    pub decode: Option<crate::decode::Decode>,
    /// Posed glyphs on a form: how the ones turned away, edge on, hidden behind nearer ones and far
    /// away are drawn (`El::form`).
    pub form: Option<crate::form::FormFx>,
    /// Turned by this many radians (clockwise) about its centre after layout, with everything inside
    /// it; hit testing follows (`El::turn`).
    pub turn: f32,
    /// How clips and poses take its text away (`El::reveal_by`).
    pub reveal_by: Reveal,
    /// It keeps moving on purpose (a live demo, a clock): the audit's "keeps asking for frames" does
    /// not count it (`El::ongoing`).
    pub ongoing: bool,
    /// The pointer passes through it to what is under it (`El::pass`); its children still take it.
    pub pass: bool,
    /// How opaque it is with its subtree, 0..1 (`El::opacity`).
    pub opacity: f32,
    /// A commit pressed within it plays only within its box (`El::commit_scope`).
    pub commit_scope: bool,
    /// A preview of components (`El::preview`): modal ones in it are drawn where they are written.
    pub preview: bool,
    /// Its one child is zoomed to fit its box (`El::fit`).
    pub fit: Option<Fit>,
    /// A text element shows at most this many lines, ending in an ellipsis (`El::clamp`; 0: all).
    pub clamp: u16,
    /// What its subtree draws stays (`El::plate`).
    pub plate: Option<Plate>,
    /// Its subtree is drawn once and kept until it changes (`El::cached`).
    pub cached: bool,
    /// Its clip has round corners of this radius, logical px (`El::clip_round`).
    pub clip_radius: f32,
    /// It keeps its own size when zoomed (`El::own_size`): the point of its box (0–1 across, down) that
    /// stays where the zoomed box would put it, and the zoom it was given (1: none).
    pub own_size: Option<([f32; 2], f32)>,
    /// The look's grain is drawn over it (`El::grain`).
    pub grain: bool,
    /// The look's bloom is drawn over it (`El::bloom`).
    pub bloom: bool,
    /// Shown only while it is clear (`El::show_clear`): how far inside its view it must be, and its rank.
    pub show_clear: Option<(f32, u8)>,
    /// How much a view scale above it (`El::view_scale`) scaled its geometry and not its pen: a drawing's
    /// strokes are drawn this much wider than its scale makes them (1: none).
    pub pen: f32,
    /// It is a view scale's container (`El::view_scale`): the scale.
    pub view_scale: Option<f32>,
    /// It is a stage (`El::stage`): when it is slow to draw, it holds its last image while input comes.
    pub stage: bool,
    /// It is a stack (`stack`): its children are laid over one another, centred.
    pub stack: bool,
    /// It can be viewed alone when it steps (`El::steps`).
    pub steps: bool,
    /// What its handlers send changes only it and the elements that step (`El::local_messages`).
    pub local_messages: bool,
}
impl Props {
    #[inline(never)]
    fn new(style: Style, content: Content) -> Self { Props { key: String::new(), style, content, fill: None, fill_on: PaintOn { hover: None, held: None, border_hover: None }, border: None, radius: 0.0, focusable: false, tab_order: 0, skip_to: None, focus_shown: false, scroll: false, horizontal: false, bar: false, reveal: false, disabled: false, commit: false, fonts: None, loose: 0.0, wipe: None, flow: None, hidden: false, shift: (0.0, 0.0), shed: None, flow_field: None, stirr: None, trace: None, annotate: None, gaze_scope: false, material: None, loosen: 0.0, anchor: None, clips: false, calm: false, map_life: None, multiline: false, selectable: false, cursor: None, ripple: Ripple::default(), guide: None, skeleton: false, rtl: None, decode: None, form: None, turn: 0.0, reveal_by: Reveal::Glyph, ongoing: false, pass: false, opacity: 1.0, commit_scope: false, preview: false, fit: None, clamp: 0, plate: None, cached: false, clip_radius: 0.0, own_size: None, grain: false, bloom: false, show_clear: None, pen: 1.0, view_scale: None, stage: false, stack: false, steps: false, local_messages: false } }
}
impl<M> std::ops::Deref for El<M> { type Target = Props; fn deref(&self) -> &Props { &self.props } }
impl<M> std::ops::DerefMut for El<M> { fn deref_mut(&mut self) -> &mut Props { &mut self.props } }

/// Rows of a virtual list: how many, how tall each (logical px), how many beyond the visible ones to
/// build on each side, what row `i` is, and which row is selected (it is scrolled into view after a key).
pub struct Virtual<M> {
    /// How many rows there are.
    pub count: usize,
    /// Each row's height, logical px.
    pub row_h: f32,
    /// Rows built beyond the view on each side.
    pub overscan: usize,
    /// Builds row `i`.
    pub build: Rc<dyn Fn(usize) -> El<M>>,
    /// The selected row, kept in view when the keyboard changes it.
    pub selected: Option<usize>,
}

/// Roles of the accessibility mirror (the WAI-ARIA roles of the same name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)] // Each variant is the ARIA role `Role::aria` names.
pub enum Role {
    Button, Switch, Slider, SpinButton, TabList, Tab, TextBox, Dialog, List, ListItem, Img, Heading, Group, Text,
    Checkbox, Radio, RadioGroup, Combobox, Listbox, Option, Tooltip, Status, ProgressBar, Region, Table, Row, Cell, ColumnHeader,
    Navigation, Menu, MenuItem, Separator, Toolbar, Link,
}
impl Role {
    /// The ARIA role name.
    pub fn aria(self) -> &'static str {
        match self {
            Role::Button => "button", Role::Switch => "switch", Role::Slider => "slider", Role::SpinButton => "spinbutton",
            Role::TabList => "tablist", Role::Tab => "tab", Role::TextBox => "textbox", Role::Dialog => "dialog", Role::List => "list",
            Role::ListItem => "listitem", Role::Img => "img", Role::Heading => "heading", Role::Group => "group", Role::Text => "text",
            Role::Checkbox => "checkbox", Role::Radio => "radio", Role::RadioGroup => "radiogroup", Role::Combobox => "combobox", Role::Listbox => "listbox",
            Role::Option => "option", Role::Tooltip => "tooltip", Role::Status => "status", Role::ProgressBar => "progressbar", Role::Region => "region",
            Role::Table => "table", Role::Row => "row", Role::Cell => "cell", Role::ColumnHeader => "columnheader", Role::Navigation => "navigation",
            Role::Menu => "menu", Role::MenuItem => "menuitem", Role::Separator => "separator", Role::Toolbar => "toolbar", Role::Link => "link",
        }
    }
}
/// An element's accessible role, name and state. Unset parts are derived (the name from the text inside).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Access {
    /// Its role (else one is inferred).
    pub role: Option<Role>,
    /// Its name (else its text).
    pub label: Option<String>,
    /// A slider's or spin button's value, minimum and maximum.
    pub value: Option<(f32, f32, f32)>,
    /// A text box's text.
    pub text: Option<String>,
    /// On or off (a checkbox, a switch).
    pub checked: Option<bool>,
    /// Chosen (a tab, an option).
    pub selected: Option<bool>,
    /// A heading's level.
    pub level: u8,
    /// Whether what this control opens (a disclosure's panel, a combobox's list, a menu) is open.
    pub expanded: Option<bool>,
    /// A longer description than the name (help text under a field, a hint).
    pub description: Option<String>,
    /// Announce changes to this element's text without moving focus (a status line, a toast).
    pub live: Option<Live>,
    /// A field whose value is not accepted.
    pub invalid: bool,
    /// A field that must be filled.
    pub required: bool,
}

/// How urgently a live region's changes are announced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    /// When the screen reader is next idle.
    Polite,
    /// At once, interrupting.
    Assertive,
}
impl Live {
    /// The `aria-live` value.
    pub fn aria(self) -> &'static str { match self { Live::Polite => "polite", Live::Assertive => "assertive" } }
}

/// Pointer shapes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cursor {
    /// The arrow.
    #[default]
    Default,
    /// A hand: something to press.
    Pointer,
    /// An I-beam: text to select or edit.
    Text,
    /// An open hand: something to drag.
    Grab,
    /// A closed hand: dragging.
    Grabbing,
    /// Resizing a column.
    ColResize,
    /// Resizing a row.
    RowResize,
}
impl Cursor {
    /// The CSS name.
    pub fn css(self) -> &'static str {
        match self { Cursor::Default => "default", Cursor::Pointer => "pointer", Cursor::Text => "text", Cursor::Grab => "grab", Cursor::Grabbing => "grabbing", Cursor::ColResize => "col-resize", Cursor::RowResize => "row-resize" }
    }
}

impl<M> El<M> {
    fn new(style: Style, content: Content) -> Self {
        // Nothing shrinks unless asked: fixed sizes stay fixed inside columns and scroll areas.
        let style = Style { flex_shrink: 0.0, ..style };
        Self { props: Props::new(style, content), on_press: None, on_key: None, on_drag: None, on_blur: None, modal: None, on_text: None, virt: None, access: None, on_pick: None, on_wheel: None, on_move: None, on_visible: None, on_outline: None, on_pens: None, children: vec![] }
    }
    /// Its key: unique on the page, stable across views (focus, scroll, births and recomposition follow it).
    pub fn key(mut self, k: impl Into<String>) -> Self { self.key = k.into(); self }
    /// Space between its children, logical px, both ways.
    pub fn gap(mut self, g: f32) -> Self { self.style.gap = Size { width: length(g), height: length(g) }; self }
    /// Padding: `x` left and right, `y` top and bottom, logical px.
    pub fn pad(mut self, x: f32, y: f32) -> Self { self.style.padding = Rect { left: length(x), right: length(x), top: length(y), bottom: length(y) }; self }
    /// A fixed width, logical px.
    pub fn width(mut self, w: f32) -> Self { self.style.size.width = length(w); self }
    /// A fixed height, logical px.
    pub fn height(mut self, h: f32) -> Self { self.style.size.height = length(h); self }
    /// At least this wide.
    pub fn min_width(mut self, w: f32) -> Self { self.style.min_size.width = length(w); self }
    /// At most this wide.
    pub fn max_width(mut self, w: f32) -> Self { self.style.max_size.width = length(w); self }
    /// At least this tall.
    pub fn min_height(mut self, h: f32) -> Self { self.style.min_size.height = length(h); self }
    /// As wide as its parent.
    pub fn fill_width(mut self) -> Self { self.style.size.width = percent(1.0); self }
    /// Width as a fraction of the parent's.
    pub fn width_frac(mut self, f: f32) -> Self { self.style.size.width = percent(f); self }
    /// As tall as its parent.
    pub fn fill_height(mut self) -> Self { self.style.size.height = percent(1.0); self }
    /// Take a share of the free space along the parent's main axis.
    pub fn grow(mut self, g: f32) -> Self { self.style.flex_grow = g; self.style.flex_basis = length(0.0); self.style.flex_shrink = 1.0; self }
    /// Its flex basis, logical px.
    pub fn basis(mut self, b: f32) -> Self { self.style.flex_basis = length(b); self }
    /// Let it shrink when its row or column is short of room (nothing shrinks by default).
    pub fn shrink(mut self) -> Self { self.style.flex_shrink = 1.0; self }
    /// Let its children wrap onto more lines.
    pub fn wrap(mut self) -> Self { self.style.flex_wrap = FlexWrap::Wrap; self }
    /// How its children sit across its axis.
    pub fn align(mut self, a: AlignItems) -> Self { self.style.align_items = a; self }
    /// How its children are spread along its axis.
    pub fn justify(mut self, j: JustifyContent) -> Self { self.style.justify_content = j; self }
    /// Its children centred both ways.
    pub fn center(self) -> Self { self.align(AlignItems::CENTER).justify(JustifyContent::CENTER) }
    /// A background.
    pub fn fill(mut self, p: Paint) -> Self { self.fill = Some(p); self }
    /// The background while the pointer is over it, or over the control it is part of (the nearest
    /// pressable element at or above it). Without one it keeps its fill at rest.
    pub fn fill_hover(mut self, p: Paint) -> Self { self.fill_on.hover = Some(p); self }
    /// The background while it, or the control it is part of, is held: pressed, or dragged. Without
    /// one it takes its hover fill while the pointer is over it, else its fill at rest.
    pub fn fill_held(mut self, p: Paint) -> Self { self.fill_on.held = Some(p); self }
    /// The border while the pointer is over it or it has focus, itself or the control it is part of (the
    /// nearest pressable or focusable element at or above it). Without one it keeps its border at rest.
    pub fn border_hover(mut self, p: Paint) -> Self { self.fill_on.border_hover = Some(p); self }
    /// A border `w` logical px wide (it takes room in the layout, as CSS's does).
    pub fn border(mut self, w: f32, p: Paint) -> Self { self.border = Some((w, p)); self.style.border = Rect { left: length(w), right: length(w), top: length(w), bottom: length(w) }; self }
    /// Rounded corners of the fill and border, logical px.
    pub fn radius(mut self, r: f32) -> Self { self.radius = r; self }
    /// Send `m` on a press, Enter or Space; it takes focus.
    pub fn on_press(mut self, m: M) -> Self { self.on_press = Some(m); self.focusable = true; self }
    /// Hear keys while it has focus (the flag is Shift); returning a message uses the key. It takes focus.
    pub fn on_key(mut self, f: impl Fn(Key, bool) -> Option<M> + 'static) -> Self { self.on_key = Some(Rc::new(f)); self.focusable = true; self }
    /// Send `m` when focus leaves it.
    pub fn on_blur(mut self, m: M) -> Self { self.on_blur = Some(m); self }
    /// Make this a text field (see `TextInput`); it can take focus.
    pub fn on_text(mut self, f: impl Fn(TextInput) -> Option<M> + 'static) -> Self { self.on_text = Some(Rc::new(f)); self.focusable = true; self }
    /// A text field of several lines: Enter breaks the line, Up / Down and Home / End move by lines.
    pub fn multiline(mut self) -> Self { self.multiline = true; self }
    /// Let the pointer select the text in this subtree (read-only), to copy it.
    pub fn selectable(mut self) -> Self { self.selectable = true; self }
    /// The pointer's shape over this element.
    pub fn cursor(mut self, c: Cursor) -> Self { self.cursor = Some(c); self }
    fn access_mut(&mut self) -> &mut Access { self.access.get_or_insert_with(Access::default) }
    /// The same subtree with every key written under `prefix` (`prefix/key`), anchors too: one example
    /// shown several times on a page keeps its keys unique.
    pub fn rekey(mut self, prefix: &str) -> Self {
        if !self.key.is_empty() { self.key = format!("{prefix}/{}", self.key); }
        if let Some(a) = &self.anchor { self.anchor = Some(format!("{prefix}/{a}")); }
        self.children = std::mem::take(&mut self.children).into_iter().map(|c| c.rekey(prefix)).collect();
        self
    }
    /// This subtree runs right to left (DESIGN.md §1.20): a row's first child is at its right, its
    /// start alignment is the right edge, padding and margins mirror. Inherited.
    pub fn rtl(mut self, on: bool) -> Self { self.rtl = Some(on); self }
    /// Its accessible name (what a screen reader says), when the text inside does not say it.
    pub fn label(mut self, name: impl Into<String>) -> Self { self.access_mut().label = Some(name.into()); self }
    /// Its role in the accessibility mirror.
    pub fn role(mut self, r: Role) -> Self { self.access_mut().role = Some(r); self }
    /// On or off (a checkbox, a switch), for the mirror.
    pub fn checked(mut self, on: bool) -> Self { self.access_mut().checked = Some(on); self }
    /// Selected (a tab, an option, a list row), for the mirror.
    pub fn selected(mut self, on: bool) -> Self { self.access_mut().selected = Some(on); self }
    /// A slider's or spin button's value and range, for the mirror.
    pub fn value_range(mut self, v: f32, min: f32, max: f32) -> Self { self.access_mut().value = Some((v, min, max)); self }
    /// A text box's text, for the mirror.
    pub fn text_value(mut self, t: impl Into<String>) -> Self { self.access_mut().text = Some(t.into()); self }
    /// A heading of `level` (1 the page's title).
    pub fn heading(mut self, level: u8) -> Self { let a = self.access_mut(); a.role = Some(Role::Heading); a.level = level; self }
    /// Whether what this control opens is open (a disclosure, a combobox, a menu button).
    pub fn expanded(mut self, open: bool) -> Self { self.access_mut().expanded = Some(open); self }
    /// A description read after the name (help text, a hint).
    pub fn description(mut self, d: impl Into<String>) -> Self { self.access_mut().description = Some(d.into()); self }
    /// Announce changes to its text without moving focus. `Role::Status` is polite by default.
    pub fn live(mut self, l: Live) -> Self { self.access_mut().live = Some(l); self }
    /// The field's value is not accepted (say why with `description`).
    pub fn invalid(mut self, on: bool) -> Self { self.access_mut().invalid = on; self }
    /// The field must be filled.
    pub fn required(mut self, on: bool) -> Self { self.access_mut().required = on; self }
    /// Make this (a scroll node) a virtual list of `count` rows `row_h` tall: only the visible rows and
    /// `overscan` more on each side exist in the tree; row `i` is `build(i)` (keyed `key/row{i}` unless it
    /// has a key). Its children are replaced by the rows.
    pub fn virtual_rows(mut self, count: usize, row_h: f32, overscan: usize, selected: Option<usize>, build: impl Fn(usize) -> El<M> + 'static) -> Self {
        self.virt = Some(Virtual { count, row_h, overscan, build: Rc::new(build), selected });
        self
    }
    /// Show bytes `a..b` of a text element as selected (while its field has focus).
    pub fn select(mut self, a: usize, b: usize) -> Self { if let Content::Text { select, .. } = &mut self.content { *select = (a != b).then_some((a.min(b), a.max(b))); } self }
    /// Underline bytes `a..b` of a text element (text being composed).
    pub fn mark(mut self, a: usize, b: usize) -> Self { if let Content::Text { mark, .. } = &mut self.content { *mark = (a < b).then_some((a, b)); } self }
    /// Keep a text element on one line.
    pub fn nowrap(mut self) -> Self { if let Content::Text { nowrap, .. } = &mut self.content { *nowrap = true; } self }
    /// A drag reported as the pointer's fraction of the element (0..1), from the press on.
    pub fn on_drag(self, f: impl Fn(f32, f32) -> M + 'static) -> Self { self.on_drag_with(move |d| Some(f(d.fx(), d.fy()))) }
    /// A press and the drag that follows, each step as a `Drag`; it takes focus.
    pub fn on_drag_with(mut self, f: impl Fn(Drag) -> Option<M> + 'static) -> Self { self.on_drag = Some(Rc::new(f)); self }
    /// Whether Tab and presses can give this element focus (pressable elements are by default).
    pub fn focusable(mut self, f: bool) -> Self { self.focusable = f; self }
    /// Its place in the Tab order: positive numbers come first, lowest first; 0 (the default) keeps
    /// document order after them; a negative number leaves it out of Tab, though a press, a skip link
    /// or `Ui::focus_key` can still focus it.
    pub fn tab_order(mut self, order: i32) -> Self { self.tab_order = order; self }
    /// Make it a skip link: Enter, Space, a press or assistive technology's activation moves focus to
    /// the element `target`, or the first element inside it that takes focus, and scrolls it into view.
    /// It takes focus itself.
    pub fn skip_to(mut self, target: impl Into<String>) -> Self { self.skip_to = Some(target.into()); self.focusable = true; self }
    /// Draw it and what it holds only while focus is on it or inside it (a skip link that appears when
    /// Tab reaches it). Screen readers still find it, and it still takes focus and presses.
    pub fn focus_shown(mut self) -> Self { self.focus_shown = true; self }
    /// Take no input and draw faint, with everything inside.
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    /// Pressing it commits a new state (a theme, a material): the old frame stays beyond a front that
    /// travels out from the press.
    pub fn commit(mut self) -> Self { self.commit = true; self }
    /// Scroll it into view after a key changes the state (a selected row).
    pub fn reveal(mut self, r: bool) -> Self { self.reveal = r; self }
    /// Open this element as a modal layer; see `Modal`.
    pub fn modal(mut self, dismiss: M, beneath: f32) -> Self { self.modal = Some(Modal { dismiss, beneath }); self }
    /// Put a caret at byte `at` of a text element (drawn only while it has focus).
    pub fn caret(mut self, at: usize) -> Self { if let Content::Text { caret, .. } = &mut self.content { *caret = Some(at); } self }
    /// The faces display, body and mono resolve to in this subtree.
    pub fn fonts(mut self, roles: [u8; 3]) -> Self { self.fonts = Some(roles); self }
    /// How loose it is at rest, 0–1.
    pub fn loose(mut self, k: f32) -> Self { self.loose = k; self }
    /// A compare wipe at `x`, a fraction of its width: loosened right of it, untouched left.
    pub fn wipr(mut self, x: f32) -> Self { self.wipe = Some(x); self }
    /// Fronts do not push or loosen anything in this element's box.
    pub fn calm(mut self) -> Self { self.calm = true; self }
    /// What is put in the field map inside this subtree's box is back at rest `energy_life` and
    /// `flow_life` seconds (0.1–30) after it was put in, however much and however wide: the look's map
    /// slowed down there, with nothing spread (DESIGN.md §1.29). A trail that takes six seconds to close
    /// on one piece of a page.
    pub fn map_life(mut self, energy_life: f32, flow_life: f32) -> Self { self.map_life = Some((energy_life.clamp(0.1, 30.0), flow_life.clamp(0.1, 30.0))); self }
    /// The focus marks frame it only when it has keyboard focus, never on hover.
    pub fn unframed(mut self) -> Self { self.ripple.unframed = true; self }
    /// Give this subtree an identity across pages (see `flow`).
    pub fn flow(mut self, id: impl Into<String>) -> Self { self.flow = Some(id.into()); self }
    /// Lay this out but do not draw it until a morph shows it.
    pub fn hidden_until_morphed(mut self, h: bool) -> Self { self.hidden = h; self }
    /// Move this subtree by (dx, dy) logical px after layout (no layout when only this changes).
    pub fn shift(mut self, dx: f32, dy: f32) -> Self { self.shift = (dx, dy); self }
    /// Where the look's grain (`PostSpec::grain`) is drawn: once any element on the page is marked, the
    /// grain is drawn only inside the box round all that marked elements draw (each within its clip), the
    /// same grain as over the whole frame (its hash, amount and cell in device px), so a window keeps the
    /// page's grain and a picture that flows carries it where it is drawn each frame. With none marked it
    /// covers the whole frame.
    pub fn grain(mut self) -> Self { self.grain = true; self }
    /// Where the look's bloom (`PostSpec::bloom`) is drawn: once any element on the page is marked, the
    /// bloom glows only inside the box round all that marked elements draw (each within its clip), and
    /// only from what is drawn inside it, so a bright control beside a marked picture neither glows nor
    /// lights the picture. With none marked it covers the whole frame, as before.
    pub fn bloom(mut self) -> Self { self.bloom = true; self }
    /// Keep this element's own size when it is zoomed (`El::zoom`) or carried by a picture that grows
    /// (`Cmd::Flow`): a label on a scaled drawing, at its own size and sharp at any device pixel ratio.
    /// Its type, box, border and pose stay as they are; where it is (insets, margins, `shift`) scales,
    /// and it is placed so that its point (`ax`, `ay`: 0–1 across and down its box) is where the zoomed
    /// box would have put that point. Within a row or column it takes its own size's room.
    pub fn own_size(mut self, ax: f32, ay: f32) -> Self { self.own_size = Some(([ax.clamp(0.0, 1.0), ay.clamp(0.0, 1.0)], self.own_size.map_or(1.0, |o| o.1))); self }
    /// Shed marks while this element moves (`rate` 0: none).
    pub fn shed(mut self, s: crate::motes::Shed) -> Self { self.shed = Some(s); self }
    /// Leave a trail in the field map while this element moves (by a drag, a spring, a flow or
    /// `shift`): along the way its centre went, energy and flow in the direction it went (DESIGN.md §1.29).
    pub fn stirr(mut self, s: Stirr) -> Self { self.stirr = Some(s); self }
    /// Draw a long path in this element's box by progress, a line of constant width that the field map's
    /// flow pushes and whose head stirs the map (DESIGN.md §1.30). Any material.
    pub fn trace(mut self, t: crate::strand::Trace) -> Self { self.trace = Some(t); self }
    /// Carry marks along a noise field over this element's box (`speed` 0: they stand still).
    pub fn flow_field(mut self, f: crate::motes::FlowField) -> Self { self.flow_field = Some(f); self }
    /// A callout label for the annotation layer ("①  电源开关").
    pub fn annotate(mut self, label: impl Into<String>) -> Self { self.annotate = Some(label.into()); self }
    /// Make this subtree what the annotation layer looks at.
    pub fn gaze_scope(mut self) -> Self { self.gaze_scope = true; self }
    /// Draw this subtree with another material (`style::Material` id), under the collage material.
    pub fn material(mut self, id: u8) -> Self { self.material = Some(id); self }
    /// Put a material, a response and a scope on this subtree in one line (DESIGN.md §1.6, §1.15):
    /// `page.effect(Effect { material: Some(Material::Smudgr), response: Some(Response::Loosen), scope: Some(Scope::Container(None)) })`.
    /// What is `None` stays as it is. A scope of `Scope::Container(None)` also makes this element the
    /// container, so the effect's fronts stay inside it.
    pub fn effect(mut self, e: Effect) -> Self {
        if let Some(m) = e.material { self = self.material(m.id()); }
        if let Some(r) = e.response { self = self.respond(r); }
        if let Some(s) = e.scope {
            if s == Scope::Container(None) { self = self.ripplr_scope(); }
            self = self.ripplr(s);
        }
        self
    }
    /// Hold the whole subtree at looseness `k`.
    pub fn loosen(mut self, k: f32) -> Self { self.loosen = k; self }
    /// Place this element just below the element `key` (see `anchor`).
    pub fn anchor(mut self, key: impl Into<String>) -> Self { self.anchor = Some(key.into()); self }
    /// Clip the children inside this element's border: what lies outside is not drawn and cannot be hit.
    pub fn clip(mut self) -> Self { self.clips = true; self.style.overflow = taffy::Point { x: taffy::Overflow::Clip, y: taffy::Overflow::Clip }; self }
    /// Take this element out of the flow and pin it to its parent's edges (logical px; `None` is auto).
    pub fn absolute(mut self, left: Option<f32>, top: Option<f32>, right: Option<f32>, bottom: Option<f32>) -> Self {
        let v = |o: Option<f32>| o.map_or(auto(), length);
        self.style.position = taffy::Position::Absolute;
        self.style.inset = Rect { left: v(left), top: v(top), right: v(right), bottom: v(bottom) };
        self
    }
    /// Show a text element in at most `lines` lines (0: all of it). Text that does not fit ends in this
    /// face's own ellipsis on the last line, in the text's colour and size, on the baseline, drawn by
    /// the material as the rest of the line is (no fade, no mask). It breaks after a word, or between
    /// characters when that would leave more than a quarter of the last line empty (a word wider than the
    /// line is cut in it too), and leaves no space or punctuation before the ellipsis. `Ui::clamped` says whether it was cut; assistive
    /// technology reads the whole text. Taking the clamp away lets the rest come in as text that
    /// changes does. With `El::around` it does not clamp yet (`Ui::problems` says so).
    pub fn clamp(mut self, lines: u16) -> Self { self.clamp = lines; self }
    /// Extra letter spacing, in em (−0.05–1.5, DESIGN.md §4.2): laid out by parley, so the measured width,
    /// selections, recomposition and scribblr's outlines all follow it.
    pub fn tracking(mut self, em: f32) -> Self { if let Content::Text { track, .. } = &mut self.content { *track = em.clamp(-0.05, 1.5); } self }
    /// Whisper (DESIGN.md §4.2): this text in lowercase, in the serif, tracked 0.6 em: the small, airy
    /// type beside a picture or along an edge. Keep it small (11–13 px).
    pub fn whisper(mut self) -> Self {
        if let Content::Text { s, font, track, .. } = &mut self.content { *s = s.to_lowercase(); *font = Font::Whisper; *track = 0.6; }
        self
    }
    /// Set a text element in another face or role.
    pub fn font(mut self, f: Font) -> Self { if let Content::Text { font, .. } = &mut self.content { *font = f; } self }
    /// Print a text element through a lattice of this pitch (logical px).
    pub fn print(mut self, pitch: f32) -> Self { if let Content::Text { print, .. } = &mut self.content { *print = pitch; } self }
    /// Make a text element bolder (px > 0) or lighter (px < 0) by moving its outline (logical px).
    pub fn weight(mut self, px: f32) -> Self { if let Content::Text { weight, .. } = &mut self.content { *weight = px; } self }
    /// Let a text element flow around the true outlines in `a` (coordinates relative to its own
    /// content box), keeping `a.gap` px from their ink.
    pub fn around(mut self, a: Around) -> Self { if let Content::Text { around, .. } = &mut self.content { *around = Some(Rc::new(a)); } self }
    /// How this subtree answers fronts, the lens and pulls (inherited).
    pub fn respond(mut self, r: Response) -> Self { self.ripple.respond = Some(r); self }
    /// Mark this subtree as text to read / a control (`true`), so field effects loosen it at most
    /// `Look::legible_cap`, or as decoration (`false`). Inherited; text and controls are legible by default.
    pub fn legible(mut self, on: bool) -> Self { self.ripple.legible = Some(on); self }
    /// What a press in this subtree sends a front to (inherited). Controls default to `Scope::Own`;
    /// pressing plain ground sends nothing unless a scope is set here.
    pub fn ripplr(mut self, s: Scope) -> Self { self.ripple.scope = Some(s); self }
    /// Fronts sent with `Scope::Container(None)` from inside stop at this element's box.
    pub fn ripplr_scope(mut self) -> Self { self.ripple.container = true; self }
    /// Whether a change of this text's string recomposes the changed glyphs (inherited; on by default
    /// for text with an explicit key, off inside text fields).
    pub fn recompose(mut self, on: bool) -> Self { self.ripple.recompose = Some(on); self }
    /// Choose the `Markr` slot of a picture.
    pub fn markr(mut self, slot: u8) -> Self { if let Content::Picture { fx, .. } = &mut self.content { *fx = slot; } self }
    /// Hear which copy of a `scene3d` element the pointer is over and which one it presses.
    pub fn on_pick(mut self, f: impl Fn(crate::mesh::Pick) -> Option<M> + 'static) -> Self { self.on_pick = Some(Rc::new(f)); self }
    /// Take the wheel over this element (before any scroll node, unless a gesture already scrolling comes over it):
    /// dx, dy in logical px. `None` leaves the step to the scroll nodes.
    pub fn on_wheel(mut self, f: impl Fn(f32, f32) -> Option<M> + 'static) -> Self { self.on_wheel = Some(Rc::new(f)); self }
    /// Draw this subtree as its own skeleton (DESIGN.md §1.23): laid out exactly as it is, but every
    /// line of text is a bar as wide as that line measures, every picture or 3D element a block, every
    /// fill and border faint; it takes no input. Swapping `.skeleton()` off shows the content in the same
    /// boxes, so nothing moves.
    pub fn skeleton(mut self) -> Self { self.skeleton = true; self }
    /// A closed outline through `points` (logical px from this element's top left), drawn dotted by the
    /// annotation layer in its colour whatever its mode: a debug overlay (a menu's safe zone).
    pub fn guide(mut self, points: Vec<(f32, f32)>) -> Self { self.guide = (points.len() >= 2).then(|| Rc::new(points)); self }
    /// Hear a mouse or pen moving over this element (`Some((x, y))`, logical px from its top left) and
    /// leaving it (`None`): for menus that steer by where the pointer goes. Fingers have no hover.
    pub fn on_move(mut self, f: impl Fn(Option<(f32, f32)>) -> Option<M> + 'static) -> Self { self.on_move = Some(Rc::new(move |p: Option<Point>| f(p.map(|p| (p.x, p.y))))); self }
    /// Draw this text's posed glyphs as lying on a form (`form::FormFx`): far ones first, the ones
    /// turned away and edge on faded out, the ones mostly hidden by nearer ones left out, the far
    /// side dimmed. Use with `El::pose` (or `form::Form::pose`).
    pub fn form(mut self, fx: crate::form::FormFx) -> Self { self.form = Some(fx); self }
    /// Turn this element and everything inside it by `radians` (clockwise) about its centre, after
    /// layout: drawn turned, and pressed where it is drawn.
    pub fn turn(mut self, radians: f32) -> Self { self.turn = radians; self }
    /// How clips, poses and the window take this text away: `Reveal::Word` or `Reveal::Line` show a
    /// word (a line) whole or not at all, and fade it as one.
    pub fn reveal_by(mut self, r: Reveal) -> Self { self.reveal_by = r; self }
    /// This element keeps moving on purpose (a live demo): the audit does not report the frames it asks for.
    pub fn ongoing(mut self) -> Self { self.ongoing = true; self }
    /// The pointer passes through this element (`true`): presses, hovering and the wheel go to what is
    /// under it, as if it were not there; its children take the pointer as usual. It is still drawn,
    /// and the field still reaches it (a front loosens it, the light falls on it). For a drawing laid
    /// over controls. Off by default.
    pub fn pass(mut self, on: bool) -> Self { self.pass = on; self }
    /// How opaque this element is, with everything in it, 0..1 (1 by default). The subtree is drawn
    /// as one and then made see-through as one: where two of its children overlap, the one on top
    /// shows, not both. Set it from the state to animate it, frame by frame. At 0 it is not drawn and
    /// takes no pointer; a screen reader still finds it.
    pub fn opacity(mut self, k: f32) -> Self { self.opacity = k.clamp(0.0, 1.0); self }
    /// A commit (`El::commit`) pressed within this element, or a `Cmd::Commit` naming it, plays only
    /// within its box: the front travels inside its clip and every pixel outside the box grown by the
    /// material's reach (`text::SPREAD` device px and the front's push) stays as it was, that frame and
    /// after; the commit is over once the front has reached the box's farthest corner. The fronts of
    /// presses within it stay inside it too (it is their container). The look's own wave speed and
    /// steps are used.
    pub fn commit_scope(mut self) -> Self { self.commit_scope = true; self }
    /// Zoom this container's one child to fit its content box (`Fit`): the child is laid out at 1 to
    /// find its natural size, then zoomed (`El::zoom`) by the scale that fits, both in the same layout.
    /// The scale depends only on the box and the child's natural size, so a larger box never gets a
    /// smaller one; with pixel type it goes down to a whole multiple of the face's size. At `Fit::min`
    /// and still too large, it is drawn at `min` and clipped, and `Ui::problems` says so. There is no
    /// hook after layout: `Ui::rect` reads where things went.
    pub fn fit(mut self, f: Fit) -> Self { self.fit = Some(f); self.clip() }
    /// Keep what this subtree draws (DESIGN.md §1.33): the next frame does not wipe it, new ink goes
    /// over the old, and a pixel drawn again is new again. It is a picture, not content: not read by
    /// assistive technology, not selectable, emptied when the window changes size and by
    /// `Cmd::ClearPlate`. The plate draws nothing of its own (no edge, no ground) and reaches as far as
    /// its clip box (it clips its children). With `fade_s` above 0 a pixel thins out over the last
    /// fifth of `fade_s` after it was drawn and is then gone. At most 4 plates in a window, together at
    /// most twice its area; more are drawn as plain elements and `Ui::problems` says so. For music
    /// videos, the lab and pages that say they are a drawing board; not for ordinary pages.
    pub fn plate(mut self, p: Plate) -> Self { self.plate = Some(Plate { fade_s: (p.fade_s.clamp(0.0, 60.0) * 2.0).round() / 2.0 }); self.clip() }
    /// Draw this subtree once and keep it (DESIGN.md §1.36): a frame that would draw it as it was draws
    /// nothing of it and lays on what it kept, so a large page of small text costs no instances while it
    /// stays as it is. It is drawn again when what it draws changes (it scrolls, moves, loosens) and after
    /// the window changes size. It is a plate (`El::plate`, with its limits): a picture, not content, not
    /// selectable or read by assistive technology, and it clips its children. Another element shows
    /// part of it with `el::cached_view`. A stage, a 3D view or a see-through group inside it is drawn
    /// as the rest is, every time it is drawn, not by itself.
    pub fn cached(mut self) -> Self { self.cached = true; self.plate(Plate { fade_s: 0.0 }) }
    /// Clip the subtree to the box inside the border with round corners of `radius` logical px (half the
    /// box's side: a circle). Edges are antialiased. Where a clip around it cuts the box, that clip's own
    /// edge is straight, and the corners stay where the uncut box has them only when it is not cut. A
    /// press is still taken anywhere in the box.
    pub fn clip_round(mut self, radius: f32) -> Self { self.clip_radius = radius.max(0.0); self.clip() }
    /// A preview of components (a gallery's or a document's specimen), for the whole subtree: a modal
    /// element in it (a sheet, a context menu, a dialog) is drawn where it is written, laid out and
    /// clipped by this container, without a layer: nothing beneath it is held loose, focus is not kept
    /// inside it, a press outside the container or Esc sends no dismiss, input outside is not blocked,
    /// and assistive technology is not told it is modal. What is drawn only with focus (`focus_shown`,
    /// a skip link) is always drawn, over its siblings as when it has focus. The controls inside are
    /// pressed and focused as usual. It clips its children inside its border.
    pub fn preview(mut self, on: bool) -> Self { self.preview = on; if on { self = self.clip(); } self }
    /// The pointer over this element and dragging on it, as one handler: `Some(Point)` while it moves
    /// over it (`down` false: a mouse or pen hovering) or presses and drags (`down` true until let go;
    /// a finger only does this), `None` when it leaves. On a canvas the point is in the canvas's space.
    /// The focus marks do not frame it (it is a surface, not a control).
    pub fn on_point(mut self, f: impl Fn(Option<Point>) -> Option<M> + 'static) -> Self {
        let f = Rc::new(f);
        let g = f.clone();
        self.on_move = Some(Rc::new(move |p: Option<Point>| g(p)));
        self.on_drag = Some(Rc::new(move |d: Drag| f(Some(Point { x: d.x, y: d.y, down: d.phase != Phase::End, glyph: None, z: 0.0, t: d.t }))));
        self.ripple.unframed = true;
        self
    }
    /// Hear when this element comes into view or leaves it (inside its scroll area's and clips' view):
    /// a live demo steps only while it can be seen.
    pub fn on_visible(mut self, f: impl Fn(bool) -> Option<M> + 'static) -> Self { self.on_visible = Some(Rc::new(f)); self }
    /// Hear where the shaper set this text's glyphs: each one's pen x as laid out (logical px in the
    /// content box, kerning included, not put on a pixel; `GlyphAt::pen`), in the glyphs' order
    /// (`GlyphAt::i`), sent after a paint that draws the text when they differ from those last sent. For a
    /// state that places the letters itself (the Lab's Kern) and measures from the face's own setting.
    pub fn on_pens(mut self, f: impl Fn(Rc<Vec<f32>>) -> Option<M> + 'static) -> Self { self.on_pens = Some(Rc::new(f)); self }
    /// Be told this subtree's true outline — the coverage of its glyphs, shapes and pictures, hidden
    /// ones included — sampled on a `cols × rows` grid over its box (values 0..1), each time it changes.
    /// What it gives is a mask for a field (DESIGN.md §1.26).
    pub fn on_outline(mut self, cols: usize, rows: usize, f: impl Fn(Rc<crate::field::Grid>) -> Option<M> + 'static) -> Self { self.on_outline = Some(Outline { cols, rows, f: Rc::new(f) }); self }
    /// Appear by decoding and disappear by the same played backwards (DESIGN.md §1.28): blocks over the
    /// true outline resolve coarse to fine, then the content shows. `hold` in it scrubs to a progress.
    pub fn decode(mut self, d: crate::decode::Decode) -> Self { self.decode = Some(d); self }
    /// Draw each glyph of this text by its pose (`Pose`): on a path, on a sphere, lit by distance.
    pub fn pose(mut self, f: impl Fn(GlyphAt) -> Pose + 'static) -> Self { if let Content::Text { pose, .. } = &mut self.content { *pose = Some(Rc::new(f)); } self }
    /// Scroll its children vertically and clip them.
    pub fn scroll(mut self) -> Self { self.scroll = true; self.style.overflow.y = taffy::Overflow::Scroll; self.style.scrollbar_width = 0.0; self }
    /// Scroll horizontally instead (the wheel's vertical movement scrolls it too).
    pub fn scroll_x(mut self) -> Self { self.scroll = true; self.horizontal = true; self.style.overflow.x = taffy::Overflow::Scroll; self.style.scrollbar_width = 0.0; self }
    /// Show a thin bar for where a scroll node is.
    pub fn bar(mut self) -> Self { self.bar = true; self }
    /// Add a child.
    pub fn child(mut self, c: El<M>) -> Self { self.children.push(c); self }
    /// Add children.
    pub fn children(mut self, c: impl IntoIterator<Item = El<M>>) -> Self { self.children.extend(c); self }

    /// The same tree sending other messages: lets one application embed another's view.
    pub fn map<N: 'static>(self, f: Rc<dyn Fn(M) -> N>) -> El<N> where M: 'static {
        let El { props, on_press, on_key, on_drag, on_blur, modal, on_text, virt, on_pick, on_wheel, on_move, on_visible, on_outline, on_pens, access, children } = self;
        let virt = virt.map(|v| { let f = f.clone(); let b = v.build; Virtual { count: v.count, row_h: v.row_h, overscan: v.overscan, selected: v.selected, build: Rc::new(move |i: usize| b(i).map(f.clone())) as Rc<dyn Fn(usize) -> El<N>> } });
        let on_key = on_key.map(|k| { let f = f.clone(); Rc::new(move |key: Key, shift: bool| k(key, shift).map(|m| f(m))) as OnKey<N> });
        let on_text = on_text.map(|t| { let f = f.clone(); Rc::new(move |i: TextInput| t(i).map(|m| f(m))) as OnText<N> });
        let on_drag = on_drag.map(|d| { let f = f.clone(); Rc::new(move |g: Drag| d(g).map(|m| f(m))) as OnDrag<N> });
        let on_pick = on_pick.map(|d| { let f = f.clone(); Rc::new(move |g: crate::mesh::Pick| d(g).map(|m| f(m))) as OnPick<N> });
        let on_wheel = on_wheel.map(|d| { let f = f.clone(); Rc::new(move |x: f32, y: f32| d(x, y).map(|m| f(m))) as OnWheel<N> });
        let on_move = on_move.map(|d| { let f = f.clone(); Rc::new(move |p: Option<Point>| d(p).map(|m| f(m))) as OnMove<N> });
        let on_visible = on_visible.map(|d| { let f = f.clone(); Rc::new(move |v: bool| d(v).map(|m| f(m))) as OnVisible<N> });
        let on_pens = on_pens.map(|d| { let f = f.clone(); Rc::new(move |v: Rc<Vec<f32>>| d(v).map(|m| f(m))) as OnPens<N> });
        let on_outline = on_outline.map(|o| { let f = f.clone(); let g = o.f; Outline { cols: o.cols, rows: o.rows, f: Rc::new(move |x: Rc<crate::field::Grid>| g(x).map(|m| f(m))) } });
        // A plain loop, not a collect: mapping in place would be built again for every pair of message types.
        let mut kids = Vec::with_capacity(children.len());
        for c in children { kids.push(c.map(f.clone())); }
        El {
            props, on_press: on_press.map(|m| f(m)), on_key, on_drag, on_blur: on_blur.map(|m| f(m)), modal: modal.map(|m| Modal { dismiss: f(m.dismiss), beneath: m.beneath }),
            on_text, virt, on_pick, on_wheel, on_move, on_visible, on_outline, on_pens, access, children: kids,
        }
    }
}

/// A flex row: children left to right.
pub fn row<M>() -> El<M> { El::new(Style { display: Display::Flex, flex_direction: FlexDirection::Row, ..Default::default() }, Content::None) }
/// A flex column: children top to bottom.
pub fn col<M>() -> El<M> { El::new(Style { display: Display::Flex, flex_direction: FlexDirection::Column, ..Default::default() }, Content::None) }
/// Empty space, `w` × `h` logical px.
pub fn space<M>(w: f32, h: f32) -> El<M> { El::new(Style { size: Size { width: length(w), height: length(h) }, ..Default::default() }, Content::None) }
/// Text at `size` logical px, its line height 1.6 times that, in the body face.
pub fn text<M>(s: impl Into<String>, size: f32, paint: Paint) -> El<M> {
    El::new(Style::default(), Content::Text { s: s.into(), size, line: (size * 1.6).round(), paint, font: Font::Body, print: 0.0, caret: None, weight: 0.0, around: None, select: None, mark: None, nowrap: false, pose: None, track: 0.0 })
}
/// Map every length (not percentages, auto and the rest) of a taffy value through `f`.
fn map_lp(v: LengthPercentage, f: impl Fn(f32) -> f32) -> LengthPercentage { match v.expand() { taffy::style::ExpandedLengthPercentage::Length(x) => LengthPercentage::length(f(x)), _ => v } }
fn map_lpa(v: LengthPercentageAuto, f: impl Fn(f32) -> f32) -> LengthPercentageAuto { match v.expand() { taffy::style::ExpandedLengthPercentageAuto::Length(x) => LengthPercentageAuto::length(f(x)), _ => v } }
fn map_dim(v: Dimension, f: impl Fn(f32) -> f32) -> Dimension { match v.expand() { taffy::style::ExpandedDimension::Length(x) => Dimension::length(f(x)), _ => v } }
fn map_style(s: &mut Style, f: impl Fn(f32) -> f32 + Copy) {
    s.size = Size { width: map_dim(s.size.width, f), height: map_dim(s.size.height, f) };
    s.min_size = Size { width: map_lpa(s.min_size.width, f), height: map_lpa(s.min_size.height, f) };
    s.max_size = Size { width: map_lpa(s.max_size.width, f), height: map_lpa(s.max_size.height, f) };
    s.flex_basis = map_dim(s.flex_basis, f);
    s.gap = Size { width: map_lp(s.gap.width, f), height: map_lp(s.gap.height, f) };
    for r in [&mut s.padding, &mut s.border] { *r = Rect { left: map_lp(r.left, f), right: map_lp(r.right, f), top: map_lp(r.top, f), bottom: map_lp(r.bottom, f) }; }
    for r in [&mut s.margin, &mut s.inset] { *r = Rect { left: map_lpa(r.left, f), right: map_lpa(r.right, f), top: map_lpa(r.top, f), bottom: map_lpa(r.bottom, f) }; }
}
fn zoom_style(s: &mut Style, k: f32) { map_style(s, |x| x * k); }
/// The step every length is laid out on (logical px): a binary fraction, so lengths that are sums of
/// others (a box measured from its content, a row deciding whether its children fit) come out the same
/// whichever order they are added in, and a row as wide as its content is one line.
pub const LAYOUT_STEP: f32 = 1.0 / 64.0;
/// Put every length of a style on `LAYOUT_STEP` (the layout does this to every element).
pub fn snap_style(s: &mut Style) { map_style(s, |x| (x / LAYOUT_STEP).round() * LAYOUT_STEP); }
impl<M> El<M> {
    /// Every length in this subtree times `k`: sizes, gaps, padding, margins, insets, corner radii,
    /// borders (never thinner than a hairline), type, `shift`, a print lattice's pitch, `Around`'s
    /// outlines, gap and narrowest piece, a pose's moves and depth, a stirr's reach, a guide and a trace. A widget made for one size shown at another
    /// (the film scales its controls with the stage, so a small window does not overflow).
    pub fn zoom(self, k: f32) -> Self { self.zoom_by(k, false) }
    /// Show this label only while it is clear, checked every frame (a label at its own size,
    /// `El::own_size`, on a drawing whose scale changes): its whole box `inset` px (8 by default) inside
    /// its view (its nearest `El::view_scale` container, or the window) and at least 2 px from every label
    /// of that view already shown, taken by `rank` (the smaller first, then in the order of the view). One
    /// that is hidden comes back only with 4 px more to spare (12 inside, 6 apart), so it does not flicker.
    /// It goes and comes by opacity, linearly over 150 ms from where it is; hidden, it is at opacity 0 and
    /// takes no pointer, keeps its place in the layout and is still read by a screen reader. At most 64
    /// labels in one view (`ViewProblem::ShowClearMany`).
    pub fn show_clear(mut self, inset: f32, rank: u8) -> Self { self.show_clear = Some((inset.max(0.0), rank)); self }
    /// The stage (DESIGN.md §1.35): the live piece a page is about, drawn apart from the controls around
    /// it. While it draws within the frame it is drawn every frame like anything else. When its draws take
    /// more than 0.6 of a frame (over the last 8), it is slow: on a frame that answers input it holds the
    /// image it last drew, so the controls answer the hand in that frame, and it draws again on the first
    /// frame without new input, the first after a release, and during a long drag at least every
    /// `max(100 ms, 6 draws)` (at most 500 ms), replacing its image whole. It is no longer slow after 1 s
    /// of draws under 0.4 of a frame. Nothing marks a stage that holds. Not for controls.
    pub fn stage(mut self) -> Self { self.stage = true; self }
    /// It changes with a stage's piece and can be viewed alone (`Ui::on_step`, `Ui::stage_view`), as a
    /// stage is, without being one: a formula or a label beside the piece that follows it. It is drawn
    /// every frame like anything else, never held and never at a stage's scale. Inside a stage it is a
    /// part of it that changes alone (a lens moving over a page that stays): a step that names the stage
    /// views and draws only its parts so marked, by their keys, and the rest of the stage is the last
    /// frame's; a part that cannot be viewed alone, and the stage is viewed whole.
    pub fn steps(mut self) -> Self { self.steps = true; self }
    /// What the handlers in this subtree send (a press, a drag, a key, the pointer over it, the wheel)
    /// changes only what is inside it and the elements that change with a stage (`El::steps`): after
    /// such a message only this subtree and those are viewed again (`Ui::stage_view`, by this element's
    /// key), in the boxes the last layout gave them, and drawn again, as a stage that steps is
    /// (`Ui::on_step`); laid out otherwise (other elements, other styles, text that measures otherwise),
    /// the whole page is viewed and laid out again. For a stage whose piece is moved by the hand (chips
    /// dragged by their offsets). A message that changes anything outside them is not shown until the
    /// page is next laid out.
    pub fn local_messages(mut self) -> Self { self.local_messages = true; self }
    /// A view of a drawing at scale `s` (a map that grows from a thumbnail to a window): everything this
    /// container holds is scaled as by `El::zoom` (positions, lengths, fills, corner radii, a drawing's
    /// dots) except the pen: the widths of lines (a drawing's strokes, a trace), of borders, and a trace's
    /// dashes and gaps stay as at 1, and own-size elements (`El::own_size`) keep their size, whether added before or after this. A dot is never
    /// less than one device pixel across, drawn whole (not fainter) at that size. The container keeps
    /// its own box. Set it from the state to animate it.
    pub fn view_scale(mut self, s: f32) -> Self {
        self.view_scale = Some(s.max(1e-3));
        self
    }
    /// `El::zoom`, the pen too unless `keep_pen` (`El::view_scale`, applied to its children as the view
    /// is laid out, so children added after it are scaled too).
    pub(crate) fn zoom_by(mut self, k: f32, keep_pen: bool) -> Self {
        if !self.props.zoom(k, keep_pen) { return self; }
        self.children = std::mem::take(&mut self.children).into_iter().map(|c| c.zoom_by(k, keep_pen)).collect();
        self
    }
}

impl Props {
    /// `El::zoom_by` of this element itself; whether what it holds is zoomed too (not at 1, nor for an
    /// own-size element).
    fn zoom(&mut self, k: f32, keep_pen: bool) -> bool {
        if (k - 1.0).abs() < 1e-4 || k <= 0.0 { return false; }
        if let Some((a, z)) = self.own_size {
            // Its own size: only where it is scales.
            let mut st = self.style.clone();
            zoom_style(&mut st, k);
            (self.style.inset, self.style.margin) = (st.inset, st.margin);
            self.shift = (self.shift.0 * k, self.shift.1 * k);
            // What it holds is part of it, at its own size too. Placed absolutely, its anchor goes where
            // the zoomed box would put it; in a row or column it takes its own room where the layout puts it.
            let placed = self.style.position == taffy::Position::Absolute;
            self.own_size = Some((a, if placed { z * k } else { z }));
            return false;
        }
        zoom_style(&mut self.style, k);
        self.radius *= k;
        if keep_pen {
            // The border keeps its width: its room in the layout as well as its stroke.
            if let Some((w, _)) = self.border { let b = taffy::LengthPercentage::length(w); self.style.border = taffy::Rect { left: b, right: b, top: b, bottom: b }; }
            self.pen *= k;
        } else if let Some((w, p)) = self.border { self.border = Some(((w * k).max(w.min(1.0)), p)); }
        self.shift = (self.shift.0 * k, self.shift.1 * k);
        if let Content::Text { size, line, weight, print, around, pose, .. } = &mut self.content {
            *size *= k; *line *= k; *weight *= k; *print *= k;
            if let Some(a) = around.as_mut() {
                let a = Rc::make_mut(a);
                a.gap *= k; a.min *= k;
                for o in &mut a.obstacles { *o = o.scaled(k); }
            }
            // A pose answers in the element's own px: asked where a glyph is in the zoomed box, it is
            // told where that is at 1, and its moves and depth come back times `k`.
            if let Some(f) = pose.take() {
                *pose = Some(Rc::new(move |g: GlyphAt| {
                    let at = GlyphAt { x: g.x / k, y: g.y / k, w: g.w / k, h: g.h / k, span: g.span.map(|v| v / k), pen: g.pen / k, base: g.base / k, advance: g.advance / k, ..g };
                    let p = f(at);
                    Pose { dx: p.dx * k, dy: p.dy * k, z: p.z * k, ..p }
                }));
            }
        }
        if let Some(st) = &mut self.stirr { st.radius = st.radius.map(|r| r * k); }
        if let Some(g) = &mut self.guide { *g = Rc::new(g.iter().map(|&(x, y)| (x * k, y * k)).collect()); }
        if let Some(t) = &mut self.trace {
            t.points = Rc::new(t.points.iter().map(|&[x, y]| [x * k, y * k]).collect());
            if !keep_pen { t.weight *= k; t.dash *= k; t.gap *= k; }
        }
        true
    }
}

/// A page to read from plain text or Markdown (the subset `markdown.rs` reads: headings, paragraphs,
/// lists, code and tables; a link shows as its text), 600 logical px wide and keyed `wrap/…`. Put an
/// `El::effect` on it to give existing text a material and a field; `markdown::render` gives other
/// widths and links that send messages.
pub fn wrap_text<M: Clone>(src: &str) -> El<M> { crate::markdown::page(src, "wrap", 600.0) }

/// A text leaf with an explicit line height.
pub fn text_lh<M>(s: impl Into<String>, size: f32, line: f32, paint: Paint) -> El<M> {
    El::new(Style::default(), Content::Text { s: s.into(), size, line, paint, font: Font::Body, print: 0.0, caret: None, weight: 0.0, around: None, select: None, mark: None, nowrap: false, pose: None, track: 0.0 })
}
/// Part of what a cached subtree (`El::cached`, under the key `over`) drew, scaled to this element's
/// box: `from` is that part on the page (logical px: x, y, width, height). Each pixel of the cached image
/// is its coverage, in `ink`, sampled nearest: at a whole zoom a pixel there is a square of whole pixels
/// here. Nothing is drawn until the cached element has drawn once in this window.
pub fn cached_view<M>(over: impl Into<String>, from: [f32; 4], ink: Paint) -> El<M> {
    El::new(Style::default(), Content::CachedView { over: over.into(), from, ink })
}
/// A picture by id (`Renderer::set_picture`), as wide as its parent and `ratio` times as tall.
pub fn picture<M>(id: u32, ratio: f32) -> El<M> { El::new(Style { size: Size { width: percent(1.0), height: auto() }, ..Default::default() }, Content::Picture { id, ratio, fx: 0 }) }
/// Meshes with a camera and a light (DESIGN.md §1.18), as wide as its parent and 240 px tall unless
/// sized otherwise. It is decoration by default (`Response::Loosen`), like a picture.
pub fn scene3d<M>(s: crate::mesh::Scene3d) -> El<M> {
    El::new(Style { size: Size { width: percent(1.0), height: length(240.0) }, ..Default::default() }, Content::Scene3d(Rc::new(s)))
}
impl<M: Clone + 'static> El<M> {
    /// This demo with its formula caption under it (`widgets::caption`, DESIGN.md §5.2): the line is
    /// formatted from the fields that drive the demo and the controls set those fields.
    pub fn caption_params(self, key: &str, formula: impl Into<String>, params: &[crate::widgets::CaptionParam], msg: impl Fn(usize, crate::widgets::Edit) -> M + 'static) -> El<M> {
        col().key(format!("{key}/with")).gap(10.0).min_width(0.0).child(self).child(crate::widgets::caption(key, formula, params, msg))
    }
}
/// A drawing (DESIGN.md §1.26): as wide as its parent (unless sized), as tall as the drawing's
/// proportions make it; its marks are shapes with true outlines. Pointer handlers on it get points in
/// the drawing's space.
pub fn canvas<M>(d: crate::draw::Drawing) -> El<M> { El::new(Style { size: Size { width: percent(1.0), height: auto() }, ..Default::default() }, Content::Canvas(Rc::new(d))) }
/// The picture `c.over` beneath it as character marks (`Cells`), `cols` × `rows` cells: the box is
/// exactly that many cells at the pitch. Place it over the picture (`absolute`) and move it freely.
pub fn cells<M>(c: Cells, cols: u32, rows: u32) -> El<M> {
    let (w, h) = (cols as f32 * c.pitch[0], rows as f32 * c.pitch[1]);
    El::new(Style { size: Size { width: length(w), height: length(h) }, ..Default::default() }, Content::Cells(Rc::new(c)))
}
/// One mesh at the origin, with the default camera and light (`mesh::Scene3d::single`).
pub fn mesh<M>(id: crate::mesh::MeshId) -> El<M> { scene3d(crate::mesh::Scene3d::single(id)) }
/// Every child over the one before, each starting at the top left of the same box: the box is the
/// parent's (fill it with `fill_width` / `fill_height`, or size it) and does not grow with the children,
/// so a panel taller than it hangs out of it (clip the box). For sheets and floating boxes over a page;
/// a child sized `fill_width().fill_height()` covers the box. Each child is placed absolutely in it, at
/// the top left unless it is placed already.
pub fn layers<M>(children: impl IntoIterator<Item = El<M>>) -> El<M> {
    let mut s = El::new(Style::default(), Content::None);
    for mut c in children {
        if c.style.position != taffy::Position::Absolute {
            let st = &mut c.style;
            st.position = taffy::Position::Absolute;
            if st.inset.left.is_auto() && st.inset.right.is_auto() { st.inset.left = length(0.0); }
            if st.inset.top.is_auto() && st.inset.bottom.is_auto() { st.inset.top = length(0.0); }
        }
        s.children.push(c);
    }
    s
}
/// Every child in the same place, centred in the stack: for things that replace one another where
/// they stand, like the candidates of a morph. The stack is as wide as its widest child and as tall as
/// its tallest (or its own size, when it is sized); a child placed absolutely is placed as anywhere else.
pub fn stack<M>(children: impl IntoIterator<Item = El<M>>) -> El<M> {
    let mut s = El::new(Style { justify_content: JustifyContent::CENTER, align_items: AlignItems::CENTER, ..Default::default() }, Content::None);
    s.props.stack = true;
    s.children.extend(children);
    s
}
