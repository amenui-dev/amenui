//! Runtime: state, view, layout, hit-testing, focus, scrolling and instance birth times. No GPU here,
//! so all of it can be tested headless.
#![warn(missing_docs)]
use crate::el::{Content, Cursor, Drag, El, Font, Key, Modal, Mods, OnDrag, OnKey, OnText, Paint, Phase, Response, Role, Scope, TextInput};
use crate::morph::{self, Cmd, Ease, FlowSpec, MorphSpec, Particle};
use crate::scene::{EVERYWHERE, Field, GLYPH, Instance, MESH, PICTURE, SHAPE, Scene};
use crate::gaze;
use crate::style::{Gaze, GazeMode, Look, gaze as kinds};
use crate::text::{Face, MONO, Nav, Para, Text};
use crate::fasthash::{HashMap, HashSet};
use std::rc::Rc;
use crate::fasthash::FastHasher as DefaultHasher;
use std::hash::{Hash, Hasher};
use taffy::prelude::*;

/// Logical px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}
impl Rect {
    /// A rectangle from its left, top, width and height.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self { Self { x, y, w, h } }
    /// Its right edge.
    pub fn right(&self) -> f32 { self.x + self.w }
    /// Its bottom edge.
    pub fn bottom(&self) -> f32 { self.y + self.h }
    /// Its centre.
    pub fn center(&self) -> (f32, f32) { (self.x + self.w * 0.5, self.y + self.h * 0.5) }
    /// Whether the point is inside (left and top edges in, right and bottom out).
    pub fn contains(&self, x: f32, y: f32) -> bool { x >= self.x && x < self.right() && y >= self.y && y < self.bottom() }
    /// Where it overlaps `o` (empty, at the nearest corner, when they do not).
    pub fn intersect(&self, o: &Rect) -> Rect {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }
    /// Moved in by `l`, `t`, `r`, `b` from each side (never below zero size).
    pub fn inset(&self, l: f32, t: f32, r: f32, b: f32) -> Rect { Rect::new(self.x + l, self.y + t, (self.w - l - r).max(0.0), (self.h - t - b).max(0.0)) }
}

/// One laid-out element. Kept in paint order (parents before children).
pub struct Node<M> {
    /// Its key (the view's, or one derived from its path).
    pub key: String,
    /// Its parent's place in `Ui::nodes`.
    pub parent: Option<usize>,
    /// Border box, absolute, after scrolling.
    pub rect: Rect,
    /// Where this node is visible (its scroll ancestors' insides).
    pub clip: Rect,
    /// Content box: rect minus border and padding.
    pub inner: Rect,
    /// Scrolls its children.
    pub scroll: bool,
    /// Scrolls along x rather than y.
    pub horizontal: bool,
    /// Can take focus.
    pub focusable: bool,
    /// `El::tab_order`.
    pub tab_order: i32,
    /// `El::skip_to`.
    pub skip_to: Option<String>,
    /// `El::focus_shown`.
    pub focus_shown: bool,
    /// Its place among its siblings as written. It is also its place in the tree except in a preview,
    /// where what is shown only with focus is drawn after its siblings; Tab follows the written order.
    pub written: usize,
    /// `El::preview`: it clips a pixel inside its border, so what is docked to its edges in it does not
    /// draw a second line along the border.
    pub preview: bool,
    /// Takes no input (it or an ancestor was marked disabled) and is drawn faint.
    pub disabled: bool,
    /// For scroll nodes: length of what they hold along their axis, so offsets can be clamped.
    pub extent: f32,
    /// What it shows.
    pub content: Content,
    fill: Option<Paint>,
    fill_on: crate::el::PaintOn,
    border: Option<(f32, Paint)>,
    radius: f32,
    on_press: Option<M>,
    on_key: Option<OnKey<M>>,
    on_drag: Option<OnDrag<M>>,
    on_blur: Option<M>,
    loose: f32,
    wipe: Option<f32>,
    bar: bool,
    reveal: bool,
    commit: bool,
    modal: Option<Modal<M>>,
    /// Laid out but not drawn (it or an ancestor is hidden until morphed); takes no input.
    pub hidden: bool,
    /// Moved after layout (`El::shift`).
    shift: (f32, f32),
    /// Kept at its own size when zoomed (`El::own_size`): its anchor and the zoom.
    own_size: Option<([f32; 2], f32)>,
    /// How much wider than its scale its drawing's strokes are drawn (`El::view_scale`; 1: as scaled).
    pen: f32,
    /// The look's grain is drawn over it (`El::grain`).
    grain: bool,
    /// The look's bloom is drawn over it (`El::bloom`).
    bloom: bool,
    /// Shown only while clear (`El::show_clear`): inset and rank.
    show_clear: Option<(f32, u8)>,
    /// A view scale's container (`El::view_scale`).
    view_scale: Option<f32>,
    /// A stage (`El::stage`): where it was in the view, so it can be viewed again alone (`Ui::stage_view`).
    stage: Option<Box<StageCtx>>,
    /// Its opacity and its whole opacity as the view gave them, before `El::show_clear`.
    op0: (f32, f32),
    /// At its own size and in its parent's flow (not placed absolutely), so it takes room there.
    own_in_flow: bool,
    shed: Option<crate::motes::Shed>,
    flow_field: Option<crate::motes::FlowField>,
    stirr: Option<crate::el::Stirr>,
    /// A long path it draws (`El::trace`), in its own box.
    pub trace: Option<crate::strand::Trace>,
    /// The nearest flow id at or above this node, and the node that carries it.
    flow: Option<(Rc<str>, usize)>,
    /// The material override of this subtree as the instance code (0 = none, see `style::override_code`).
    pub mat: f32,
    /// Looseness held for the whole subtree (inherited).
    pub loosen: f32,
    /// An explicit callout for the annotation layer.
    pub annotate: Option<String>,
    /// This subtree is what the annotation layer looks at (`El::gaze_scope`).
    pub scope: bool,
    anchor: Option<String>,
    pub(crate) clips: bool,
    calm: bool,
    map_life: Option<(f32, f32)>,
    on_text: Option<OnText<M>>,
    /// A multi-line text field.
    pub multiline: bool,
    /// Text that can be selected (read-only).
    pub selectable: bool,
    /// The pointer's shape here.
    pub cursor: Option<Cursor>,
    /// A virtual list: its row count, row height and selected row.
    pub virt: Option<(usize, f32, Option<usize>)>,
    /// What it is to assistive technology, as the view said.
    pub access: Option<crate::el::Access>,
    /// How it answers fronts, the lens and pulls (DESIGN.md §1.15).
    pub respond: Response,
    /// Text to read or a control: field effects loosen it at most `Look::legible_cap`.
    pub legible: bool,
    /// What a press in it sends a front to (`None`: a control's own subtree, nothing for plain ground).
    pub ripple: Option<Scope>,
    /// Fronts from inside with `Scope::Container(None)` stop at its box.
    pub ripple_scope: bool,
    /// A change of its string recomposes (DESIGN.md §1.16).
    pub recompose: bool,
    /// It asked to recompose itself (`.recompose(true)` on the text): it recomposes even across a page
    /// change with a flow, where keyed text is otherwise replaced (a title that changes word to word).
    pub recompose_always: bool,
    /// Not framed by the marks on hover (`El::unframed`).
    pub unframed: bool,
    /// Its key was written by the view (not derived from its path).
    pub keyed: bool,
    /// A `scene3d` element's pointer handler, and a wheel handler.
    on_pick: Option<crate::el::OnPick<M>>,
    on_wheel: Option<crate::el::OnWheel<M>>,
    on_move: Option<crate::el::OnMove<M>>,
    guide: Option<Rc<Vec<(f32, f32)>>>,
    on_visible: Option<crate::el::OnVisible<M>>,
    on_outline: Option<crate::el::Outline<M>>,
    on_pens: Option<crate::el::OnPens<M>>,
    decode: Option<crate::decode::Decode>,
    /// Drawn as a skeleton (`El::skeleton`, inherited).
    pub skeleton: bool,
    /// Posed glyphs on a form (`El::form`).
    pub form: Option<crate::form::FormFx>,
    /// Its own turn (`El::turn`).
    pub turn: f32,
    /// The turn of its subtree as drawn, about each turned ancestor's centre (logical px; `hit::Affine`).
    pub xf: Option<crate::hit::Affine>,
    /// How clips take its text away.
    pub reveal_by: crate::el::Reveal,
    /// It moves on purpose (`El::ongoing`, inherited).
    pub ongoing: bool,
    /// The pointer passes through it (`El::pass`).
    pub pass: bool,
    /// Its own opacity (`El::opacity`).
    pub opacity: f32,
    /// Its opacity with its ancestors' (0: not drawn, takes no pointer).
    pub opacity_all: f32,
    /// Commits within it stay in its box (`El::commit_scope`).
    pub commit_scope: bool,
    /// Its subtree is zoomed to fit its box (`El::fit`).
    pub fit: Option<crate::el::Fit>,
    /// How many lines a text element shows at most (`El::clamp`; 0: all).
    pub clamp: u16,
    /// What a clamped text element shows when it is cut: its start and the ellipsis.
    pub shown: Option<String>,
    /// What its subtree draws stays (`El::plate`).
    pub plate: Option<crate::el::Plate>,
    /// Its subtree is drawn once and kept until it changes (`El::cached`).
    pub cached: bool,
    /// Its own clip's corner radius (`El::clip_round`, logical px).
    clip_radius: f32,
    /// The corner radius of the clip it is drawn in (logical px; 0: square).
    pub clip_r: f32,
    /// The box layout gave it, relative to its parent (scrolling does not change it).
    local: taffy::Layout,
    /// Where taffy put it in its parent before rounding to whole px (for `El::own_size`).
    exact: (f32, f32),
    tid: NodeId,
}
impl<M> Node<M> {
    /// The nearest flow id at or above this node (`El::flow`).
    pub fn flow(&self) -> Option<&str> { self.flow.as_ref().map(|f| &*f.0) }
    /// Its text, if it is a text element.
    pub fn text(&self) -> Option<&str> { if let Content::Text { s, .. } = &self.content { Some(s) } else { None } }
    /// It has a press handler.
    pub fn pressable(&self) -> bool { self.on_press.is_some() }
    /// What the annotation layer calls this element.
    pub fn kind(&self) -> &'static str {
        match self.content {
            Content::Text { .. } => "text",
            Content::Picture { .. } => "image",
            Content::Scene3d(_) => "mesh",
            Content::Canvas(_) => "drawing",
            Content::Cells(_) | Content::CachedView { .. } => "image",
            Content::None if self.focusable || self.on_press.is_some() || self.on_drag.is_some() => "control",
            Content::None if self.fill.is_some() || self.border.is_some() => "shape",
            Content::None => "box",
        }
    }
    /// Something the pointer can act on: hover marks frame it.
    fn active(&self) -> bool { self.focusable || self.on_press.is_some() || self.on_drag.is_some() }
    /// A control that a front pushes as one piece with what is in it: active, and not a surface (an
    /// element that follows the pointer, `El::on_point` / `El::on_move`, or one larger than a quarter of
    /// the window: a stage or a page that takes keys holds many controls, each its own piece).
    fn piece(&self, window: (f32, f32)) -> bool {
        self.active() && self.on_move.is_none() && self.rect.w * self.rect.h <= 0.25 * window.0 * window.1
    }
    /// A text field.
    pub fn editable(&self) -> bool { self.on_text.is_some() }
    /// How its text is laid out, when it is text: the same paragraph that is drawn.
    pub(crate) fn para(&self) -> Option<Para<'_>> {
        let Content::Text { s, size, line, font, print, around, nowrap, track, .. } = &self.content else { return None };
        let width = if *nowrap { f32::INFINITY } else { self.inner.w + if around.is_some() { 0.0 } else { 1.0 } };
        let s = self.shown.as_deref().unwrap_or(s);
        Some(Para { s, face: font.resolve([0; 3]), size: *size, line: *line, width, print: *print, around: around.as_deref(), track: *track })
    }
    pub(crate) fn origin(&self) -> (f32, f32) { (self.inner.x, self.inner.y) }
}

/// A drag in progress: the handler and where the press was, captured at the press.
struct Capture<M> { key: String, handler: OnDrag<M>, start: (f32, f32), rect: Rect }

/// The frame as it was when a commit (theme, material) was pressed, and where: the renderer shows it
/// beyond a front travelling out from `at` (device px) from time `t`.
pub struct Split {
    /// Unique to this commit.
    pub id: u32,
    /// The old frame's scene.
    pub scene: Scene,
    /// The old frame's field.
    pub field: Field,
    /// Where the press was, device px.
    pub at: [f32; 2],
    /// When, seconds.
    pub t: f32,
    /// The look it was drawn in.
    pub look: Look,
    /// The box the commit is confined to (`El::commit_scope`), device px x0, y0, x1, y1: outside it the
    /// frame stays as it was.
    pub scope: Option<[f32; 4]>,
}
impl Split {
    /// How much of the look's transition time a commit takes (DESIGN.md §1.1): all of it on the whole
    /// page; within a scope, the scope's diagonal over the page's (`w`, `h` device px), from half to all.
    pub fn span(&self, w: f32, h: f32) -> f32 {
        match self.scope { Some(b) => ((b[2] - b[0]).hypot(b[3] - b[1]) / w.hypot(h).max(1.0)).clamp(0.5, 1.0), None => 1.0 }
    }
}

/// One laid-out line of a text element and its ink (`Ui::line_inks`), logical px, window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineInk {
    /// The line's box, as `Ui::line_boxes` gives it.
    pub line: Rect,
    /// The box round its glyphs' true outlines at rest (what they cover more than half), out to whole
    /// device pixels; loosened marks are not in it.
    pub ink: Rect,
    /// Its baseline.
    pub baseline: f32,
    /// Where its face's capitals reach up to (the baseline less the cap height).
    pub cap_top: f32,
    /// Where its face's lowercase reaches up to (the baseline less the x height).
    pub x_top: f32,
}

/// One glyph of a text as the last paint drew it, after its pose (`Ui::glyphs_drawn`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphDrawn {
    /// Its number in the text (`GlyphAt::i`).
    pub i: usize,
    /// The box its raster covers as drawn (moved, turned and scaled by its pose; for a turned glyph the
    /// box round it), logical px, window coordinates: no pixel of its coverage is outside it.
    pub rect: Rect,
    /// Its opacity as drawn (the text's paint times its pose's).
    pub alpha: f32,
}

/// The four accent corners that frame the hovered or focused element. They are on their target in the
/// frame the target changes (the pointer's frame); the only easing is `MARKS_SETTLE`, a cosmetic close
/// of a few pixels and the fade in, never a glide behind the pointer.
#[derive(Default)]
struct Marks { at: Option<Rect>, clip: Rect, alpha: f32, since: f32, key: Option<String>, fade: f32, within: Option<Rect> }
/// How long the marks take to close onto a new target (s): under 80 ms, so they never trail.
pub const MARKS_SETTLE: f32 = 0.07;
/// How far out (logical px) the marks start when they close onto a new target.
const MARKS_OPEN: f32 = 3.0;

/// Which instance of a flow subtree an instance is: glyphs and other instances are counted apart, and
/// a glyph only matches the same glyph.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Tag { flow: Rc<str>, glyph: bool, n: u32, gid: u32 }

/// A shared part on its way from its old rect (device px) to the one layout gives it now.
struct FlowRec {
    from: [f32; 4], clip: [f32; 4], start: f32, dur: f32, loose: f32,
    /// A glyph flying from the last frame (`Ui::morph_from_last`): the old glyph it starts as. Its box
    /// and colour go to the new one's; it is drawn as the old glyph for the first half of its way, as
    /// the new one after, each at the size of the box it has then.
    src: Option<Instance>,
    /// Comes in over these times (s) where it is, its alpha from 0.
    fade_in: Option<[f32; 2]>,
    /// The picture of a flow subtree that grows (`Ui::grows`): drawn in the box the grow has then, and
    /// only inside the part of it that shows.
    grow: Option<usize>,
    /// An instance with the same key on both pages, carried by a growing picture (`Keyed`).
    keyed: Option<Keyed>,
}
/// A flow on its way, for what the new page draws only after it began (a note shown once what it
/// follows is in view, a drawing's new marks): it comes in as what was new when the flow began does,
/// from the same start. `grows`: the flows of the subtrees that grow; `seen`: the identities the page
/// has had since it began; `end`: when what comes in has come in.
struct FlowJoin { start: f32, wait: f32, fade: f32, grows: HashSet<Rc<str>>, seen: HashSet<u64>, end: f32 }
/// An instance of an element keyed the same on both pages of a flow that grows (`Cmd::Flow`): it goes
/// from where it was to where it is with the picture's transform, its point `pin` (its centre, or an
/// own-size label's anchor) carried by the picture and brought onto its new place on the same cubic;
/// its size goes from the old to the new on that cubic too (a stroke's width does not), its colour
/// linearly. `grow`: the picture it goes with; `old`: the instance as it was drawn.
struct Keyed { grow: usize, old: Instance, pin: [f32; 2], keep_h: bool }
/// An instance of the old page that has no place on the new one: it loosens (`loosen`) and fades; one
/// of a subtree whose picture grows (`ride`) rides with the picture at its own size instead.
/// `anchor`: for one that rides on a growing picture, the box of the part of its element it moves with
/// (a word, one box of lines), so that part keeps its own shape.
/// `pin`: the point of that box (0–1 across, down) the picture carries: its centre, or a label's own
/// anchor (`El::own_size`).
/// `scaled`: it is a drawing's mark, or was drawn under a view scale (`El::view_scale`) and not at its own
/// size, so it is geometry and goes with the picture's transform, its pen kept, not at its own size.
struct Ghost { inst: Instance, start: f32, dur: f32, loosen: bool, ride: Option<usize>, anchor: [f32; 4], pin: [f32; 2], scaled: bool }
/// A look's ground and second ground.
type Grounds = ([f32; 3], [f32; 3]);
/// A flow subtree's picture on its way (`Cmd::Flow`): its box (x, y, w, h) and the part of it that shows
/// (x0, y0, x1, y1), old and new, device px. In between both go in a straight line, whole pixels; what
/// else of the subtree is old rides with the picture at its own size and fades, what is new waits until
/// it lands.
struct Grow { p0: [f32; 4], p1: [f32; 4], r0: [f32; 4], r1: [f32; 4], start: f32, dur: f32 }
impl Grow {
    fn ease(&self, now: f32) -> f32 { Ease::InOutCubic.at(((now - self.start) / self.dur.max(1e-4)).clamp(0.0, 1.0)) }
    /// The picture's box at `e` of the way, its edges on whole pixels.
    fn picture(&self, e: f32) -> [f32; 4] {
        let p: [f32; 4] = std::array::from_fn(|k| self.p0[k] + (self.p1[k] - self.p0[k]) * e);
        let (x0, y0, x1, y1) = (p[0].round(), p[1].round(), (p[0] + p[2]).round(), (p[1] + p[3]).round());
        [x0, y0, x1 - x0, y1 - y0]
    }
    /// The part that shows at `e` of the way, on whole pixels.
    fn shows(&self, e: f32) -> [f32; 4] { std::array::from_fn(|k| (self.r0[k] + (self.r1[k] - self.r0[k]) * e).round()) }
    /// Where a rect of the old subtree is at `e` of the way: its centre where the picture takes it, its
    /// size its own.
    fn ride(&self, r: [f32; 4], e: f32) -> [f32; 4] {
        let p = self.picture(e);
        let (cx, cy) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        let (u, v) = ((cx - self.p0[0]) / self.p0[2].max(1e-3), (cy - self.p0[1]) / self.p0[3].max(1e-3));
        let (nx, ny) = (p[0] + u * p[2], p[1] + v * p[3]);
        [r[0] + nx - cx, r[1] + ny - cy, r[2], r[3]]
    }
    /// Where a rect of the old subtree's scaled geometry is at `e` of the way: taken by the picture's
    /// transform; a line (`keep_h`) keeps its width across and its round ends.
    fn carry(&self, r: [f32; 4], e: f32, keep_h: bool) -> [f32; 4] {
        let p = self.picture(e);
        let (kx, ky) = (p[2] / self.p0[2].max(1e-3), p[3] / self.p0[3].max(1e-3));
        let at = |x: f32, y: f32| [p[0] + (x - self.p0[0]) * kx, p[1] + (y - self.p0[1]) * ky];
        if keep_h {
            let c = at(r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
            let w = (r[2] - r[3]) * kx + r[3];
            return [c[0] - w * 0.5, c[1] - r[3] * 0.5, w, r[3]];
        }
        let a = at(r[0], r[1]);
        [a[0], a[1], r[2] * kx, r[3] * ky]
    }
}
/// A page instance as it was drawn, kept across a relayout for `Cmd::Flow`.
/// `key`: its element's key when the element is keyed, `slot` which of that element's instances it is
/// (glyph or not, the count, the glyph), `own` whether the element keeps its own size, `scaled` whether
/// it is geometry: a drawing's mark, or under a view scale (`El::view_scale`) not at its own size.
struct Old { id: u64, node: usize, pin: [f32; 2], tag: Option<Tag>, inst: Instance, key: Option<Rc<str>>, slot: (bool, u32, u32), own: bool, scaled: bool }

/// A text's glyphs as last painted, kept to see what changed in the next view (DESIGN.md §1.16).
/// `changed` is when its string last changed (seconds), to tell a readout that changes every frame.
struct TextRec { s: String, face: u8, size: f32, line: f32, width: f32, print: f32, track: f32, origin: (f32, f32), glyphs: Vec<(u64, Instance)>, changed: f32 }
/// One tile of a recomposition: its run (a morph tile, `morph::Particle`) and the pieces of the old and
/// the new glyphs it shows, as instances placed relative to its source and its target cell.
struct RTile { p: Particle, src: Vec<Instance>, dst: Vec<Instance> }
/// The changed glyphs of one text recomposing: the tiles, when they started and land, and the new glyphs
/// they stand in for (hidden until then).
struct Recomp { tiles: Vec<RTile>, start: f32, end: f32, hide: HashSet<u64>, keep: Vec<Instance>, keep_until: f32 }
/// An unchanged glyph sliding to where the new string puts it.
struct Slide { from: [f32; 4], clip: [f32; 4], start: f32, dur: f32 }
/// A recomposition found while painting, worked out after the scene is built.
struct RecJob { node: usize, rec: TextRec }
/// The lists a text element's glyphs go through on their way into the scene, kept between paints so a
/// paint of a page of text (every scroll step) does not allocate them for each element again.
#[derive(Default)]
struct TextScratch { glyphs: Vec<(Instance, u32)>, swept: Vec<f32>, pens: Vec<[f32; 3]>, posed: Vec<crate::form::Posed>, out: Vec<(Instance, u32)>, chosen: Vec<bool> }
/// What a cached subtree (`El::cached`) last drew besides its instances, kept so a paint in which nothing
/// it draws from changed (`sig`, `Ui::keep_sig`) passes it by: its identities' births, its texts' births and
/// recomposition records, its texts' laid glyphs (by node, from its root), and its latest birth and
/// whether something in it moves loose by itself.
struct PlateKeep { sig: u64, timing: Timing, births: Vec<(u64, f32)>, text_births: Vec<(u64, f32)>, recs: Vec<String>, laid: Vec<crate::hit::Laid>, latest: f32, loose: bool }
/// A cached subtree being painted: its root, plate slot and first instance; what it is drawn from (`None`:
/// something it reads cannot be told the same next time); how long the lists it may add to were.
struct KeepOpen { root: usize, slot: u8, start: usize, sig: Option<u64>, lens: [usize; 7] }
/// One node of a cached subtree into its signature (`Ui::keep_sig`): what it is drawn from.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sig_node(h: &mut crate::fasthash::FastHasher, key: &str, nums: [f32; 15], flags: (bool, bool, bool, bool, crate::el::Reveal, u16, &Option<String>), fill: Option<Paint>, border: Option<(f32, Paint)>, respond: Response, content: &Content) {
    use std::hash::Hash;
    let paint = |p: Option<Paint>, h: &mut crate::fasthash::FastHasher| p.map(|p| ([p.tone, p.accent, p.alpha].map(f32::to_bits), p.hue)).hash(h);
    key.hash(h);
    nums.map(f32::to_bits).hash(h);
    flags.hash(h);
    paint(fill, h);
    paint(border.map(|b| b.1), h);
    (border.map(|b| b.0.to_bits()), respond).hash(h);
    if let Content::Text { s, size, line, paint: tp, font, print, weight, nowrap, track, .. } = content {
        s.hash(h);
        [*size, *line, *print, *weight, *track].map(f32::to_bits).hash(h);
        nowrap.hash(h);
        paint(Some(*tp), h);
        font.hash(h);
    }
}
impl PlateKeep {
    /// What the subtree of `root` (the nodes `root..end`, keyed `keys`) drew from instance `start` on.
    #[allow(clippy::too_many_arguments)]
    fn take(sig: u64, timing: Timing, insts: &[Instance], owners: &[(usize, u64)], births: &HashMap<u64, f32>, text_births: &HashMap<u64, f32>, jobs: &[(usize, &str)], laid: &[crate::hit::Laid], root: usize, keys: &[&str]) -> Self {
        use std::hash::{Hash, Hasher};
        let mut ids: Vec<(u64, f32)> = owners.iter().filter_map(|&(_, id)| births.get(&id).map(|&b| (id, b))).collect();
        ids.dedup_by_key(|x| x.0);
        let last = root + keys.len().saturating_sub(1);
        let tb = keys.iter().filter_map(|key| { let mut h = crate::fasthash::FastHasher::new(); key.hash(&mut h); let h = h.finish(); text_births.get(&h).map(|&b| (h, b)) }).collect();
        let recs = jobs.iter().filter(|j| j.0 >= root && j.0 <= last).map(|j| j.1.to_string()).collect();
        let laid = laid.iter().filter(|l| l.0 >= root && l.0 <= last).map(|l| (l.0 - root, l.1.clone(), l.2.clone())).collect();
        let latest = insts.iter().map(|i| i.extra[3]).fold(f32::NEG_INFINITY, f32::max);
        let loose = insts.iter().any(|i| { let m = if i.look[0] >= 127.5 { (i.look[0].round() as u32 % 16) as u8 } else { timing.material }; i.extra[2] > 0.0 && crate::style::Material::from_id(m).time_varying() });
        PlateKeep { sig, timing, births: ids, text_births: tb, recs, laid, latest, loose }
    }
    /// Lay on what it drew besides its instances, for the subtree now at `root`.
    fn lay(&self, root: usize, births: &mut HashMap<u64, f32>, text_births: &mut HashMap<u64, f32>, seen_recs: &mut HashSet<String>, laid: &mut Vec<crate::hit::Laid>) {
        for &(id, b) in &self.births { births.insert(id, b); }
        for &(h, b) in &self.text_births { text_births.insert(h, b); }
        for r in &self.recs { seen_recs.insert(r.clone()); }
        for l in &self.laid { laid.push((l.0 + root, l.1.clone(), l.2.clone())); }
    }
}

const NEVER: f32 = f32::INFINITY;

/// Run ids are unique in the process, so a renderer shared between runtimes never takes one morph's
/// pictures for another's.
fn next_run() -> u32 {
    static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A morph in progress (DESIGN.md §1.10). Times are seconds on the runtime's clock; a ramp is
/// looseness `k0` at `t0` to `k1` at `t1`. The renderer reads it: the instances of both elements to
/// draw their pictures once, and the tiles to fly while `flight` is on.
pub struct MorphRun {
    /// Changes when there are new elements to draw (a new morph); `upload` when the tiles change.
    pub id: u32,
    /// Changes when the tiles change.
    pub upload: u32,
    /// The element that loosens and hands over.
    pub from: String,
    /// The element the tiles become.
    pub to: String,
    /// How the morph runs.
    pub spec: MorphSpec,
    /// The source is drawn (by the morph) until this time, with this looseness.
    pub src_until: f32,
    /// The source's looseness ramp (`ramp`).
    pub src_ramp: [f32; 4],
    /// While the tiles fly: start and end.
    pub flight: Option<[f32; 2]>,
    /// The target is drawn from this time on, with this looseness.
    pub dst_from: f32,
    /// The target's looseness ramp.
    pub dst_ramp: [f32; 4],
    /// When the morph is over.
    pub end: f32,
    /// The tiles, on the CPU path.
    pub particles: Vec<Particle>,
    /// On the GPU path: the grid the GPU simulates (and `particles` is empty).
    pub grid: Option<morph::Grid>,
    /// Tile size, device px, and the instances of the source and the target (looseness 1).
    pub tile: f32,
    /// The source's instances.
    pub src: Vec<Instance>,
    /// The target's instances.
    pub dst: Vec<Instance>,
    /// 3D elements on either side: their tiles start (or land) on points of the mesh surface, projected
    /// again every frame and hidden while they are behind the mesh. `tags` says, per tile, which point
    /// of which emitter its source and target are (`emitter << 24 | point`, or `morph::NO_TAG`).
    pub emit: Vec<Emitter>,
    /// Per tile, its source's and target's emitter point.
    pub tags: Vec<[u32; 2]>,
    /// Held at this point of the flight (`Cmd::Scrub`), if it is.
    pub scrub: Option<f32>,
}
/// A `scene3d` element that emits (or takes in) a morph's tiles: its key and the surface points.
pub struct Emitter {
    /// The element's key.
    pub key: String,
    /// The points on its mesh's surface.
    pub points: Vec<crate::mesh::SurfacePoint>,
}
impl MorphRun {
    /// Seconds into the flight, while the tiles are flying.
    pub fn clock(&self, now: f32) -> Option<f32> {
        if let (Some(u), Some(f)) = (self.scrub, self.flight) { return Some((u.clamp(0.0, 1.0) * (f[1] - f[0])).min(f[1] - f[0] - 1e-4).max(0.0)); }
        self.flight.filter(|f| now >= f[0] && now < f[1]).map(|f| now - f[0])
    }
    fn phase(&self, now: f32) -> u8 { (now >= self.src_until) as u8 | (self.clock(now).is_some() as u8) << 1 | ((now >= self.dst_from) as u8) << 2 }
}
/// A looseness ramp `[k0, t0, k1, t1]` at time `t`: `k0` until `t0`, `k1` from `t1`, straight between.
pub fn ramp(r: [f32; 4], t: f32) -> f32 {
    if r[3] <= r[1] { return if t >= r[3] { r[2] } else { r[0] }; }
    r[0] + (r[2] - r[0]) * ((t - r[1]) / (r[3] - r[1])).clamp(0.0, 1.0)
}
/// The runtime: the application's state, its view and update, the laid-out tree, focus, input, the
/// field and everything the renderer needs to draw a frame (DESIGN.md §1).
pub struct Ui<S, M> {
    /// The application's state (the view reads it, update changes it).
    pub state: S,
    view: fn(&S) -> El<M>,
    update: fn(&mut S, M),
    text: Text,
    /// Logical size and device px per logical px.
    size: (f32, f32),
    scale: f32,
    /// The beat clock (DESIGN.md §1.32), s: what moves on its own runs on it; the runtime's own while the
    /// look has no beat.
    clock: f32,
    /// The last frame drawn was taken while something moved: one more frame is owed, so the screen
    /// ends on the settled page even when the motion ends between two frames.
    owed: bool,
    /// The last frame drawn shaded the scroll areas softened (`Ui::soft_drawn`): a sharp one is owed
    /// once scrolling stops.
    soft_shown: bool,
    /// Frames a second for what moves by itself (`Ui::ambient_fps`), when the last input came and when
    /// the last frame was drawn (the runtime's own clock, s).
    ambient: f32,
    input_t: f32,
    frame_t: f32,
    /// The runtime's own clock (what `tick` was given), and the gap between the last two ticks (the
    /// display's refresh, for a host that ticks on each one).
    wall: f32,
    tick_gap: f32,
    /// Where the beat clock is against the runtime's.
    bclock: crate::beat::Clock,
    /// Commands sent from outside the application (`Ui::command`), taken with the next update's.
    sent: Vec<Cmd>,
    /// The look's numbers set by `Cmd::Set`, in order, over the application's look.
    sets: Vec<(String, f32)>,
    nodes: Vec<Node<M>>,
    scene: Scene,
    /// Which node and identity each instance of the page (not the marks) belongs to.
    owners: Vec<(usize, u64)>,
    page_len: usize,
    field: Field,
    scroll: HashMap<String, f32>,
    births: HashMap<u64, f32>,
    /// When `condense_all` began loosening the page toward each element's cap (DESIGN.md §1.23), while
    /// it does.
    ramp: Option<f32>,
    /// Each text node's birth (by key): a glyph new to a text that was already there (its lines broke
    /// differently) takes the text's birth, so a paragraph never shows some glyphs condensing among
    /// crisp neighbours. A text is crisp or it condenses as a whole.
    text_births: HashMap<u64, f32>,
    hover: Option<String>,
    focus: Option<String>,
    /// The element that had the focus when it was cleared (`clear_focus`): Tab goes on from it.
    after_focus: Option<String>,
    /// Boxes marked `El::fit` that could not hold their subtree at the least scale: key, the scale it
    /// would need, the least.
    fit_short: Vec<(String, f32, f32)>,
    /// Where the look's grain is drawn this frame (device px, `El::grain`): `None` over the whole frame.
    grain_at: Option<[f32; 4]>,
    /// Where the look's bloom is drawn this frame (device px, `El::bloom`): `None` over the whole frame.
    bloom_at: Option<[f32; 4]>,
    /// Each label shown only while clear (`El::show_clear`), by key: its opacity, and when that was
    /// worked out if it is to be shown, or `−1 −` that time if it is to be hidden.
    clears: HashMap<String, (f32, f32)>,
    /// A label of `El::show_clear` is on its way in or out.
    clearing: bool,
    /// Own-size type keyed the same on both pages of a growing flow at another size (`ViewProblem::FlowOwnSize`).
    flow_own: Vec<String>,
    /// A flow on its way (`Cmd::Flow`): what the new page drew when it began, and how what comes on the
    /// page after it began comes in (`flow_late`).
    flow_join: Option<FlowJoin>,
    /// Views with more labels shown while clear than are checked (`ViewProblem::ShowClearMany`).
    clear_many: Vec<(String, usize)>,
    /// The scale each box marked `El::fit` gave its subtree in the last layout.
    fit_k: HashMap<String, f32>,
    /// The scales the fitted boxes were last given (`El::fit`), kept while nothing layout reads changes.
    fit_last: Option<Vec<Fitted>>,
    pressed: Option<String>,
    /// The element being dragged, if its press started a drag.
    capture: Option<Capture<M>>,
    /// The open modal layer (its node index and key), and where focus was before it opened.
    modal_at: Option<usize>,
    modal_key: Option<String>,
    return_focus: Option<String>,
    split: Option<Split>,
    splits: u32,
    /// The last input came from the keyboard, so the marks follow focus rather than the pointer.
    keyboard: bool,
    marks: Marks,
    /// Tells the state the window's logical size before each layout after a resize.
    sized: Option<fn(&mut S, f32, f32)>,
    /// Takes the commands `update` left in the state.
    commands: Option<fn(&mut S) -> Vec<Cmd>>,
    /// Instances of hidden nodes (laid out, not drawn), for sampling a morph's target.
    unseen: Vec<(usize, Instance)>,
    /// The stages (`El::stage`) by key: their last images and how they draw.
    stages: Vec<(String, crate::stage::Stage)>,
    /// Input came since the last frame (`tick`), and a press ended since then (for the stages).
    stage_input: (bool, bool),
    /// Added to every stage's draw, ms (`Ui::slow_stage`).
    stage_add_ms: f32,
    /// Where the page's faces for glyph outlines go when one comes (`Ui::on_faces`).
    faces_to: Option<fn(&mut S, crate::pair::Faces)>,
    /// Where a cached element's having a kept image or not goes when it changes (`Ui::on_kept`), what
    /// each was last told (absent: kept), and the changes the last paint found, not yet told.
    kept_to: Option<fn(&mut S, &str, bool)>,
    kept_told: HashMap<String, bool>,
    kept_flips: Vec<(String, bool)>,
    /// What the page's root resolves the roles to (its `El::fonts`), shared with the faces handed out.
    root_roles: std::rc::Rc<std::cell::Cell<[u8; 3]>>,
    /// The stages' device scale and the frames it goes by (DESIGN.md §1.35), the scale pinned by the look's
    /// quality (`Ui::stage_scale`), and the last frame it was fed.
    scaler: crate::perf::StageScale,
    stage_pin: Option<f32>,
    scaler_fed: f64,
    governed: bool,
    /// What the stages' pieces do by themselves (`Ui::on_step`), and each stage's subtree by its key
    /// (`Ui::stage_view`).
    stepped: Option<fn(&mut S, f32) -> Vec<String>>,
    stage_of: Option<StageView<S, M>>,
    /// How each stage's root was flattened, by key, and the stages stepped while layout waits for the frame.
    stale_stages: Vec<String>,
    /// Steps of slow stages that held when they came (`Ui::restage`): viewed when the stage draws next.
    held_steps: Vec<String>,
    /// While a paint draws only some stages again: their roots (the rest is the last paint's).
    restaging: Option<Vec<usize>>,
    /// The page instances as painted, before morph ramps and flows are applied; and their flow tags.
    base: Vec<Instance>,
    tags: Vec<Option<Tag>>,
    /// Per node: 1 inside the morph's source, 2 inside its target.
    roles: Vec<u8>,
    run: Option<MorphRun>,
    painted_phase: u8,
    flows: HashMap<u64, FlowRec>,
    ghosts: Vec<Ghost>,
    /// The pictures of flow subtrees on their way (`Grow`), for `flows` and `ghosts` to point at.
    grows: Vec<Grow>,
    /// The ground of the look as last ticked, and the old page's ground held while a picture grows over
    /// the page (until then the old page's ground is around it).
    ground_seen: Option<Grounds>,
    ground_hold: Option<(Grounds, f32)>,
    /// Page instances plus ghosts: where the marks start.
    body_len: usize,
    animating: bool,
    /// Page instance → where its copy drawn over the page is, while it flows.
    lifted: HashMap<usize, usize>,
    /// The annotation layer: its settings (from the state), when each annotated element first
    /// appeared, and this frame's records.
    gaze_of: Option<fn(&S) -> Gaze>,
    gaze_seen: HashMap<String, f32>,
    annots: Vec<gaze::Annot>,
    /// What the scope-wide readouts (edge notes, waveform, level bar) drew this frame.
    readout: gaze::Readout,
    /// Tells the state which element the pointer is over (when it changes), and the time on every
    /// tick; either returns whether the view changed.
    hovered: Option<fn(&mut S, Option<&str>) -> bool>,
    /// Told which element has the keyboard focus (`Ui::on_focus`).
    focused: Option<fn(&mut S, Option<&str>) -> bool>,
    /// A modal layer moved the focus during a layout: the state is told after it.
    focus_moved: bool,
    pointer_of: Option<fn(&mut S, f32, f32) -> bool>,
    /// A scripted press (`Cmd::Press`) is going on: it is not a person's input.
    scripted: bool,
    ticked: Option<fn(&mut S, f32) -> bool>,
    /// How far the renderer's warm-up is (`None`: warm), and who is told (`Ui::on_warm`).
    warm: Option<crate::render::Progress>,
    warmed: Option<fn(&mut S, Option<crate::render::Progress>) -> bool>,
    /// Keys nothing on the page used (the flag is Shift), and keys let go; either returns whether
    /// the state changed.
    unused_key: Option<fn(&mut S, Key, bool) -> bool>,
    /// Cmd / Ctrl + a character no text field took (⌘K), and scroll positions told to the state.
    shortcut: Option<fn(&mut S, char) -> bool>,
    scrolled_of: Option<ScrollHook<S>>,
    released: Option<fn(&mut S, Key) -> bool>,
    /// Told of every press, wheel turn and key from the person (before it is handled); returns whether
    /// the state changed.
    input: Option<fn(&mut S) -> bool>,
    // ── Demand-driven frames (DESIGN.md §1.11) ──
    /// Something changed since the last frame was drawn (input, a new scene, an animation step).
    dirty: bool,
    /// The look, for the timings that decide whether anything is still moving.
    look_of: Option<fn(&S) -> Look>,
    /// The locale the state is in (DESIGN.md §1.20): made the current one while the view is built.
    locale_of: Option<fn(&S) -> crate::i18n::Locale>,
    timing: Timing,
    /// When the state next wants a tick (a clock, a running demonstration), asked after every tick.
    wake_of: Option<fn(&S, f32) -> Option<f32>>,
    /// The newest birth time among drawn instances (they condense for `enter` seconds after it).
    latest_birth: f32,
    /// Until when an annotation is still drawing on.
    gaze_until: f32,
    /// `Gaze::echo`: each annotated element's earlier `INK` boxes (beat-grid time, device px), the
    /// latest last, and when the last one was taken.
    echoes: HashMap<String, Vec<(f32, [f32; 4])>>,
    echo_at: f32,
    /// The scene as it was when a `Cmd::Hold` began: what is presented until the hold ends, whatever
    /// the state does meanwhile.
    frozen: Option<Scene>,
    /// The marks moved or faded on the last tick.
    marks_moving: bool,
    /// The caret: its page instance, when it last moved (blinking restarts there), its rect, and
    /// whether it is shown in the current blink phase.
    caret: Option<(usize, f32, [f32; 4])>,
    caret_on: bool,
    /// Some drawn instance holds a static looseness under a time-varying material (which moves it).
    static_loose: bool,
    /// The material clock: time terms of materials (glitchr's hold, dottr's drift) read it.
    /// It only runs while something moves, so a page at rest is a still image.
    mclock: f32,
    /// The ground clock, and whether it runs (see `ground_clock`).
    gclock: f32,
    ground_moving: bool,
    moving: bool,
    // ── Frame times and the readout (DESIGN.md §1.12) ──
    perf: crate::perf::Perf,
    /// Nesting of timed update / layout spans (only the outermost counts), and painting (so `finish`
    /// inside `paint` is not counted twice).
    span: u32,
    painting: bool,
    /// The readout is shown; when it was last drawn (it refreshes twice a second while shown).
    overlay: u8,
    overlay_at: f32,
    /// Where the readout's box was drawn last (logical px).
    overlay_box: Option<Rect>,
    /// The readout's widest line so far (logical px) for its mode and the window's size: the plate never
    /// narrows while the numbers change.
    overlay_w: (u8, (f32, f32), f32),
    /// The fade with less motion, seconds (`Ui::reduced_fade`).
    fade_s: f32,
    /// Faces whose bytes came since the last tick (`add_face`), with what stood in for them: they
    /// change together, in one layout.
    came: Vec<(u8, crate::text::FaceDecl)>,
    /// Faces changing now: when, and what stood in for them (DESIGN.md §1.6.5).
    arriving: HashMap<u8, (f32, crate::text::FaceDecl)>,
    /// The last paint swept a face along a line on screen.
    sweeping: bool,
    /// The text nodes that last paint swept (their glyphs loosen as they change face, scribblr's too).
    swept_nodes: Vec<usize>,
    /// Faces changed: the next layout measures every text again.
    remeasure: bool,
    /// The plates (DESIGN.md §1.33): each slot's element, when its ink was last drawn and how long it
    /// lasts (plate clock); slots to empty before the next frame; plates over the limit; the plate clock
    /// (it stops while a `Hold` lasts) and when a frame was last drawn for aging.
    plate_slots: [Option<(String, f32, f32)>; crate::scene::PLATES],
    /// What each cached plate last drew (`El::cached`): a hash of its instances.
    plate_sig: [Option<u64>; crate::scene::PLATES],
    /// What each cached plate's subtree last drew from and besides its instances (`PlateKeep`).
    plate_keep: [Option<PlateKeep>; crate::scene::PLATES],
    /// Faces added so far (`Ui::add_face`): text in a face that came draws otherwise from the same nodes.
    faces_added: u32,
    plate_clear: Vec<u8>,
    plate_over: Vec<String>,
    pclock: f32,
    plate_frame: f32,
    /// Resources the host added (`add_asset`), and the names asked for that are not there.
    assets: HashMap<String, Vec<u8>>,
    assets_asked: std::cell::RefCell<std::collections::BTreeSet<String>>,
    /// The shaping time at the last frame, so the readout shows what this frame spent.
    shape_seen: f64,
    /// The readout last said the page is idle (so it does not ask for another frame).
    hud_idle: bool,
    /// Nodes the last view rebuilt and laid out (a view builds every node).
    rebuilt: usize,
    // ── Text (DESIGN.md §1.8.1) ──
    /// The text field the pointer pressed in, while the press lasts (its drag selects).
    text_capture: Option<String>,
    /// Read-only selection over selectable text: anchor and focus (node key, byte), and whether the
    /// pointer is still dragging it.
    sel: Option<((String, usize), (String, usize))>,
    sel_drag: bool,
    /// The last press: when, where, and how many presses in a row it made (a double click is 2).
    clicks: (f32, f32, f32, u8),
    /// The accessibility mirror as last sent to the host, and the focus it was told.
    a11y_sent: HashMap<String, A11yNode>,
    /// Layouts so far, and the count when the mirror was last sent; whether moves while a scroll goes on
    /// are owed to it (`Ui::a11y_diff`).
    layout_gen: u64,
    a11y_gen: u64,
    a11y_owed: bool,
    a11y_focus: Option<String>,
    // ── Incremental layout (DESIGN.md §1.13) ──
    /// The taffy tree, kept across layouts: each key's taffy node, and what its measured content was.
    tree: TaffyTree<usize>,
    tids: HashMap<String, NodeId>,
    mkeys: HashMap<NodeId, u64>,
    /// Elements a restage moved by their offsets since the tree was last laid out (`Ui::swap_stage`): where
    /// they are (unrounded, in their parents), which the tree's own layout does not know.
    moved: HashMap<NodeId, (f32, f32)>,
    /// Virtual lists: each one's viewport height as last placed, and the rows it has built.
    virt_views: HashMap<String, f32>,
    virt_made: HashMap<String, (usize, usize)>,
    /// The widths each stack's children (`el::stack`) took in the last layout, by the stack's key: what
    /// their margins are set from so that they lie over one another (`stack_extra`).
    stack_w: HashMap<String, Vec<f32>>,
    // ── Touch (DESIGN.md §1.8.2) ──
    /// How touch scrolling behaves.
    pub scrolling: Scrolling,
    touch: Option<Touch>,
    touches: HashSet<u64>,
    /// A finger is down: no hover, no lens.
    touching: bool,
    fling: Option<(String, f32)>,
    /// How far each scroll node is pulled past its end (negative: past the start), logical px.
    over: HashMap<String, f32>,
    /// How fast each scroll node's content moves (DESIGN.md §1.8.3), by key.
    motion: HashMap<String, Motion>,
    /// Painting because something scrolled: what comes into view has been there all along (it does
    /// not condense as if it were new).
    scroll_paint: bool,
    /// When the pointer event being handled happened (the host's timestamp), if it says: a finger's
    /// speed is measured by these, not by when the events were handled.
    event_t: Option<f32>,
    /// When the pointer event being handled happened, on the runtime's clock (`Point::t`, `Drag::t`).
    ev: f32,
    // ── Recomposition (DESIGN.md §1.16) ──
    /// Each recomposing text's glyphs as last painted; the runs in progress by text key; glyphs sliding;
    /// and whether the layout in progress is a page change (a flow), which replaces text instead.
    recs: HashMap<String, TextRec>,
    recomps: HashMap<String, Recomp>,
    slides: HashMap<u64, Slide>,
    no_recompose: bool,
    // ── Meshes (DESIGN.md §1.18) ──
    /// Every mesh `scene3d` elements can name; which copy the pointer was last over (element key, hit).
    meshes: crate::mesh::Library,
    mesh_hover: Option<(String, Option<crate::mesh::MeshHit>)>,
    /// The glyphs of the last paint, for hits and the audit (`hit::Glyphs`).
    glyphs: crate::hit::Glyphs,
    text_scratch: TextScratch,
    /// Preparation that runs in the frames' spare time (`Ui::prepare`), and how ready the renderer is.
    prep: crate::warmup::Prepare<S>,
    /// The element whose `on_move` heard the pointer last.
    move_over: Option<String>,
    /// Shed marks in flight, where each shedding element was, the fraction of a mark it still owes, the
    /// flow fields' marks by element (box-relative), and how many marks were ever made (their seed).
    motes: Vec<crate::motes::Mote>,
    shed_at: HashMap<String, (f32, f32)>,
    shed_owed: HashMap<String, f32>,
    field_marks: HashMap<String, Vec<(f32, f32)>>,
    motes_made: u32,
    /// What each `on_visible` element was last told, and each `on_outline` element's last outline.
    seen: HashMap<String, bool>,
    outlines: HashMap<String, u64>,
    /// Each `on_pens` text's pens last sent, and those the paint found (node, pens).
    pens_told: HashMap<String, Rc<Vec<f32>>>,
    pens_found: Vec<(usize, Vec<f32>)>,
    /// Messages the runtime itself has to send after a layout or paint (visibility, outlines).
    queued: Vec<M>,
    flushing: bool,
    /// A host that hands over several events a frame (`Ui::coalesce`): inside a pointer move, a wheel or a
    /// tick, laying out and painting wait for the frame, so the events a frame brings together cost one
    /// layout.
    coalesce: bool,
    defer: u32,
    /// What waits: a layout (and the paint after it), or only a paint (`Ui::settle`).
    stale_layout: bool,
    /// Where the layout left for the frame was asked for (`Ui::relayout`'s caller), and the layouts since the
    /// last frame drawn with where the first was asked for (`perf::FramePath::layouts`).
    stale_from: Option<&'static std::panic::Location<'static>>,
    /// The wheel gesture under way: the key of the node that took its last step, when, and where the
    /// pointer was (`WHEEL_LATCH`).
    wheel_latch: Option<(String, f32, (f32, f32))>,
    laid: (u32, Option<&'static std::panic::Location<'static>>),
    stale_paint: bool,
    /// What the paints that answer the pointer (`El::fill_hover`, `fill_held`, `border_hover`) were
    /// painted for: the hovered, the held and the focused key (`Ui::under_now`); `None` when no element
    /// has such a paint.
    fill_state: Option<(Option<String>, Option<String>, Option<String>)>,
    /// Decode transitions running (by key), and each decoding element's instances as last painted
    /// (what its disappearance plays over).
    decodes: HashMap<String, DecodeRun>,
    decode_last: HashMap<String, Shown>,
    /// A morph from the last frame (`Ui::morph_from_last`): its source instances, target key and spec,
    /// started once the new view is laid out.
    last_morph: Option<(Vec<Instance>, String, MorphSpec)>,
    /// Keyed elements the last layout dropped together with the element they were in (a page that went,
    /// not a part of it that changed): they leave nothing behind, no decode played backwards.
    left: std::collections::HashSet<String>,
    /// The renderer can simulate particles on the GPU (it says so on every draw).
    particle_gpu: bool,
    // ── The field map (DESIGN.md §1.29) ──
    /// The map; where each `stirr` element's centre was at the last tick; the window size then; which
    /// scene the map's ink (for `creep`) was taken from, and the ink each instance put in the nodes then
    /// (by what it is and where: only an instance that changed is read again).
    fmap: crate::fieldmap::FieldMap,
    stirr_at: HashMap<String, (f32, f32)>,
    fmap_size: (f32, f32),
    fmap_ink: u64,
    fmap_inks: HashMap<u64, Vec<(u32, f32)>>,
    /// scribblr (DESIGN.md §1.30): page instances whose fill is faded this frame (their own alpha), each
    /// glyph's outlines by its atlas place, size and transform (relative to its rect), the threads held
    /// for a step of the beat (by element: the beat and the pieces), the threads drawn last (for tests and the
    /// tree), and where each trace's head was.
    strand_faded: HashMap<usize, f32>,
    strand_cache: HashMap<u64, Vec<Vec<[f32; 2]>>>,
    strand_hold: HashMap<String, (i64, f32, Vec<crate::strand::Seg>)>,
    strand_drawn: Vec<(String, f32, Vec<crate::strand::Seg>)>,
    trace_drawn: Vec<(String, Vec<[f32; 2]>)>,
    /// The INK leaders of the last frame: the element's key and the whole path, device px.
    leaders: Vec<(String, Vec<[f32; 2]>)>,
    trace_head: HashMap<String, [f32; 2]>,
}

/// One element of the accessibility mirror (DESIGN.md §1.14): what a screen reader is told about it.
#[derive(Clone, Debug, PartialEq)]
pub struct A11yNode {
    /// The element's key.
    pub key: String,
    /// The nearest ancestor that is in the mirror.
    pub parent: Option<String>,
    /// Its role.
    pub role: Role,
    /// Its accessible name.
    pub name: String,
    /// A slider's value, minimum and maximum.
    pub value: Option<(f32, f32, f32)>,
    /// A text box's text.
    pub text: Option<String>,
    /// On or off.
    pub checked: Option<bool>,
    /// Selected.
    pub selected: Option<bool>,
    /// A heading's level (0: none).
    pub level: u8,
    /// Open or closed.
    pub expanded: Option<bool>,
    /// Read after the name.
    pub description: Option<String>,
    /// How its changes are announced.
    pub live: Option<crate::el::Live>,
    /// Its value is not accepted.
    pub invalid: bool,
    /// It must be filled.
    pub required: bool,
    /// A modal layer.
    pub modal: bool,
    /// Takes no input.
    pub disabled: bool,
    /// Can take focus.
    pub focusable: bool,
    /// Where it is (logical px).
    pub rect: Rect,
}
/// A change to the mirror since it was last sent.
#[derive(Clone, Debug, PartialEq)]
pub enum A11yOp {
    /// A node is new or changed.
    Set(A11yNode),
    /// The node with this key is gone.
    Remove(String),
    /// Focus moved here (`None`: nowhere).
    Focus(Option<String>),
}

/// The kind of pointer: a mouse and a pen hover; a finger does not, and drags scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    /// A mouse or trackpad.
    Mouse,
    /// A finger.
    Touch,
    /// A pen or stylus.
    Pen,
}

/// How touch scrolling behaves (logical px, seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scrolling {
    /// After the finger lifts the content keeps going and slows: velocity × e^(−friction · t).
    pub friction: f32,
    /// The fastest a fling starts, logical px a second.
    pub max_velocity: f32,
    /// A finger must move this far before a press becomes a scroll.
    pub slop: f32,
    /// Past the end the content does not move; the pull (`rubber` of the finger's movement, less as it
    /// grows, at most `max_over`) shows as looseness at that edge and springs back at `spring` per second.
    pub rubber: f32,
    /// The furthest the pull goes, logical px.
    pub max_over: f32,
    /// How fast it springs back, per second.
    pub spring: f32,
}
impl Default for Scrolling {
    fn default() -> Self { Self { friction: 3.5, max_velocity: 5000.0, slop: 8.0, rubber: 0.55, max_over: 140.0, spring: 9.0 } }
}
/// One step of a fling: how far it goes in `dt` and its velocity after (exact for exponential decay).
pub fn fling_step(v: f32, dt: f32, friction: f32) -> (f32, f32) {
    let k = (-friction.max(1e-3) * dt).exp();
    (v * (1.0 - k) / friction.max(1e-3), v * k)
}
/// The pull past an end after the finger moves `d` more: it resists more the further it is pulled.
pub fn rubber_band(over: f32, d: f32, rubber: f32, max: f32) -> f32 {
    let r = rubber * (1.0 - (over.abs() / max.max(1.0)).min(1.0));
    (over + d * r).clamp(-max, max)
}

/// A scroll node's motion (DESIGN.md §1.8.3): its offset at the last tick, its speed (logical px a
/// second, smoothed, signed: positive is the offset growing) while it moves, and when it last moved.
#[derive(Clone, Copy, Debug, Default)]
struct Motion { off: f32, v: f32, last: f32 }
/// A scroll area is still scrolling this long after its offset last changed (s); then one frame is
/// drawn sharp (DESIGN.md §1.12).
pub const SOFT_STOP: f32 = 0.1;
/// A wheel gesture goes on while its steps come less than this apart (s) and the pointer stays where it was:
/// each step goes to the node that took the one before, so an element that takes the wheel (`El::on_wheel`)
/// scrolled under the pointer by the gesture (a trackpad's momentum) does not take it over (DESIGN.md §1.8.2).
pub const WHEEL_LATCH: f32 = 0.5;
/// Tells the state where a scroll node is: (state, key, offset, where an element is in it).
pub type ScrollHook<S> = fn(&mut S, &str, f32, &dyn Fn(&str) -> Option<f32>) -> bool;
/// How quickly the measured scroll speed follows the motion (s): long enough that one wheel notch in one
/// frame does not read as a fling, short enough that a fling reads at once.
const SPEED_TAU: f32 = 0.06;

/// The finger that is down: where it started, its last positions (for the velocity), and what it is
/// doing: not yet decided (a press shown but not sent), scrolling node `key`, handled as a mouse press
/// (a drag, a text field, nothing to scroll), or cancelled (a second finger).
struct Touch { id: u64, start: (f32, f32), last: (f32, f32), samples: std::collections::VecDeque<(f32, f32, f32)>, mode: TouchMode }
#[derive(Clone, PartialEq)]
enum TouchMode { Pending, Scroll(String), Direct, Cancelled }

/// What the runtime needs from the look to know whether anything is still moving: how long fronts live
/// and how fast they travel, how long new instances condense, and whether the material (global, or
/// any override) has a time term that moves loose things.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    /// How long a front lives, seconds.
    pub wave_life: f32,
    /// How fast a front travels, logical px a second.
    pub wave_speed: f32,
    /// How long new instances take to condense, seconds.
    pub enter: f32,
    /// The global material's id.
    pub material: u8,
    /// The performance readout (`Look::overlay`).
    pub overlay: u8,
    /// The field numbers the CPU's model of looseness needs.
    pub fx: crate::scene::Fx,
    /// `Look::legible_cap`.
    pub legible_cap: f32,
    /// `Look::loose_max`.
    pub loose_max: f32,
    /// How changed glyphs recompose.
    pub recompose: crate::morph::Recompose,
    /// The preset's interaction character (DESIGN.md §4.1): how long a press's burst lasts, seconds.
    pub press: f32,
    /// What text and controls, and everything else, do when nothing says.
    pub roles: [Response; 2],
    /// How long a commit's transition lasts, seconds (0: the front).
    pub transition: f32,
    /// How scrolling looks (DESIGN.md §1.8.3): the speeds the trace eases in between and how it settles.
    pub scroll: crate::style::Scroll,
    /// The field map's numbers (DESIGN.md §1.29).
    pub map: crate::fieldmap::MapFx,
    /// scribblr's numbers and how long its line holds a pose (DESIGN.md §1.30).
    pub strand: crate::strand::StrandFx,
    /// The look's beat (DESIGN.md §1.32): scribblr's line holds a pose for a step of it.
    pub beat: crate::beat::Beat,
    /// The finishing pass is one-bit (`post::Post::binary`).
    pub binary: bool,
    /// The effects turned off (`Look::off`).
    pub off: crate::style::Off,
}
impl Default for Timing {
    fn default() -> Self { Self::of(&Look::default()) }
}
impl Timing {
    /// The numbers `l` gives.
    pub fn of(l: &Look) -> Self {
        Self { wave_life: l.fx.wave_life, wave_speed: l.fx.wave_speed, enter: l.fx.enter, material: l.material.id(), overlay: l.overlay,
               fx: l.fx, legible_cap: l.legible_cap, loose_max: l.loose_max, recompose: l.recompose,
               press: l.press_ms / 1000.0, roles: l.roles, transition: l.transition.duration_ms / 1000.0, scroll: l.scroll, map: l.map, strand: crate::strand::StrandFx::of(l.numbers(crate::style::Material::Scribblr).0), beat: l.beat.clamped(), binary: l.post.binary, off: l.off }
    }
}

/// Blink half-period of the caret, seconds.
pub const BLINK: f32 = 0.53;
/// How long `Ui::condense_all` takes to loosen the page to each element's cap before it condenses (s).
pub const RAMP: f32 = 0.12;
/// How tall a skeleton's bar is, as a fraction of its line's height.
pub const SKELETON_BAR: f32 = 0.56;
/// How far a selected glyph's halo reaches past its outline by default, logical px (`Look::select`).
pub const SELECT_HALO: f32 = 3.0;
/// How far from a pulled edge (logical px) things loosen.
const PULL_REACH: f32 = 160.0;
/// The smallest box (logical px) a control can be hit in: smaller ones are hit as if they were this big.
pub const MIN_HIT: f32 = 28.0;
/// Presses closer than this in time (s) and space (px) count as a double (triple) click.
const CLICK_GAP: (f32, f32) = (0.45, 5.0);

impl<S, M: Clone> Ui<S, M> {
    /// Let the state know the window's logical size whenever it changes (for views that lay out
    /// differently on narrow windows).
    pub fn on_resize(mut self, f: fn(&mut S, f32, f32)) -> Self {
        f(&mut self.state, self.size.0, self.size.1);
        self.sized = Some(f);
        self.relayout();
        self
    }
    /// Read the annotation layer's settings from the state (DESIGN.md §1.7).
    pub fn on_gaze(mut self, f: fn(&S) -> Gaze) -> Self { self.gaze_of = Some(f); self.finish(); self }
    /// Let the state know which element the pointer is over; `f` returns whether the view changed.
    pub fn on_hover(mut self, f: fn(&mut S, Option<&str>) -> bool) -> Self { self.hovered = Some(f); self }
    /// Let the state know which element has the focus, as it moves (Tab, a press, `focus_key`, `clear_focus`,
    /// a modal layer opening or closing); `f` returns whether the view changed. A Lab cell runs while its
    /// `[ params ]` has the focus as it does while the pointer is over it.
    pub fn on_focus(mut self, f: fn(&mut S, Option<&str>) -> bool) -> Self { self.focused = Some(f); self }
    /// Let the state know the time on every tick (for things that change by themselves, like a clock);
    /// `f` returns whether the view changed.
    pub fn on_tick(mut self, f: fn(&mut S, f32) -> bool) -> Self { self.ticked = Some(f); self }
    /// What the stages' pieces (`El::stage`) do by themselves on every tick, after `on_tick`: `f` steps
    /// them and returns the keys of the stages whose pieces changed, and of the elements beside them that
    /// changed with them (`El::steps`: a formula, a label). When nothing else changed the view, only those
    /// are viewed again (`Ui::stage_view`), in the boxes the last layout gave them, and drawn again; the
    /// rest of the page is the last frame's (DESIGN.md §1.35). A stage with parts that change alone
    /// (`El::steps` inside it) is viewed and drawn by those parts.
    pub fn on_step(mut self, f: fn(&mut S, f32) -> Vec<String>) -> Self { self.stepped = Some(f); self }
    /// A stage's subtree alone (`El::stage`, or `El::steps`, by its key), as the page's view would make
    /// it: what one that stepped (`Ui::on_step`) is viewed by. `None`, or a subtree laid out otherwise
    /// (other elements, other styles, text that measures otherwise), and the whole page is viewed and
    /// laid out again. Text whose words changed but measures the same (figures of one width) is laid out
    /// the same.
    pub fn stage_view(mut self, f: fn(&S, &str) -> Option<El<M>>) -> Self { self.stage_of = Some(f); self }
    /// Let the state know how far the renderer's warm-up is, each time it moves on and once more with
    /// `None` when it is done (DESIGN.md §1.9), so the view can show it (a loading line, a mark that
    /// fills); `f` returns whether the view changed.
    pub fn on_warm(mut self, f: fn(&mut S, Option<crate::render::Progress>) -> bool) -> Self { self.warmed = Some(f); self }
    /// Read the look from the state, for the timings `needs_frame` uses (front life and speed, how long
    /// new things condense, whether the material moves by itself).
    /// Read the locale from the state (DESIGN.md §1.20): before every view it becomes the current one
    /// (`i18n::current`), which widgets and `i18n::tr` use for their own text.
    pub fn on_locale(mut self, f: fn(&S) -> crate::i18n::Locale) -> Self { self.locale_of = Some(f); self.relayout(); self }
    /// Read the look (theme, material and their numbers) from the state.
    pub fn on_look(mut self, f: fn(&S) -> Look) -> Self { self.look_of = Some(f); self.timing = Timing::of(&f(&self.state)); self }
    /// Ask the state, after every tick, when it next wants one (`Some(now)`: every frame; a later time:
    /// a timed wake; `None`: only on input). Without it, `on_tick` subscribers only tick when something
    /// else draws a frame.
    pub fn on_wake(mut self, f: fn(&S, f32) -> Option<f32>) -> Self { self.wake_of = Some(f); self }
    /// Give the state the keys nothing on the page used (Esc on a scene, a key that is held);
    /// `f` returns whether the state changed.
    pub fn on_unused_key(mut self, f: fn(&mut S, Key, bool) -> bool) -> Self { self.unused_key = Some(f); self }
    /// Cmd (Ctrl) with a character that no text field used (⌘K for a search); `f` returns whether it
    /// was used (the view is then built again).
    pub fn on_shortcut(mut self, f: fn(&mut S, char) -> bool) -> Self { self.shortcut = Some(f); self }
    /// After a scroll node moved: its key, its offset, and where any element is from the scroller's
    /// top (logical px, `None` when it is not in it), so the state can follow what is in view (a page
    /// outline). `f` returns whether the view changed.
    pub fn on_scrolled(mut self, f: ScrollHook<S>) -> Self { self.scrolled_of = Some(f); self }
    /// Hear of every press, wheel turn and key press before it is handled (a demonstration that the
    /// person takes over).
    pub fn on_input(mut self, f: fn(&mut S) -> bool) -> Self { self.input = Some(f); self }
    /// Tell the state where a mouse or pen is (logical px) whenever it moves over the window; returns
    /// whether the view changes. For views that follow the pointer itself (parallax, a highlight).
    pub fn on_pointer(mut self, f: fn(&mut S, f32, f32) -> bool) -> Self { self.pointer_of = Some(f); self }
    /// Input came (`heard`, and pointer moves and releases, which a state is not told of): what it
    /// starts is not paced for a second (`Ui::ambient_fps`).
    fn heard(&mut self) {
        if !self.scripted { self.input_t = self.wall; }
        if !self.scripted && let Some(f) = self.input && f(&mut self.state) { self.relayout(); }
    }
    /// Pace what moves by itself to `n` frames a second (DESIGN.md §1.29): the map, its breath and
    /// gusts, flow fields, drawings and anything a tick changes. Frames for input, and for what input
    /// starts, are never paced until a second after the last input. Everything goes by the clock, not by
    /// frames, so a frame at a given time is the same at any `n`. 60 (the default) or more is no pacing.
    pub fn ambient_fps(&mut self, n: f32) { self.ambient = n.max(1.0); }
    /// The time before which a frame wanted only by what moves by itself waits (`ambient_fps`), if any.
    fn paced_until(&self, now: f32) -> Option<f32> {
        if self.ambient >= 60.0 || now - self.input_t < 1.0 { return None; }
        // A frame is due within half a refresh of its time, so 30 on a 60 Hz display is every second
        // refresh even when the host's timestamps jitter.
        let next = self.frame_t + 1.0 / self.ambient;
        let tol = (0.5 * self.tick_gap).min(0.5 / self.ambient).max(1e-4);
        (now + tol < next).then_some(next)
    }
    /// Tell the state when a key is let go; `f` returns whether the state changed.
    pub fn on_key_up(mut self, f: fn(&mut S, Key) -> bool) -> Self { self.released = Some(f); self }
    /// A key let go.
    pub fn key_up(&mut self, k: Key) {
        self.settle();
        self.dirty = true;
        if let Some(f) = self.released && f(&mut self.state, k) { self.relayout(); }
    }
    /// This frame's annotation records.
    pub fn annots(&self) -> &[gaze::Annot] { &self.annots }
    /// What the edge notes, the waveform and the level bar drew this frame (DESIGN.md §4.2, §4.3).
    pub fn readout(&self) -> &gaze::Readout { &self.readout }
    /// Take the commands `update` leaves in the state (morphs, flows) after every update.
    pub fn on_commands(mut self, f: fn(&mut S) -> Vec<Cmd>) -> Self { self.commands = Some(f); self }
    /// `w`, `h` are device px; `scale` is device px per logical px.
    pub fn new(state: S, view: fn(&S) -> El<M>, update: fn(&mut S, M), faces: Vec<Face>, w: u32, h: u32, scale: f32) -> Self {
        let mut ui = Self {
            state, view, update, text: Text::new(faces), size: (0.0, 0.0), scale, clock: 0.0, wall: 0.0, bclock: crate::beat::Clock::default(), sent: vec![], sets: vec![], nodes: vec![], scene: Scene::default(),
            owners: vec![], page_len: 0, field: Field::default(), scroll: HashMap::default(), births: HashMap::default(), ramp: None, text_births: HashMap::default(),
            hover: None, focus: None, after_focus: None, fit_short: vec![], grain_at: None, bloom_at: None, clears: HashMap::default(), clearing: false, clear_many: vec![], flow_own: vec![], flow_join: None, fit_k: HashMap::default(), fit_last: None, pressed: None, capture: None, modal_at: None, modal_key: None, return_focus: None, split: None, splits: 0,
            keyboard: false, marks: Marks::default(), sized: None, commands: None, unseen: vec![], stages: vec![], stage_input: (false, false), stage_add_ms: 0.0, faces_to: None, kept_to: None, kept_told: HashMap::default(), kept_flips: vec![], root_roles: Default::default(), scaler: Default::default(), stage_pin: None, scaler_fed: f64::NEG_INFINITY, governed: false, stepped: None, stage_of: None, stale_stages: vec![], held_steps: vec![], restaging: None, base: vec![], tags: vec![], roles: vec![],
            run: None, painted_phase: 0, flows: HashMap::default(), ghosts: vec![], grows: vec![], ground_seen: None, ground_hold: None, body_len: 0, animating: false, lifted: HashMap::default(),
            gaze_of: None, gaze_seen: HashMap::default(), annots: vec![], focused: None, focus_moved: false, readout: gaze::Readout::default(), hovered: None, pointer_of: None, scripted: false, ticked: None, warm: None, warmed: None, owed: false, soft_shown: false, ambient: 60.0, input_t: -1e9, frame_t: -1e9, tick_gap: 1.0 / 60.0, unused_key: None, shortcut: None, scrolled_of: None, released: None, input: None,
            dirty: true, look_of: None, locale_of: None, timing: Timing::default(), wake_of: None, latest_birth: f32::NEG_INFINITY, gaze_until: f32::NEG_INFINITY,
            marks_moving: false, caret: None, caret_on: true, static_loose: false, mclock: 0.0, moving: false, gclock: 0.0, ground_moving: false,
            perf: crate::perf::Perf { quality: 1.0, ..Default::default() }, span: 0, painting: false, overlay: 0, overlay_at: 0.0, overlay_box: None, overlay_w: (0, (0.0, 0.0), 0.0), fade_s: REDUCED_FADE, came: vec![], arriving: HashMap::default(), sweeping: false, swept_nodes: vec![], remeasure: false, plate_slots: Default::default(), plate_sig: Default::default(), plate_keep: Default::default(), faces_added: 0, plate_clear: vec![], plate_over: vec![], pclock: 0.0, plate_frame: f32::NEG_INFINITY, assets: HashMap::default(), assets_asked: Default::default(), shape_seen: 0.0, hud_idle: false, rebuilt: 0,
            text_capture: None, sel: None, sel_drag: false, clicks: (f32::NEG_INFINITY, 0.0, 0.0, 0), a11y_sent: HashMap::default(), a11y_focus: None, layout_gen: 0, a11y_gen: u64::MAX, a11y_owed: false,
            tree: TaffyTree::new(), tids: HashMap::default(), mkeys: HashMap::default(), moved: HashMap::default(), virt_views: HashMap::default(), virt_made: HashMap::default(), stack_w: HashMap::default(),
            scrolling: Scrolling::default(), touch: None, touches: HashSet::default(), touching: false, fling: None, over: HashMap::default(), motion: HashMap::default(), scroll_paint: false, event_t: None, ev: 0.0,
            recs: HashMap::default(), recomps: HashMap::default(), slides: HashMap::default(), no_recompose: false,
            meshes: crate::mesh::Library::default(), mesh_hover: None, glyphs: Default::default(), text_scratch: TextScratch::default(), prep: Default::default(), move_over: None, particle_gpu: false, motes: vec![], shed_at: HashMap::default(), shed_owed: HashMap::default(), field_marks: HashMap::default(), seen: HashMap::default(), outlines: HashMap::default(), pens_told: HashMap::default(), pens_found: vec![], decodes: HashMap::default(), decode_last: HashMap::default(), last_morph: None, left: Default::default(), queued: vec![], flushing: false, coalesce: false, defer: 0, stale_layout: false, stale_from: None, wheel_latch: None, laid: (0, None), stale_paint: false, fill_state: None, motes_made: 0,
            fmap: crate::fieldmap::FieldMap::default(), stirr_at: HashMap::default(), fmap_size: (0.0, 0.0), fmap_ink: u64::MAX, fmap_inks: HashMap::default(), strand_faded: HashMap::default(), strand_cache: HashMap::default(), strand_hold: HashMap::default(), strand_drawn: vec![], trace_drawn: vec![], leaders: vec![], echoes: HashMap::default(), echo_at: f32::NEG_INFINITY, frozen: None, trace_head: HashMap::default(),
        };
        ui.resize(w, h, scale);
        ui
    }

    // ── Queries ──────────────────────────────────────────────────────────────────────────────────
    /// The laid-out tree, parents before children.
    pub fn nodes(&self) -> &[Node<M>] { &self.nodes }
    /// The node with `key`.
    pub fn node(&self, key: &str) -> Option<&Node<M>> { self.nodes.iter().find(|n| n.key == key) }
    /// How far the scroll node `key` is scrolled, logical px.
    pub fn scroll_offset(&self, key: &str) -> f32 { self.scroll.get(key).copied().unwrap_or(0.0) }
    /// The focused element's key.
    pub fn focus(&self) -> Option<&str> { self.focus.as_deref() }
    /// The hovered element's key.
    pub fn hover(&self) -> Option<&str> { self.hover.as_deref() }
    /// The element whose `El::on_move` hears the pointer now (a menu, a chart that scrubs), if any.
    pub fn move_over(&self) -> Option<&str> { self.move_over.as_deref() }
    /// The scene last built for drawing.
    pub fn scene(&self) -> &Scene { self.frozen.as_ref().unwrap_or(&self.scene) }
    /// The key of the element that drew page instance `i` of the scene, for checks.
    pub fn owner_key(&self, i: usize) -> Option<&str> { (i < self.page_len).then(|| self.owners.get(i)).flatten().map(|o| self.nodes[o.0].key.as_str()) }
    /// The field (fronts, the lens, bursts) as it is now.
    pub fn field(&self) -> &Field { &self.field }
    /// The field map (DESIGN.md §1.29): energy and flow on a coarse grid, logical px.
    pub fn fieldmap(&self) -> &crate::fieldmap::FieldMap { &self.fmap }
    /// The viewer asked for less motion (the web page's `prefers-reduced-motion`): the field map keeps
    /// its energy but puts in no flow, and lives half as long.
    pub fn set_reduced_motion(&mut self, on: bool) { self.fmap.reduced = on; }
    /// The viewer asked for less motion (`Ui::set_reduced_motion`): what the framework brings in and
    /// takes away fades (`reduced_fade`, 150 ms by default) instead of loosening and moving.
    pub fn reduced_motion(&self) -> bool { self.fmap.reduced }
    /// How long that fade is, seconds: `FlowSpec::fade_ms` of the last `Cmd::Flow` (150 ms until one).
    pub fn reduced_fade(&self) -> f32 { self.fade_s }
    /// The text system, to change (mark the atlas for upload after a new device).
    pub fn text_mut(&mut self) -> &mut Text { &mut self.text }
    /// Device px per logical px.
    pub fn scale(&self) -> f32 { self.scale }
    /// The runtime's clock at the last tick, seconds.
    pub fn now(&self) -> f32 { self.wall }
    /// The beat clock (DESIGN.md §1.32), s: the runtime's clock less what `Cmd::Hold`, `Cmd::Retrigger`
    /// and `Cmd::Rewind` took from it; the runtime's own while the look has no beat. Continuous:
    /// `beat_grid` is it on a step.
    pub fn beat_time(&self) -> f32 { self.clock }
    /// The beat clock put on the start of its step: the time what falls on the beat is drawn at
    /// (fronts, glitchr's held rows, a transition's steps). Without a beat, `beat_time`.
    pub fn beat_grid(&self) -> f32 { self.timing.beat.grid(self.clock) }
    /// Where the beat clock is: bar, beat and step (each from 1) and how far into the step.
    pub fn beat(&self) -> crate::beat::BeatNow { self.timing.beat.now(self.clock) }
    /// The beat clock at the runtime's time `t`.
    fn beat_of(&self, t: f32) -> f32 { if self.timing.beat.on() { self.bclock.at(t) } else { t } }
    /// Send a command as the application's `on_commands` would (`Cmd`): it is taken now, with an update.
    pub fn command(&mut self, cmd: Cmd) {
        self.settle();
        self.sent.push(cmd);
        self.relayout();
    }
    /// The window's size, logical px.
    pub fn logical_size(&self) -> (f32, f32) { self.size }
    /// Device px.
    pub fn device_size(&self) -> (u32, u32) { ((self.size.0 * self.scale).round() as u32, (self.size.1 * self.scale).round() as u32) }
    /// The key of the open modal layer, if any.
    pub fn modal(&self) -> Option<&str> { self.modal_key.as_deref() }
    /// The last commit's frame, kept for the renderer.
    pub fn split(&self) -> Option<&Split> { self.split.as_ref() }
    /// The key of the element being dragged.
    pub fn dragging(&self) -> Option<&str> { self.capture.as_ref().map(|c| c.key.as_str()) }
    /// The morph in progress, if any.
    pub fn morph_run(&self) -> Option<&MorphRun> { self.run.as_ref() }
    /// Whether morphs with many tiles may run on the GPU (the renderer sets it; headless runs are CPU).
    pub fn set_particle_gpu(&mut self, on: bool) { self.particle_gpu = on; }
    /// Whether morphs run on the GPU.
    pub fn particle_gpu(&self) -> bool { self.particle_gpu }
    /// Every mesh `scene3d` elements can draw (DESIGN.md §1.18).
    pub fn meshes(&self) -> &crate::mesh::Library { &self.meshes }
    /// Add a mesh (one made in code, or read with `mesh::load_gltf`); views name it by the id returned.
    pub fn add_mesh(&mut self, m: crate::mesh::Mesh) -> crate::mesh::MeshId { self.dirty = true; self.meshes.add(m) }
    /// Declare a face that comes later (DESIGN.md §1.6.5): its number now, for themes and `El::fonts`.
    /// Until its bytes come (`add_face`) text in it is set, read and laid out in `stand_in` at `scale`
    /// times its size, at rest, in its final colour, size and line height. A name already there keeps
    /// The page's faces for glyph outlines (`pair::Faces`): what the state keeps to measure between
    /// letters (`pair::gap`, `pair::spaced`) without the text system. A face added later (`Ui::add_face`)
    /// is in the `Faces` asked for after it.
    pub fn faces(&self) -> crate::pair::Faces { crate::pair::Faces::new(self.text.face_bytes(), self.root_roles.clone()) }
    /// `f` gets the page's faces for glyph outlines (`Ui::faces`) now, and again each time a face comes
    /// (`Ui::add_face`): for a state that keeps them.
    pub fn on_faces(mut self, f: fn(&mut S, crate::pair::Faces)) -> Self { self.faces_to = Some(f); let fs = self.faces(); f(&mut self.state, fs); self }
    /// `f` is told when a cached element (`El::cached`) stops having a kept image, being over the plates'
    /// limit (four, twice the window's area; DESIGN.md §1.36) and drawn as a plain one, and again when
    /// it has one again (or goes, while it had none): its key and whether it is kept. A paint that finds
    /// the change asks for the next frame, which tells it and views the page again, so what the view
    /// does about it (a `cached_view` showing nothing) changes on that frame, on a cut.
    pub fn on_kept(mut self, f: fn(&mut S, &str, bool)) -> Self { self.kept_to = Some(f); self }
    /// Whether the cached element `key` has a kept image, as the last paint found: `None` when there
    /// is no cached element of that key.
    pub fn kept_image(&self, key: &str) -> Option<bool> {
        self.nodes.iter().any(|n| n.cached && n.key == key).then(|| self.kept_told.get(key).copied().unwrap_or(true))
    }
    /// its number.
    pub fn declare_face(&mut self, name: &str, decl: crate::text::FaceDecl) -> u8 {
        let known = self.text.face_named(name).is_some();
        let id = self.text.declare(name, decl);
        if !known { self.relayout(); }
        id
    }
    /// A face's bytes came: a declared one changes over from its stand-in (all that come before the
    /// next tick together, in one layout; each line sweeps from left to right over 240 ms, 150 ms fading
    /// in place with less motion; before any frame was drawn, at once), a new name is registered and
    /// numbered. Bytes that are not a font are refused: the face keeps its stand-in for good and is no
    /// longer in `faces_wanted`; nothing changes on screen.
    pub fn add_face(&mut self, name: &str, bytes: Vec<u8>) -> Result<u8, crate::error::FontError> {
        let before = self.text.face_named(name).and_then(|i| self.text.stand_in(i));
        let id = self.text.add(name, bytes)?;
        self.faces_added += 1;
        if let Some(f) = self.faces_to { let fs = self.faces(); f(&mut self.state, fs); }
        match before {
            Some(d) => { self.came.push((id, d)); self.dirty = true; }
            None => { self.remeasure = true; self.relayout(); }
        }
        Ok(id)
    }
    /// The declared faces that have not come and that the page laid out last uses, by name: what the
    /// host should fetch now (read it after each frame or page change). The framework fetches nothing.
    pub fn faces_wanted(&self) -> Vec<String> { self.text.wanted() }
    /// A resource the host fetched (a component's source, a registry): kept by name for `asset`.
    pub fn add_asset(&mut self, name: &str, bytes: Vec<u8>) {
        self.assets_asked.borrow_mut().remove(name);
        self.assets.insert(name.to_owned(), bytes);
        self.relayout();
    }
    /// The resource `name`, if it has come; asking for one that has not puts it in `assets_wanted`.
    pub fn asset(&self, name: &str) -> Option<&[u8]> {
        let a = self.assets.get(name).map(Vec::as_slice);
        if a.is_none() { self.assets_asked.borrow_mut().insert(name.to_owned()); }
        a
    }
    /// The resources asked for with `asset` that have not come, by name.
    pub fn assets_wanted(&self) -> Vec<String> { self.assets_asked.borrow().iter().cloned().collect() }
    /// The drawn page instances of a node and its descendants, as they are this frame.
    /// How many recomposition tiles are alive now, over the whole window (bounded by
    /// `Recompose::max_tiles`).
    pub fn recompose_tiles(&self) -> usize { self.recomps.values().map(|r| r.tiles.len()).sum() }
    /// The copy of a 3D element under the pointer (its key and the hit), as pick handlers last heard.
    pub fn mesh_hover(&self) -> Option<&(String, Option<crate::mesh::MeshHit>)> { self.mesh_hover.as_ref() }
    /// What the renderer compares to draw only what changed (DESIGN.md §1.19), as it would see this
    /// frame under `look`, without a GPU: tests use it to bound the redrawn area. `global` is set when
    /// a commit is showing or tiles fly (the renderer adds the reduced pass, which tests leave off).
    pub fn dirty_inputs(&self, look: &Look) -> crate::dirty::Inputs {
        let sc = self.scene();
        crate::dirty::Inputs {
            instances: sc.instances.clone(), annot: sc.annot.clone(),
            meshes: sc.meshes.iter().map(|m| m.scene.fingerprint() ^ (m.rect[2].to_bits() as u64) << 32 ^ m.rect[3].to_bits() as u64).collect(),
            field: *self.field(), look: *look, now: self.beat_grid(), mclock: self.mclock, gclock: self.gclock, size: self.device_size(), map: self.fmap.snapshot().clone(), cuts: sc.cuts(),
            global: self.split.as_ref().is_some_and(|sp| self.split_alive(sp, self.clock)) || self.run.as_ref().is_some_and(|r| r.clock(self.clock).is_some()),
        }
    }
    /// The line boxes of a text element, window coordinates (logical px): `Text::line_boxes`.
    pub fn line_boxes(&mut self, key: &str) -> Vec<Rect> {
        let Some(n) = self.nodes.iter().find(|n| n.key == key) else { return vec![] };
        let Some(p) = n.para() else { return vec![] };
        let o = n.origin();
        self.text.line_boxes(p, o)
    }
    /// Each line of text element `key` with its ink (window coordinates, logical px): `LineInk`. Known
    /// once it is laid out, no frame drawn; asking again shapes nothing.
    pub fn line_inks(&mut self, key: &str) -> Vec<LineInk> {
        let Some(n) = self.nodes.iter().find(|n| n.key == key) else { return vec![] };
        let Some(p) = n.para() else { return vec![] };
        let o = n.origin();
        self.text.line_inks(p, o, self.scale).into_iter().map(|v| LineInk {
            line: Rect::new(v[0], v[1], v[2], v[3]), ink: Rect::new(v[4], v[5], v[6], v[7]), baseline: v[8], cap_top: v[9], x_top: v[10],
        }).collect()
    }
    /// How many lines of text element `key` end in a hyphenated break (`Around::hyphenate`).
    pub fn hyphenated_lines(&mut self, key: &str) -> usize {
        let Some(n) = self.nodes.iter().find(|n| n.key == key) else { return 0 };
        let Some(p) = n.para() else { return 0 };
        if !p.around.is_some_and(|a| a.hyphenate) { return 0; }
        self.text.line_ends(p).into_iter().filter(|&e| crate::wrap::hyphenated_at(p.s, e)).count()
    }
    /// The glyph atlas the instances sample (coverage and signed distance per texel), for `morph::cells`.
    pub fn atlas(&self) -> &[u8] { &self.text.pixels }
    /// The drawn instances of `key` and everything inside it (none while it is not drawn).
    pub fn instances_of(&self, key: &str) -> Vec<Instance> {
        let Some(root) = self.nodes.iter().position(|n| n.key == key) else { return vec![] };
        (0..self.page_len).filter(|&i| within(&self.nodes, self.owners[i].0, root)).map(|i| self.scene.instances[*self.lifted.get(&i).unwrap_or(&i)]).collect()
    }
    /// How loose the most loosened instance of a node and its descendants is at `now`, by the same rule
    /// the first stage uses (DESIGN.md §1.15): its own looseness (held, born, set at rest) plus what the
    /// fronts that reach it add up to its cap, clamped to `Look::loose_max`. Measured at each instance's
    /// centre; the style's looseness scale is not applied.
    pub fn looseness(&self, key: &str, now: f32) -> f32 {
        let Some(root) = self.nodes.iter().position(|n| n.key == key) else { return 0.0 };
        (0..self.page_len).filter(|&i| within(&self.nodes, self.owners[i].0, root)).map(|i| self.inst_loose(i, now)).fold(0.0, f32::max)
    }
    /// The looseness of page instance `i` at `now` (the rule of `looseness`).
    fn inst_loose(&self, i: usize, now: f32) -> f32 {
        let tm = &self.timing;
        {
            let j = *self.lifted.get(&i).unwrap_or(&i);
            let inst = &self.scene.instances[j];
            // A fill scribblr faded is still the element it was.
            if self.strand_faded.get(&j).copied().unwrap_or(inst.paint[2]) <= 0.0 { return 0.0; }
            let born = ((now - inst.extra[3]) / tm.enter.max(1e-3)).clamp(0.0, 1.0);
            let own = inst.extra[2].max(1.0 - born);
            let c = if inst.resp[1] > 1.5 { [inst.resp[2], inst.resp[3]] } else { [inst.rect[0] + inst.rect[2] * 0.5, inst.rect[1] + inst.rect[3] * 0.5] };
            let add = if inst.resp[0] > 0.0 {
                let mut w = self.field.wave_at(c, c, now, &tm.fx, self.scale, 0.6);
                // The field map, where the instance reads it (as the first stage does at each pixel).
                if inst.look[2] > 0.5 { w = w.max(self.fmap.energy_at(c[0] / self.scale, c[1] / self.scale)); }
                w.clamp(0.0, (inst.resp[0] - own).max(0.0))
            } else { 0.0 };
            (own + add).min(tm.loose_max)
        }
    }
    /// The look the state asks for (`on_look`), or the default one.
    pub fn look(&self) -> Look {
        let mut l = self.look_of.map_or_else(Look::default, |f| f(&self.state));
        // While a picture grows over the page, the old page's ground is around it.
        if let Some((g, until)) = self.ground_hold && self.clock < until { (l.theme.ground, l.theme.ground2) = g; }
        // Names were checked when they were set.
        for (n, v) in &self.sets { let _ = l.set(n, *v); }
        // What is turned off (DESIGN.md §1.34) is turned off in the look the renderer draws with.
        if l.off.bloom { l.post.bloom.strength = 0.0; }
        l.post.region = self.grain_at;
        l.post.bloom_region = self.bloom_at;
        if l.off.material_still { l.beat.bpm = 0.0; }
        l
    }

    /// The laid-out tree as data (DESIGN.md §1.17): every element's key, kind, role and name, rect, text,
    /// material, response, input state, scroll offset and looseness. What a model reads instead of a
    /// screenshot; `serde_json::to_string_pretty(&ui.export_tree())` for JSON.
    pub fn export_tree(&self) -> crate::tree::TreeNode {
        let mirror: HashMap<String, (Role, String)> = self.a11y().into_iter().map(|n| (n.key, (n.role, n.name))).collect();
        let now = self.clock;
        let mut made: Vec<crate::tree::TreeNode> = self.nodes.iter().map(|n| {
            let m = mirror.get(&n.key);
            let mat = if n.mat >= 127.5 { crate::style::Material::from_id((n.mat.round() as u32 % 16) as u8).name() } else { "global" };
            let ripple = n.ripple.as_ref().map(|r| match r {
                Scope::Own => "own".to_owned(), Scope::Container(None) => "container".to_owned(),
                Scope::Container(Some(k)) => format!("container:{k}"), Scope::Global => "global".to_owned(), Scope::Off => "off".to_owned(),
            });
            let seen = n.rect.intersect(&n.clip);
            crate::tree::TreeNode {
                key: n.key.clone(), keyed: n.keyed, kind: n.kind(), role: m.map(|(r, _)| r.aria()), label: m.map(|(_, name)| name.clone()).filter(|s| !s.is_empty()),
                rect: [n.rect.x, n.rect.y, n.rect.w, n.rect.h], visible: [seen.x, seen.y, seen.w, seen.h],
                text: n.text().map(str::to_owned), material: mat, response: n.respond.name(), legible: n.legible, ripple, recompose: n.recompose,
                focusable: n.focusable, focused: self.focus.as_deref() == Some(&n.key), hovered: self.hover.as_deref() == Some(&n.key),
                pressed: self.pressed.as_deref() == Some(&n.key), disabled: n.disabled,
                scroll: n.scroll.then(|| [self.scroll.get(&n.key).copied().unwrap_or(0.0), n.extent]),
                loose: 0.0, field: None, children: vec![],
            }
        }).collect();
        // Looseness per node from its own instances only (a parent's would repeat its children's).
        let mut loose = vec![0.0f32; self.nodes.len()];
        let tm = &self.timing;
        for i in 0..self.page_len {
            let inst = &self.scene.instances[*self.lifted.get(&i).unwrap_or(&i)];
            if inst.paint[2] <= 0.0 { continue; }
            let born = ((now - inst.extra[3]) / tm.enter.max(1e-3)).clamp(0.0, 1.0);
            let own = inst.extra[2].max(1.0 - born);
            let c = if inst.resp[1] > 1.5 { [inst.resp[2], inst.resp[3]] } else { [inst.rect[0] + inst.rect[2] * 0.5, inst.rect[1] + inst.rect[3] * 0.5] };
            let add = if inst.resp[0] > 0.0 { self.field.wave_at(c, c, now, &tm.fx, self.scale, 0.6).clamp(0.0, (inst.resp[0] - own).max(0.0)) } else { 0.0 };
            let o = self.owners[i].0;
            loose[o] = loose[o].max((own + add).min(tm.loose_max));
        }
        for (n, l) in made.iter_mut().zip(loose) { n.loose = (l * 1000.0).round() / 1000.0; }
        // Children come after their parents: fold from the back.
        for i in (1..made.len()).rev() {
            let Some(p) = self.nodes[i].parent else { continue };
            let child = std::mem::replace(&mut made[i], crate::tree::TreeNode::empty());
            made[p].children.insert(0, child);
        }
        let mut root = made.into_iter().next().unwrap_or_else(crate::tree::TreeNode::empty);
        root.field = Some(crate::tree::FieldReadout { level: self.fmap.level(), nonzero: self.fmap.nonzero() });
        root
    }

    /// What the runtime had to guess at in the current view (duplicate keys, a flow id used twice, an
    /// anchor or ripple container that is not there), each with what to write instead. Empty when the
    /// view is clean.
    pub fn problems(&self) -> Vec<crate::error::ViewProblem> {
        use crate::error::ViewProblem as P;
        let mut out = vec![];
        let path = |i: usize| -> String {
            let mut parts = vec![];
            let mut at = Some(i);
            while let Some(k) = at { parts.push(self.nodes[k].key.rsplit('/').next().unwrap_or("").to_owned()); at = self.nodes[k].parent; }
            parts.reverse();
            parts.join(" › ")
        };
        let mut first: HashMap<&str, usize> = HashMap::default();
        for (i, n) in self.nodes.iter().enumerate() {
            match first.get(n.key.as_str()) {
                Some(&j) => out.push(P::DuplicateKey { key: n.key.clone(), first: path(j), second: path(i) }),
                None => { first.insert(&n.key, i); }
            }
        }
        let mut flows: HashMap<&str, usize> = HashMap::default();
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some((f, root)) = &n.flow && *root == i {
                match flows.get(&**f) {
                    Some(&j) if !self.within(i, j) => out.push(P::FlowTwice { flow: f.to_string(), first: path(j), second: path(i) }),
                    Some(_) => {}
                    None => { flows.insert(f, i); }
                }
            }
        }
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(a) = &n.anchor && !self.nodes[..i].iter().any(|m| &m.key == a) { out.push(P::UnknownAnchor { key: n.key.clone(), anchor: a.clone() }); }
            if let Some(Scope::Container(Some(c))) = &n.ripple && (n.parent.is_none_or(|p| self.nodes[p].ripple != n.ripple)) && !first.contains_key(c.as_str()) {
                out.push(P::UnknownContainer { key: n.key.clone(), container: c.clone() });
            }
        }
        for n in self.nodes.iter().filter(|n| n.clamp > 0 && matches!(&n.content, Content::Text { around: Some(_), .. })) { out.push(P::ClampAround { key: n.key.clone() }); }
        for (key, need, min) in &self.fit_short { out.push(P::FitShort { key: key.clone(), need: *need, min: *min }); }
        for key in &self.plate_over { out.push(P::PlateOver { key: key.clone() }); }
        for key in &self.flow_own { out.push(P::FlowOwnSize { key: key.clone() }); }
        for (key, count) in &self.clear_many { out.push(P::ShowClearMany { key: key.clone(), count: *count }); }
        for n in &self.nodes {
            let read = n.legible && matches!(n.content, Content::Text { .. });
            if n.hidden || n.plate.is_some() || !(read || n.focusable || n.on_press.is_some()) { continue; }
            let mut at = n.parent;
            while let Some(k) = at { if self.nodes[k].plate.is_some() { out.push(P::OnPlate { key: n.key.clone(), plate: self.nodes[k].key.clone() }); break; } at = self.nodes[k].parent; }
        }
        // Own-size elements in a row or column do not shrink with the rest: one that reaches out of its
        // parent's content box at this scale.
        for n in self.nodes.iter().filter(|n| n.own_in_flow) {
            let Some(p) = n.parent else { continue };
            let c = self.nodes[p].inner;
            if n.rect.x < c.x - 0.5 || n.rect.y < c.y - 0.5 || n.rect.right() > c.right() + 0.5 || n.rect.bottom() > c.bottom() + 0.5 {
                out.push(P::OwnSizeOver { key: n.key.clone(), parent: self.nodes[p].key.clone() });
            }
        }
        // A ramp from one hue to another that is not over a picture keeps its first hue all the way.
        let pictures: Vec<[f32; 4]> = self.base[..self.page_len].iter().filter(|q| q.paint[3] == PICTURE && q.extra[0] <= 0.5)
            .map(|q| [q.rect[0].max(q.clip[0]), q.rect[1].max(q.clip[1]), (q.rect[0] + q.rect[2]).min(q.clip[2]), (q.rect[1] + q.rect[3]).min(q.clip[3])]).collect();
        let mut seen = HashSet::default();
        // A drawing's own colours (`Drawing::palette`) off a picture are hue 0 there.
        let mut own = HashSet::default();
        for (i, q) in self.base[..self.page_len].iter().enumerate() {
            let own_hue = q.paint[3] == SHAPE && q.paint[1] > 0.0 && q.look[1] > 2.5;
            let ramp = q.paint[3] == SHAPE && q.uv[2] < -0.5 && ramp_hues(q);
            if !own_hue && !ramp { continue; }
            // What of it is drawn (inside its clip): a mark that crosses the clip's edge is cut there.
            let b = q.bounds(0.0);
            let b = [b[0].max(q.clip[0]), b[1].max(q.clip[1]), b[2].min(q.clip[2]), b[3].min(q.clip[3])];
            if b[0] >= b[2] || b[1] >= b[3] { continue; }
            if pictures.iter().any(|p| b[0] >= p[0] - 0.5 && b[1] >= p[1] - 0.5 && b[2] <= p[2] + 0.5 && b[3] <= p[3] + 0.5) { continue; }
            let key = &self.nodes[self.owners[i].0].key;
            if own_hue && own.insert(key.clone()) { out.push(P::OwnColourOffPicture { key: key.clone() }); }
            if ramp && seen.insert(key.clone()) { out.push(P::RampHue { key: key.clone() }); }
        }
        out
    }

    /// How many instances of an old page are still dissolving.
    pub fn dissolving(&self) -> usize { self.ghosts.len() }
    /// Whether any part of a node is in a flow right now.
    pub fn flowing(&self) -> usize { self.flows.len() }

    // ── Frames ───────────────────────────────────────────────────────────────────────────────────
    /// Whether a frame has to be drawn at `now` (after `tick(now)`): input arrived or the scene changed
    /// since the last drawn frame, or something is moving (a front, a burst, condensing, a morph, a
    /// flow, the old page dissolving, the marks gliding, an annotation drawing on, a commit's front
    /// still on screen, a time-varying material with something loose), or a timed wake is due (the
    /// caret's blink, a clock the state keeps). When it is false the host draws nothing and waits for
    /// input or `next_wake`.
    pub fn needs_frame(&self, now: f32) -> bool {
        if self.paced_until(now).is_some() { return false; }
        let b = self.beat_of(now);
        self.dirty || self.clearing || self.stale_layout || self.stale_paint || self.owed || self.stage_due() || self.prep.pending() || self.held(now) || self.moving_at(b) || self.wake_at(b).is_some_and(|w| w <= b) || (self.sweeping && !self.arriving.is_empty())
            || (self.plates_ageing() && self.clock - self.plate_frame >= PLATE_FRAME - 1e-4)
            || self.soft_wake().is_some_and(|w| w <= now) || !self.kept_flips.is_empty() || !self.queued.is_empty()
    }
    /// Whether a scroll area's offset changed within `SOFT_STOP`: the renderer may shade the scroll
    /// areas softened (DESIGN.md §1.12).
    pub fn scroll_soft(&self) -> bool { self.motion.values().any(|m| self.clock - m.last < SOFT_STOP) }
    /// The renderer drew a frame with the scroll areas softened or not.
    pub fn soft_drawn(&mut self, soft: bool) { self.soft_shown = soft; }
    /// When the sharp frame after softened ones is due.
    fn soft_wake(&self) -> Option<f32> {
        self.soft_shown.then(|| self.motion.values().map(|m| m.last).fold(-1e9, f32::max) + SOFT_STOP + 1e-3)
    }
    /// A `Cmd::Hold` is going on: frames go on (the same) until it ends.
    fn held(&self, now: f32) -> bool { self.timing.beat.on() && self.bclock.hold.is_some_and(|(_, until)| now < until) }
    /// When the next frame is needed: `now` if one is needed now, a later time for a timed wake, `None`
    /// when only input can change anything.
    pub fn next_wake(&self, now: f32) -> Option<f32> {
        let b = self.beat_of(now);
        let wanted = self.dirty || self.clearing || self.owed || self.prep.pending() || self.held(now) || self.moving_at(b) || (self.sweeping && !self.arriving.is_empty());
        if wanted && let Some(t) = self.paced_until(now) { return Some(t); }
        if self.dirty || self.clearing || self.owed || self.stage_due() || self.prep.pending() || self.held(now) || self.moving_at(b) || (self.sweeping && !self.arriving.is_empty()) || !self.kept_flips.is_empty() || !self.queued.is_empty() { return Some(now); }
        let soft = self.soft_wake().map(|w| w.max(now));
        // The mirror's places, owed while a scroll went on, once it rests.
        let soft = if self.a11y_owed { let rest = self.motion.values().map(|m| m.last).fold(-1e9, f32::max) + SOFT_STOP + 1e-3; Some(soft.map_or(rest.max(now), |w| w.min(rest.max(now)))) } else { soft };
        if self.plates_ageing() { return Some((self.plate_frame + PLATE_FRAME).max(now).min(soft.unwrap_or(f32::MAX))); }
        // A wake on the beat clock is that much later on the runtime's.
        [self.wake_at(b).map(|w| (w + now - b).max(now)), self.stage_wake().map(|w| (w + now - b).max(now)), soft].into_iter().flatten().reduce(f32::min)
    }
    /// Prepare something over the next frames (DESIGN.md §1.23): `job` is called with the state in the
    /// time the frames leave (at least once a frame) until it says 1; each call does a small piece and
    /// says how far it is. Show the element it is for with `El::skeleton()` until then, measured to the
    /// size it will have, and read the progress from the state or with `Ui::ready`.
    pub fn prepare(&mut self, key: impl Into<String>, job: impl FnMut(&mut S) -> f32 + 'static) { self.prep.add(key.into(), Box::new(job)); self.dirty = true; }
    /// How far the preparation under `key` is, 0..1 (1: done, or never asked for).
    pub fn ready(&self, key: &str) -> f32 { self.prep.ready(key) }
    /// How ready the page is: the least ready preparation and the renderer's warm-up, 0..1.
    pub fn readiness(&self) -> f32 { self.prep.readiness() }
    /// The glyphs of the text `key` as the last paint drew them, after their poses, in the order drawn;
    /// a glyph its pose or a clip left out is not there. Empty when there is no such text.
    pub fn glyphs_drawn(&self, key: &str) -> Vec<GlyphDrawn> {
        let Some(node) = self.nodes.iter().position(|n| n.key == key) else { return vec![] };
        let s = self.scale;
        self.glyphs.log.iter().filter(|g| g.node == node).filter_map(|g| {
            let inst = self.scene.instances.get(g.inst)?;
            let b = crate::form::ink_box(inst);
            Some(GlyphDrawn { i: g.i, rect: Rect::new(b[0] / s, b[1] / s, (b[2] - b[0]) / s, (b[3] - b[1]) / s), alpha: inst.paint[2] })
        }).collect()
    }
    /// The glyphs of the last paint (`hit::Glyphs`), for hits and the audit.
    pub fn glyphs(&self) -> &crate::hit::Glyphs { &self.glyphs }
    /// Which node and identity each instance of the scene belongs to (the page's instances only).
    pub fn owners(&self) -> &[(usize, u64)] { &self.owners[..self.page_len.min(self.owners.len())] }
    /// The pieces (words or lines, logical px boxes) text node `i` is shown by under `reveal` (`El::reveal_by`).
    pub fn reveal_groups(&mut self, i: usize, reveal: crate::el::Reveal) -> Vec<Vec<Rect>> {
        let n = &self.nodes[i];
        match n.para() { Some(p) => self.text.reveal_groups(p, n.origin(), reveal), None => vec![] }
    }
    /// Something visible on the page moves on purpose (`El::ongoing`).
    pub fn ongoing(&self) -> bool { self.nodes.iter().any(|n| n.ongoing && !n.hidden && { let r = n.rect.intersect(&n.clip); r.w > 0.0 && r.h > 0.0 }) }
    /// Where the modal layer starts in the nodes, when one is open.
    pub fn modal_at(&self) -> Option<usize> { self.modal_at }
    /// How long preparation may take of each frame (ms; 0: each job once a frame, the same steps on
    /// every run, for tests and tools).
    pub fn set_prepare_budget(&mut self, ms: f32) { self.prep.budget_ms = ms.max(0.0); }
    /// How far the renderer's warm-up is (DESIGN.md §1.9): `Some` from the first frame of a host that
    /// warms up (`Renderer::warm_frame`) until the last piece is made, then `None`. A renderer made at
    /// once (`Renderer::new`) never says anything, so this stays `None`.
    pub fn warm(&self) -> Option<crate::render::Progress> { self.warm }
    /// The host says how far the warm-up is (`Renderer::warm_frame` does it); one that says
    /// `Stage::Ready` is `None`. A step asks for no frame of its own (the host steps the warm-up in idle
    /// time, `Renderer::warm_idle`): only the step after which the look's material can be drawn does
    /// (the page condenses into it), and one whose `on_warm` changed the state.
    pub fn set_warm(&mut self, p: Option<crate::render::Progress>) {
        let p = p.filter(|p| p.stage != crate::render::Stage::Ready);
        if p == self.warm { return; }
        let plain = |w: Option<crate::render::Progress>| w.is_some_and(|w| w.stage == crate::render::Stage::Plain);
        if plain(self.warm) && !plain(p) { self.dirty = true; }
        self.warm = p;
        if let Some(f) = self.warmed && f(&mut self.state, p) { self.relayout(); }
    }
    /// The renderer says how far its warm-up is (`render::Warmup`).
    pub fn set_render_ready(&mut self, f: f32) { if (self.prep.render - f).abs() > 1e-6 { self.prep.render = f; self.dirty = true; } }
    /// Everything on the page loosens and condenses again from now (the frame the look's material is
    /// ready after the warm-up, DESIGN.md §1.23): over `RAMP` each element loosens from 0 to its cap,
    /// then condenses as a new element does from there. Text to read and controls stay as they are (their
    /// cap on the ramp is 0: a nudge on 12 px type reads as missing strokes); titles, decoration and
    /// pictures come apart and condense. The frame the material arrives is the page at rest, so nothing
    /// jumps.
    pub fn condense_all(&mut self) {
        self.ramp = Some(self.clock);
        self.ramp_births();
    }
    /// The births that hold each element at its place on the ramp `condense_all` began.
    fn ramp_births(&mut self) {
        let Some(t0) = self.ramp else { return };
        let enter = self.timing.enter;
        let x = ((self.clock - t0) / RAMP).clamp(0.0, 1.0);
        // On the ramp, each frame's birth gives the looseness reached; past it, the birth from which
        // the element condenses from its cap.
        let at = if x < 1.0 { self.clock } else { t0 + RAMP };
        for &(owner, id) in &self.owners {
            // Text to read and controls do not move as the material arrives: they are born condensed.
            let n = &self.nodes[owner];
            let cap = if n.legible { 0.0 } else { response_caps(n, self.timing.legible_cap).1 };
            self.births.insert(id, at - enter * (1.0 - cap * x));
        }
        if x >= 1.0 { self.ramp = None; }
        self.paint();
    }
    /// Ask for a frame even though nothing changed (the host lost what it showed).
    pub fn invalidate(&mut self) { self.dirty = true; }
    /// The layouts since the last frame drawn, and where the first was asked for (`file:line` of the
    /// `Ui::relayout` call, or of the call that left it for the frame): for the readout.
    pub fn layouts_since_drawn(&self) -> (u32, Option<&'static std::panic::Location<'static>>) { self.laid }
    /// The host drew a frame of the current scene.
    pub fn frame_drawn(&mut self) {
        self.dirty = false;
        self.laid = (0, None);
        // The stages' scale goes by the frames drawn while stages are on the page.
        if let Some(&(t, c)) = self.perf.frames.back() && t != self.scaler_fed && self.governed && !self.scene.stages.is_empty() && self.stage_pin.is_none() {
            self.scaler_fed = t;
            let held = self.pointer_held();
            if self.scaler.feed(t, c.total(), self.scale, held) { self.dirty = true; }
        }
        self.owed = self.moving_at(self.beat_of(self.clock));
        self.plate_frame = self.clock;
        self.frame_t = self.wall;
    }
    /// Which node each plate this paint draws is in which slot: a plate keeps its slot while its element
    /// is there; a slot whose element has gone is emptied; beyond four plates, or twice the window's
    /// area, an element is drawn as a plain one (`Ui::problems`).
    fn plate_plan(&mut self) -> HashMap<usize, u8> {
        let mut out = HashMap::default();
        self.plate_over.clear();
        let window = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let mut area = 0.0;
        let wanted: Vec<(usize, String, f32)> = self.nodes.iter().enumerate().filter_map(|(i, n)| n.plate.map(|p| (i, n.key.clone(), p.fade_s))).collect();
        // Slots whose element has gone are emptied (a page that went takes its plates with it).
        for k in 0..crate::scene::PLATES {
            if let Some((key, _, _)) = &self.plate_slots[k] && !wanted.iter().any(|w| &w.1 == key) { self.plate_slots[k] = None; self.plate_clear.push(k as u8); }
        }
        for (i, key, fade) in wanted {
            let r = self.nodes[i].rect.intersect(&self.nodes[i].clip).intersect(&window);
            let a = r.w.max(0.0) * r.h.max(0.0);
            let slot = self.plate_slots.iter().position(|p| p.as_ref().is_some_and(|p| p.0 == key)).or_else(|| self.plate_slots.iter().position(Option::is_none));
            match slot {
                Some(k) if area + a <= 2.0 * window.w * window.h + 1e-3 => {
                    area += a;
                    // A slot newly taken starts empty, whatever was on it before.
                    if self.plate_slots[k].is_none() { self.plate_clear.push(k as u8); }
                    let last = self.plate_slots[k].as_ref().map_or(f32::NEG_INFINITY, |p| p.1);
                    self.plate_slots[k] = Some((key, last, fade));
                    out.insert(i, k as u8);
                }
                _ => self.plate_over.push(key),
            }
        }
        out
    }
    /// Whether each cached element has a kept image in this paint's plan, against what it was last told
    /// (`Ui::on_kept`; `Ui::kept_image`): the changes wait for the next frame's `flush`.
    fn note_kept(&mut self, plates: &HashMap<usize, u8>) {
        let now: Vec<(String, bool)> = self.nodes.iter().enumerate().filter(|(_, n)| n.cached).map(|(i, n)| (n.key.clone(), plates.contains_key(&i))).collect();
        // One that went while it had none is told it is not over the limit.
        let gone: Vec<String> = self.kept_told.keys().filter(|k| !now.iter().any(|(n, _)| n == *k)).cloned().collect();
        for k in gone { if self.kept_told.remove(&k) == Some(false) { self.kept_flips.push((k, true)); } }
        for (k, kept) in now {
            if self.kept_told.get(&k).copied().unwrap_or(true) != kept {
                if kept { self.kept_told.remove(&k); } else { self.kept_told.insert(k.clone(), false); }
                self.kept_flips.push((k, kept));
            }
        }
        if self.kept_to.is_none() { self.kept_flips.clear(); }
    }
    /// Node `i` is on a plate: it or an element above it has `El::plate`.
    fn on_plate(&self, i: usize) -> bool {
        let mut at = Some(i);
        while let Some(k) = at { if self.nodes[k].plate.is_some() { return true; } at = self.nodes[k].parent; }
        false
    }
    /// An effect turned off while it is in flight ends where it was going (DESIGN.md §1.34).
    fn cut_what_is_off(&mut self) {
        let off = self.timing.off;
        let mut cut = false;
        if off.morphs && self.run.is_some() { self.run = None; cut = true; }
        if off.flows && !self.flows.is_empty() { self.flows.clear(); self.flow_join = None; cut = true; }
        if off.recompose && !self.recomps.is_empty() { self.recomps.clear(); cut = true; }
        if off.decode && !self.decodes.is_empty() { cut = true; }
        if off.fronts && self.field.fronts.iter().any(|f| f[3] > 0.0) { for f in &mut self.field.fronts { f[3] = 0.0; } cut = true; }
        if cut { self.dirty = true; self.paint(); }
    }
    /// What the cached subtree of `root` draws from, as one number, or `None` when something it reads
    /// cannot be told the same by its nodes (a morph, a flow, faces changing; in it: a drawing, a picture,
    /// a 3D view, a stage, posed or selected text, a caret, a see-through group, something hidden, turned,
    /// fitted to its own size, that loosens by itself or is pressed, focused or recomposing). `ctx`: what
    /// its ancestors give it; `pull`: each node's pull past a scroll end; `ro`: the selected ranges.
    fn keep_sig(&self, root: usize, ctx: impl std::hash::Hash, pull: &[f32], ro: &HashMap<usize, (usize, usize)>) -> Option<u64> {
        use std::hash::{Hash, Hasher};
        if self.run.is_some() || !self.flows.is_empty() || self.sweeping || !self.swept_nodes.is_empty() || self.remeasure || !self.came.is_empty() || !self.arriving.is_empty() { return None; }
        let mut h = crate::fasthash::FastHasher::new();
        ctx.hash(&mut h);
        self.faces_added.hash(&mut h);
        self.no_recompose.hash(&mut h);
        for (k, j) in (root..self.nodes.len()).take_while(|&j| within(&self.nodes, j, root)).enumerate() {
            let n = &self.nodes[j];
            let plain = match &n.content {
                Content::None => true,
                Content::Text { pose, around, select, mark, caret, .. } => pose.is_none() && around.is_none() && select.is_none() && mark.is_none() && caret.is_none(),
                _ => false,
            };
            if !plain || n.hidden || n.opacity != 1.0 || n.calm || n.trace.is_some() || n.shed.is_some() || n.flow_field.is_some() || n.stirr.is_some() || n.wipe.is_some()
                || n.skeleton || n.form.is_some() || n.xf.is_some() || n.stage.is_some() || n.own_size.is_some() || n.view_scale.is_some() || n.modal.is_some()
                || n.show_clear.is_some() || n.flow.is_some() || n.focus_shown || n.scroll || !n.fill_on.none() || n.ripple.is_some() || (j != root && n.plate.is_some())
                || ro.contains_key(&j) || self.recomps.contains_key(&n.key) || self.decodes.contains_key(&n.key)
                || self.focus.as_deref() == Some(n.key.as_str()) || self.pressed.as_deref() == Some(n.key.as_str()) || self.modal_at == Some(j) { return None; }
            let nums = [n.rect.x, n.rect.y, n.rect.w, n.rect.h, n.clip.x, n.clip.y, n.clip.w, n.clip.h, n.clip_r, n.opacity_all, n.mat, n.loose, n.loosen, n.radius, pull.get(k).copied().unwrap_or(0.0)];
            sig_node(&mut h, &n.key, nums, (n.disabled, n.legible, n.recompose, n.recompose_always, n.reveal_by, n.clamp, &n.shown), n.fill, n.border, n.respond, &n.content);
        }
        Some(h.finish())
    }
    /// A cached subtree has been painted (`keep_open`): what it drew besides its instances is kept for a
    /// paint that passes it by, when what it drew from can be told the same and it added to no list that
    /// is not kept (`lens`). `more`: the field's calm boxes and the drawings' colours now.
    fn keep_close(&mut self, o: KeepOpen, births: &HashMap<u64, f32>, text_births: &HashMap<u64, f32>, jobs: &[RecJob], more: [usize; 2]) {
        let k = o.slot as usize;
        let now = [self.unseen.len(), self.scene.groups.len(), self.scene.meshes.len(), o.lens[3], o.lens[4], more[0], more[1]];
        let (Some(sig), true) = (o.sig, now == o.lens) else { self.plate_keep[k] = None; return };
        let keys: Vec<&str> = (o.root..self.nodes.len()).take_while(|&j| within(&self.nodes, j, o.root)).map(|j| self.nodes[j].key.as_str()).collect();
        let jobs: Vec<(usize, &str)> = jobs.iter().map(|j| (j.node, self.nodes[j.node].key.as_str())).collect();
        let keep = PlateKeep::take(sig, self.timing, &self.scene.instances[o.start..], &self.owners[o.start..], births, text_births, &jobs, &self.glyphs.laid, o.root, &keys);
        self.plate_keep[k] = Some(keep);
    }
    /// The plate in `slot` drew the instances from `start` to now (the subtree of `root`).
    fn plate_span(&mut self, root: usize, slot: u8, start: u32) {
        // A cached subtree (`El::cached`) that draws what its plate holds draws nothing: the plate lays on
        // what it kept. One that draws something else replaces what the plate holds (where the frame
        // draws: a partial frame moves the plate with a scroll and inks its boxes); with ink that ages,
        // it is drawn onto an emptied plate. One copied from the last paint with nothing in it (a
        // restage) is as it was.
        let fade = self.plate_slots[slot as usize].as_ref().map_or(0.0, |p| p.2);
        let replace = self.nodes[root].cached && fade == 0.0;
        if self.nodes[root].cached {
            let (a, b) = (start as usize, self.scene.instances.len());
            if b > a {
                use std::hash::{Hash, Hasher};
                let mut h = crate::fasthash::FastHasher::new();
                bytemuck::cast_slice::<Instance, u8>(&self.scene.instances[a..b]).hash(&mut h);
                let sig = h.finish();
                let k = slot as usize;
                if self.plate_sig[k] == Some(sig) && !self.plate_clear.contains(&slot) {
                    self.scene.truncate(a);
                    self.owners.truncate(a);
                    self.tags.truncate(a);
                    self.glyphs.log.retain(|g| g.inst < a);
                } else {
                    self.plate_sig[k] = Some(sig);
                    if !replace && !self.plate_clear.contains(&slot) { self.plate_clear.push(slot); }
                }
            }
        }
        let end = self.scene.instances.len() as u32;
        if end > start && let Some(p) = &mut self.plate_slots[slot as usize] { p.1 = self.pclock; }
        self.scene.plates.push(crate::scene::PlateSpan { slot, start, end, fade_s: fade, replace });
    }
    /// The renderer has not got what the cached subtrees in `slots` drew into their plates (a renderer
    /// made after they were painted, a new size): they are drawn again in the next paint, now. Whether
    /// any was.
    pub fn plates_lost(&mut self, slots: &[u8]) -> bool {
        let mut any = false;
        for &s in slots { if let Some(sig) = self.plate_sig.get_mut(s as usize) && sig.take().is_some() { any = true; } }
        if any { self.paint(); }
        any
    }
    /// A plate's ink is ageing (`Plate::fade_s`): frames at 30 a second at most until the last has gone.
    fn plates_ageing(&self) -> bool { self.plate_slots.iter().flatten().any(|(_, last, fade)| *fade > 0.0 && self.pclock < last + fade + PLATE_FRAME) }
    /// Frame times and work counters (the renderer closes each frame with `perf_mut().frame(..)`).
    pub fn perf(&self) -> &crate::perf::Perf { &self.perf }
    /// The performance counters, to change (a shell sets the backend and adapter names).
    pub fn perf_mut(&mut self) -> &mut crate::perf::Perf { &mut self.perf }
    /// The work counters (layout passes, paragraphs shaped, views, scenes).
    pub fn counters(&self) -> crate::perf::Counters { let mut c = self.perf.counters; c.shaped = self.text.shaped(); c.reveals = self.text.reveals(); c }
    fn span_begin(&mut self) -> (f64, f32) { self.span += 1; (crate::perf::now_ms(), self.perf.pending.scene) }
    fn span_end(&mut self, (t0, scene0): (f64, f32)) {
        self.span -= 1;
        if self.span == 0 {
            let spent = (crate::perf::now_ms() - t0) as f32 - (self.perf.pending.scene - scene0);
            self.perf.pending.update_layout += spent.max(0.0);
            let sm = self.text.shape_ms();
            self.perf.pending.text += (sm - self.shape_seen) as f32;
            self.shape_seen = sm;
        }
    }
    /// The clock that materials' time terms read: it runs only while something moves.
    pub fn material_clock(&self) -> f32 { self.mclock }
    /// The ground clock (the dottr ground's drift): it runs only while something moves the whole window
    /// (a commit, a front that reaches everywhere, a morph, a flow, a fling or a pull; not a title flying
    /// from the last frame).
    pub fn ground_clock(&self) -> f32 { self.gclock }
    /// Set the ground clock (a check that needs one exact frame).
    pub fn set_ground_clock(&mut self, t: f32) { self.gclock = t; self.dirty = true; }
    /// Set the material clock (for tools that compare two runtimes whose pasts differ, such as `check`).
    pub fn set_material_clock(&mut self, t: f32) { self.mclock = t; self.dirty = true; }
    /// Is anything animating at `now`? (Not counting input or timed wakes.)
    pub fn moving_at(&self, now: f32) -> bool {
        let tm = &self.timing;
        let fronts = self.field.fronts.iter().any(|f| f[3] > 0.0 && now - f[2] < tm.wave_life);
        let split = self.split.as_ref().is_some_and(|sp| self.split_alive(sp, now));
        fronts || split || self.ramp.is_some() || self.field.burst_at(now) > 0.0 || now < self.latest_birth + tm.enter
            || self.run.is_some() || !self.flows.is_empty() || !self.ghosts.is_empty() || self.animating
            || self.marks_moving || now < self.gaze_until || self.fling.is_some() || !self.over.is_empty()
            || self.static_loose || !self.recomps.is_empty() || !self.slides.is_empty() || self.motion_alive(now)
            || self.motes_moving() || self.decodes.values().any(|d| d.spec.hold.is_none()) || self.fmap.moving(&tm.map)
    }
    /// Whether some scroll node's trace (DESIGN.md §1.8.3) is still showing or settling at `now`.
    fn motion_alive(&self, now: f32) -> bool {
        let settle = self.timing.scroll.settle_ms.max(0.0) / 1000.0;
        self.motion.values().any(|m| m.v != 0.0 && now - m.last <= settle + 1.0 / 60.0)
    }
    /// How fast scroll node `key` moves now (logical px a second, smoothed; 0 at rest) and how strong its
    /// trace is (0..1: eased in from `Scroll::calm_speed` to `full_speed`, settled `settle_ms` after it stops).
    pub fn scroll_motion(&self, key: &str) -> (f32, f32) {
        let Some(m) = self.motion.get(key) else { return (0.0, 0.0) };
        let sc = &self.timing.scroll;
        let settle = sc.settle_ms.max(1.0) / 1000.0;
        let since = (self.clock - m.last).max(0.0);
        if since > settle { return (0.0, 0.0); }
        let x = since / settle;
        (m.v, sc.strength(m.v) * (1.0 - x * x * (3.0 - 2.0 * x)))
    }
    /// Follow every scroll node's motion over `dt` seconds (its speed from how far its offset went).
    fn track_motion(&mut self, dt: f32) {
        if dt <= 0.0 { return; }
        let now = self.clock;
        let a = 1.0 - (-dt / SPEED_TAU).exp();
        let settle = self.timing.scroll.settle_ms.max(0.0) / 1000.0;
        for n in self.nodes.iter().filter(|n| n.scroll) {
            let off = self.scroll.get(&n.key).copied().unwrap_or(0.0);
            let m = self.motion.entry(n.key.clone()).or_insert(Motion { off, v: 0.0, last: -1e9 });
            let raw = (off - m.off) / dt;
            m.off = off;
            if raw != 0.0 { m.v += (raw - m.v) * a; m.last = now; }
            else if now - m.last > settle { m.v = 0.0; }
        }
        let nodes = &self.nodes;
        self.motion.retain(|k, _| nodes.iter().any(|n| n.scroll && &n.key == k));
        self.scroll_field();
    }
    /// The scroll nodes the materials see (DESIGN.md §1.8.3): up to eight visible ones, the largest
    /// first, each its viewport (device px), its content offset (device px, whole pixels: the lattices
    /// are fixed to the content), how strong its trace is and which way its content moves on screen.
    fn scroll_field(&mut self) {
        let s = self.scale;
        let mut v: Vec<(f32, [f32; 4], [f32; 4])> = vec![];
        for n in self.nodes.iter().filter(|n| n.scroll) {
            let r = n.clip.intersect(&n.rect);
            if r.w <= 0.0 || r.h <= 0.0 { continue; }
            let off = (self.scroll.get(&n.key).copied().unwrap_or(0.0) * s).round();
            let (speed, k) = self.scroll_motion(&n.key);
            // The content moves against the offset: a growing offset moves it up (or left).
            let dir = if speed > 0.0 { -1.0 } else { 1.0 };
            let (ox, oy, d) = if n.horizontal { (off, 0.0, dir * 2.0) } else { (0.0, off, dir) };
            v.push((r.w * r.h, [r.x * s, r.y * s, r.right() * s, r.bottom() * s], [ox, oy, k, d]));
        }
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.field.scrolls = [[0.0; 4]; 8];
        self.field.scrollo = [[0.0; 4]; 8];
        for (j, (_, r, o)) in v.into_iter().take(8).enumerate() { self.field.scrolls[j] = r; self.field.scrollo[j] = o; }
    }
    /// Whether a commit is still showing: its transition is running, or (without one) its front is still
    /// crossing the window.
    fn split_alive(&self, sp: &Split, now: f32) -> bool {
        let tm = &self.timing;
        let (w, h) = (self.size.0 * self.scale, self.size.1 * self.scale);
        if tm.transition > 0.0 { return now - sp.t < tm.transition * sp.span(w, h) + 1.0 / 60.0; }
        (now - sp.t) * tm.wave_speed * self.scale < (w * w + h * h).sqrt() + 1.0
    }
    /// What moves the material clock: the field and what loosens (fronts, a commit, condensing, morphs,
    /// flows, recomposition, a fling or a pull, loose things under a time material). Not the hover's
    /// burst, the focus marks gliding or the annotation layer drawing on: those change a few pixels, and
    /// a clock that moved would change the dottr ground under the whole window (DESIGN.md §1.19).
    fn field_moving_at(&self, now: f32) -> bool {
        let tm = &self.timing;
        let fronts = self.field.fronts.iter().any(|f| f[3] > 0.0 && now - f[2] < tm.wave_life);
        let split = self.split.as_ref().is_some_and(|sp| self.split_alive(sp, now));
        fronts || split || now < self.latest_birth + tm.enter || self.run.is_some() || !self.flows.is_empty() || !self.ghosts.is_empty()
            || self.fling.is_some() || !self.over.is_empty() || self.static_loose || !self.recomps.is_empty() || !self.slides.is_empty()
            // The field map loosens things too (DESIGN.md §1.29): its material clock runs while it moves.
            || self.fmap.moving(&tm.map)
    }
    /// What moves the ground clock: what acts on the whole window. A front scoped to one control or
    /// one area does not (DESIGN.md §1.19).
    fn ground_moving_at(&self, now: f32) -> bool {
        let tm = &self.timing;
        let everywhere = self.field.fronts.iter().zip(&self.field.scopes).any(|(f, sc)| f[3] > 0.0 && now - f[2] < tm.wave_life && *sc == EVERYWHERE);
        let split = self.split.as_ref().is_some_and(|sp| self.split_alive(sp, now));
        // A title flying from the last frame moves only itself.
        let flows = self.flows.values().any(|f| f.src.is_none() && f.fade_in.is_none()) || self.ghosts.iter().any(|g| g.loosen);
        everywhere || split || self.run.is_some() || flows || self.fling.is_some() || !self.over.is_empty()
    }
    /// About how much of the window (0..1) can be loose at `now`: all of it while a front lives, things
    /// condense, a morph or flow runs; otherwise the area of instances held loose, a burst, the lens over
    /// pictures and a compare wipe. The renderer uses it to decide whether a reduced material pass pays.
    pub fn loose_estimate(&self, now: f32, lens_radius: f32) -> f32 {
        let tm = &self.timing;
        if self.field.fronts.iter().any(|f| f[3] > 0.0 && now - f[2] < tm.wave_life) || now < self.latest_birth + tm.enter
            || self.run.is_some() || !self.flows.is_empty() || !self.ghosts.is_empty() { return 1.0; }
        let s = self.scale;
        let screen = (self.size.0 * s * self.size.1 * s).max(1.0);
        let clip = |r: [f32; 4]| { let w = (r[0] + r[2]).min(self.size.0 * s) - r[0].max(0.0); let h = (r[1] + r[3]).min(self.size.1 * s) - r[1].max(0.0); w.max(0.0) * h.max(0.0) };
        let mut area: f32 = self.scene.instances[..self.page_len].iter().filter(|i| i.extra[2] > 0.0).map(|i| clip(i.rect)).sum();
        if self.field.burst_at(now) > 0.0 { let b = self.field.burst; area += clip([b[0], b[1], b[2] - b[0], b[3] - b[1]]); }
        let pictures = self.scene.instances.iter().any(|i| i.paint[3] == PICTURE);
        if pictures && self.field.pointer[2] > 0.0 { area += std::f32::consts::PI * (lens_radius * s * 1.5).powi(2); }
        if self.field.wipe[1] > 0.0 { let r = self.field.wipe_rect; area += (r[2] - self.field.wipe[0]).max(0.0) * (r[3] - r[1]).max(0.0); }
        (area / screen).min(1.0)
    }
    /// The next timed wake: the caret's next blink, or when the state asked for a tick.
    fn wake_at(&self, now: f32) -> Option<f32> {
        let blink = self.caret.map(|(_, reset, _)| reset + ((now - reset) / BLINK).floor().max(0.0) * BLINK + BLINK);
        let asked = self.wake_of.and_then(|f| f(&self.state, now));
        // The readout never keeps frames coming: it changes only when a frame is drawn anyway, and once,
        // a second after the last one, to say the page is idle.
        let readout = (self.overlay > 0 && !self.hud_idle).then_some(self.overlay_at + 1.05);
        [blink, asked, readout].into_iter().flatten().reduce(f32::min)
    }

    // ── Input ────────────────────────────────────────────────────────────────────────────────────
    /// The window is `w` × `h` device px at `scale` device px per logical px: lay out again.
    pub fn resize(&mut self, w: u32, h: u32, scale: f32) {
        self.dirty = true;
        self.scale = scale;
        self.text.set_scale(scale);
        let size = (w.max(1) as f32 / scale, h.max(1) as f32 / scale);
        // A morph's tiles are in device px of the old size.
        if size != self.size {
            self.run = None;
            self.scaler.reset();
            // A new window size empties every plate.
            self.plate_clear.extend(self.plate_slots.iter().enumerate().filter(|(_, p)| p.is_some()).map(|(k, _)| k as u8));
        }
        self.size = size;
        if let Some(f) = self.sized { f(&mut self.state, self.size.0, self.size.1); }
        self.relayout();
    }

    /// Advance the clock to `t` seconds; the marks glide toward their target.
    pub fn tick(&mut self, t: f32) {
        crate::budget::begin_frame();
        let scenes0 = self.perf.counters.scenes;
        let wall = t;
        if t > self.wall && t - self.wall < 0.1 { self.tick_gap = t - self.wall; }
        self.wall = t;
        self.bclock.step(t);
        // A hold that has ended presents the page as the state has it now.
        if !self.held(t) { self.frozen = None; }
        // Everything that moves on its own runs on the beat clock (the runtime's own without a beat).
        let held_now = self.held(t);
        let t = self.beat_of(t);
        let dt = (t - self.clock).clamp(0.0, 0.1);
        // The plate clock stops while a hold lasts and never runs back (a rewind keeps the ink).
        if !held_now { self.pclock += dt; }
        self.scene.plate_now = self.pclock;
        // Each frame draws a plate's ink again (and stamps it with the plate clock), so a plate with ink
        // in the scene was drawn now, painted or not.
        for p in &self.scene.plates { if p.end > p.start && let Some(s) = &mut self.plate_slots[p.slot as usize] { s.1 = self.pclock; } }
        self.scene.plate_ageing = self.plates_ageing();
        // The material clock runs only while something was moving.
        if self.moving && !self.timing.off.material_still { self.mclock += dt; }
        if self.ground_moving && !self.timing.off.material_still { self.gclock += dt; }
        self.clock = t;
        if self.look_of.is_some() { let l = self.look(); self.timing = Timing::of(&l); self.ground_seen = Some((l.theme.ground, l.theme.ground2)); }
        self.cut_what_is_off();
        self.ramp_births();
        // What the state's tick changed is laid out: once, before the frame is drawn, when input is
        // coalesced (a host that ticks before each event does not lay out for each).
        self.deferred(|ui| {
            let page = ui.ticked.is_some_and(|f| f(&mut ui.state, wall));
            let stepped = ui.stepped.map(|f| f(&mut ui.state, wall)).unwrap_or_default();
            if page { ui.relayout(); } else if !stepped.is_empty() { ui.restage(stepped); }
            // Preparation in the spare time (`Ui::prepare`): what it changed is shown.
            if ui.prep.run(&mut ui.state) { ui.relayout(); }
        });
        // Faces that came since the last tick change together, in one layout; a frame drawn already
        // showed their stand-ins, so they sweep in (DESIGN.md §1.6.5).
        if !self.came.is_empty() {
            let shown = self.perf.drawn > 0;
            for (f, d) in std::mem::take(&mut self.came) { if shown { self.arriving.insert(f, (t, d)); } }
            // Before any frame the atlas starts over, so it holds what a page that had the faces from
            // the start holds, where it holds it.
            if !shown { self.text.refill(); }
            self.remeasure = true;
            self.relayout();
        }
        let sweep_end = if self.fmap.reduced { REDUCED_FADE } else { SWEEP + SWEEP_LOOSE };
        let n = self.arriving.len();
        self.arriving.retain(|_, (t0, _)| t < *t0 + sweep_end);
        if self.arriving.len() != n { self.dirty = true; }
        let before = (self.marks.at, self.marks.alpha);
        self.aim_marks();
        self.marks_moving = before != (self.marks.at, self.marks.alpha) || self.marks_settling();
        // The caret blinks in whole phases from when it last moved.
        if let Some((_, reset, _)) = self.caret {
            let on = ((t - reset).max(0.0) / BLINK).floor() as i64 % 2 == 0;
            if on != self.caret_on { self.caret_on = on; self.dirty = true; }
        }
        // A morph moving into its next phase changes what is drawn; one that is over is dropped and the
        // view decides again.
        self.reproject();
        let repaint = match &self.run {
            Some(r) if t >= r.end => { self.run = None; true }
            Some(r) => r.phase(t) != self.painted_phase,
            None => false,
        };
        // (A flight is over at its end, to within a tenth of a millisecond of the clock's steps.)
        self.flows.retain(|_, f| t < f.start + f.dur - 1e-4);
        if self.flows.is_empty() && self.ghosts.iter().all(|g| g.ride.is_none()) { self.grows.clear(); }
        self.recomps.retain(|_, r| t < r.end);
        self.slides.retain(|_, sl| t < sl.start + sl.dur);
        let scrolled = self.step_scrolling(dt);
        if self.step_motes(dt) { self.dirty = true; }
        if self.step_map(dt) { self.dirty = true; }
        // The readout: switched on or off, or a second past the last frame (it then says idle, once).
        let hud = self.timing.overlay != self.overlay || (self.overlay > 0 && !self.hud_idle && t >= self.overlay_at + 1.05);
        let sweep = !self.arriving.is_empty() && self.sweeping;
        let due = if repaint || scrolled || hud || sweep || self.clearing { vec![] } else { self.stages_due() };
        if repaint || scrolled || hud || sweep || self.clearing { self.paint(); }
        else if !due.is_empty() && (self.perf.counters.scenes == scenes0 || due.iter().any(|k| self.held_steps.contains(k))) {
            // A stage that held draws when it may, alone where it can be viewed alone; nothing else is drawn
            // again for it, and between its draws it costs nothing.
            if self.stage_of.is_some() { let sp = self.span_begin(); self.restage(due); self.span_end(sp); } else { self.paint(); }
        } else { self.finish(); }
        // The frame is drawn: input from here on is the next frame's (for the stages). Input whose layout
        // waits for the frame (coalesced, `Ui::settle`) is this frame's still: the paint that lays it out
        // reads it, and only then is it spent.
        if !self.stale() { self.stage_input = (false, false); }
        self.track_motion(dt);
        if self.marks_moving { self.dirty = true; }
        self.moving = self.field_moving_at(t);
        self.ground_moving = self.ground_moving_at(t);
        self.flush();
    }

    /// A fling going on and pulls springing back (it paints what moved; the result is whether the caller
    /// still has to).
    fn step_scrolling(&mut self, dt: f32) -> bool {
        let mut moved = false;
        if let Some((key, v)) = self.fling.clone() {
            let (d, v2) = fling_step(v, dt, self.scrolling.friction);
            match self.nodes.iter().position(|n| n.key == key) {
                Some(i) if v2.abs() >= 20.0 => {
                    if self.scroll_by(i, d) { self.fling = Some((key, v2)); }
                    else {
                        // It reached the end: what is left of its speed pulls a little past it.
                        let o = self.over.get(&key).copied().unwrap_or(0.0);
                        self.over.insert(key, rubber_band(o, v2 * 0.04, self.scrolling.rubber, self.scrolling.max_over));
                        self.fling = None;
                    }
                    moved = true;
                }
                _ => self.fling = None,
            }
        }
        let held = match &self.touch { Some(Touch { mode: TouchMode::Scroll(k), .. }) => Some(k.clone()), _ => None };
        let k = (-self.scrolling.spring * dt).exp();
        let before = self.over.len();
        self.over.retain(|key, o| { if held.as_deref() != Some(key.as_str()) { *o *= k; } o.abs() >= 0.5 });
        moved |= !self.over.is_empty() || before != self.over.len();
        // Moved: placed again (a virtual list builds the rows a fling brings into view) and painted.
        if moved { self.scrolled(); }
        false
    }

    // ── Pointers of every kind (DESIGN.md §1.8.2) ──────────────────────────────────────────────────
    /// A pointer pressed: a mouse or a pen presses at once; a finger shows the press (a front, the
    /// control held) but only sends it when it lifts without having scrolled — unless what it is on is
    /// dragged (a slider) or is a text field, or there is nothing to scroll, which it presses at once.
    pub fn pointer_down(&mut self, id: u64, kind: PointerKind, x: f32, y: f32, shift: bool) {
        self.settle();
        self.ev = self.event_t.take().unwrap_or(self.clock);
        if kind != PointerKind::Touch { self.touching = false; self.press_with(x, y, shift); return; }
        self.dirty = true;
        self.touches.insert(id);
        self.fling = None;
        if self.touches.len() > 1 {
            // A second finger (a pinch): what the first was doing is dropped, and nothing is sent.
            let direct = self.touch.as_mut().map(|t| (std::mem::replace(&mut t.mode, TouchMode::Cancelled) == TouchMode::Direct, t.last));
            if let Some((true, (lx, ly))) = direct { self.release(lx, ly); }
            if let Some(k) = self.pressed.take() { self.rebirth(&k); }
            return;
        }
        self.touching = true;
        self.hover = None;
        let hit = self.hit_target(x, y);
        // A scroll area inside a dragged element (a sheet's body) scrolls first: its drag waits for a pull
        // past the scroll's start (the hand-off in `drag_scroll`).
        let dragged = hit.and_then(|i| self.up(i, |n| n.on_drag.is_some() || n.on_text.is_some() || n.on_press.is_some()))
            .filter(|&i| self.nodes[i].on_drag.is_some() || self.nodes[i].on_text.is_some());
        let scroller = hit.and_then(|i| self.up(i, |n| n.scroll));
        let inner_scroll = matches!((dragged, scroller), (Some(d), Some(sc)) if self.nodes[d].on_text.is_none() && self.within(sc, d) && sc != d);
        let direct = (dragged.is_some() && !inner_scroll) || scroller.is_none();
        let mode = if direct { self.press_with(x, y, shift); TouchMode::Direct } else {
            self.heard();
            let ctl = hit.and_then(|i| self.up(i, Node::active));
            self.front_from(ctl.or(hit), x, y, ctl.is_some(), 1.0);
            self.pressed = hit.and_then(|i| self.up(i, |n| n.on_press.is_some())).map(|i| self.nodes[i].key.clone());
            if let Some(i) = hit.and_then(|i| self.up(i, |n| n.on_press.is_some())) { let r = self.nodes[i].rect; self.burst(r, 1.0, self.timing.press); }
            self.paint();
            TouchMode::Pending
        };
        let at = self.ev;
        self.touch = Some(Touch { id, start: (x, y), last: (x, y), samples: [(at, x, y)].into(), mode });
    }
    /// The time (on the runtime's clock) the next pointer event happened, when the host knows it; a
    /// finger's speed is measured from these.
    pub fn event_time(&mut self, t: f32) { self.event_t = Some(self.beat_of(t)); }
    /// A pointer moved (a mouse or pen also hovers).
    pub fn pointer_moved(&mut self, id: u64, kind: PointerKind, x: f32, y: f32) { self.stage_input.0 = true; self.deferred(|ui| ui.pointer_moved_now(id, kind, x, y)) }
    fn pointer_moved_now(&mut self, id: u64, kind: PointerKind, x: f32, y: f32) {
        let now = self.event_t.take().unwrap_or(self.clock);
        self.ev = now;
        if !self.scripted { self.input_t = self.wall; }
        if kind != PointerKind::Touch { self.touching = false; self.pointer_move_now(x, y); return; }
        let Some(t) = &mut self.touch else { return };
        if t.id != id || t.mode == TouchMode::Cancelled { return; }
        t.samples.push_back((now, x, y));
        while t.samples.len() > 2 && t.samples.front().is_some_and(|s| now - s.0 > 0.1) { t.samples.pop_front(); }
        let (last, start) = (t.last, t.start);
        t.last = (x, y);
        match t.mode.clone() {
            TouchMode::Direct => self.pointer_move_now(x, y),
            TouchMode::Pending => {
                let (dx, dy) = (x - start.0, y - start.1);
                if dx.hypot(dy) < self.scrolling.slop { return; }
                // Now it is a scroll: of the nearest scroll ancestor along the way the finger went.
                let vertical = dy.abs() >= dx.abs();
                let at = self.hit(start.0, start.1);
                let node = at.and_then(|i| self.up(i, |n| n.scroll && n.horizontal != vertical)).or_else(|| at.and_then(|i| self.up(i, |n| n.scroll)));
                if let Some(k) = self.pressed.take() { self.rebirth(&k); }
                let Some(i) = node else { if let Some(t) = &mut self.touch { t.mode = TouchMode::Cancelled; } return };
                let key = self.nodes[i].key.clone();
                if let Some(t) = &mut self.touch { t.mode = TouchMode::Scroll(key.clone()); }
                let d = if self.nodes[i].horizontal { -(x - last.0) } else { -(y - last.1) };
                if !self.hand_off(i, d, x, y) { self.drag_scroll(i, d); }
            }
            TouchMode::Scroll(key) => {
                let Some(i) = self.nodes.iter().position(|n| n.key == key) else { return };
                let d = if self.nodes[i].horizontal { -(x - last.0) } else { -(y - last.1) };
                if !self.hand_off(i, d, x, y) { self.drag_scroll(i, d); }
            }
            TouchMode::Cancelled => {}
        }
    }
    /// A finger pulling vertical scroll node `i` back past its start (`d` < 0 at offset 0, nothing pulled)
    /// while an element around it is dragged (a sheet) hands the gesture to that element: its drag
    /// starts here and the finger is then its own (DESIGN.md §1.8.2, hand-off). Returns whether it did.
    fn hand_off(&mut self, i: usize, d: f32, x: f32, y: f32) -> bool {
        let n = &self.nodes[i];
        if n.horizontal || d >= 0.0 || self.scroll.get(&n.key).copied().unwrap_or(0.0) > 0.0 || self.over.get(&n.key).is_some_and(|o| *o != 0.0) { return false; }
        let Some(p) = n.parent.and_then(|p| self.up(p, |m| m.on_drag.is_some())) else { return false };
        let Some(handler) = self.nodes[p].on_drag.clone() else { return false };
        // The drag starts where the finger was a step ago, so this step already moves it.
        let (x0, y0) = (x, y + d);
        self.capture = Some(Capture { key: self.nodes[p].key.clone(), handler, start: (x0, y0), rect: self.nodes[p].rect });
        self.drag(Phase::Start, x0, y0);
        self.drag(Phase::Move, x, y);
        if let Some(t) = &mut self.touch { t.mode = TouchMode::Direct; }
        true
    }
    /// Move scroll node `i` with the finger by `d`; past an end, pull instead.
    fn drag_scroll(&mut self, i: usize, d: f32) {
        self.dirty = true;
        let key = self.nodes[i].key.clone();
        let o = self.over.get(&key).copied().unwrap_or(0.0);
        // A pull is given back first, then the content moves.
        let back = if o != 0.0 && o.signum() != d.signum() { d.abs().min(o.abs()) * d.signum() } else { 0.0 };
        if back != 0.0 { self.over.insert(key.clone(), o + back); }
        let rest = d - back;
        if rest != 0.0 && !self.scroll_by(i, rest) {
            let o = self.over.get(&key).copied().unwrap_or(0.0);
            self.over.insert(key, rubber_band(o, rest, self.scrolling.rubber, self.scrolling.max_over));
        }
        self.scrolled();
    }
    /// A pointer let go. A finger that scrolled flings on at its speed (at most `max_velocity`); one that
    /// did not is a tap: the press is sent now.
    pub fn pointer_up(&mut self, id: u64, kind: PointerKind, x: f32, y: f32) {
        if !self.scripted { self.input_t = self.wall; }
        self.settle();
        self.ev = self.event_t.take().unwrap_or(self.clock);
        if kind != PointerKind::Touch { self.release(x, y); return; }
        self.dirty = true;
        self.touches.remove(&id);
        let Some(t) = self.touch.take_if(|t| t.id == id) else { return };
        self.touching = false;
        match t.mode {
            TouchMode::Direct => { self.touching = true; self.release(x, y); self.touching = false; }
            TouchMode::Pending => {
                if let Some(k) = self.pressed.take() { self.rebirth(&k); }
                self.touching = true;
                self.press_with(x, y, false);
                self.release(x, y);
                self.touching = false;
            }
            TouchMode::Scroll(key) => {
                let (a, b) = (t.samples.front().copied(), t.samples.back().copied());
                if let (Some(a), Some(b)) = (a, b) && b.0 - a.0 > 1e-3 {
                    let horizontal = self.nodes.iter().find(|n| n.key == key).is_some_and(|n| n.horizontal);
                    let v = -if horizontal { b.1 - a.1 } else { b.2 - a.2 } / (b.0 - a.0);
                    let v = v.clamp(-self.scrolling.max_velocity, self.scrolling.max_velocity);
                    if v.abs() > 50.0 && !self.over.contains_key(&key) { self.fling = Some((key, v)); }
                }
            }
            TouchMode::Cancelled => {}
        }
        self.hover = None;
    }
    /// A pointer the system took away (a gesture of its own): nothing is sent.
    pub fn pointer_cancel(&mut self, id: u64) {
        self.settle();
        self.dirty = true;
        self.touches.remove(&id);
        let Some(t) = self.touch.take_if(|t| t.id == id) else { return };
        if let Some(k) = self.pressed.take() { self.rebirth(&k); }
        if t.mode == TouchMode::Direct { self.release(t.last.0, t.last.1); }
        self.touching = false;
    }
    /// How far a scroll node is pulled past its end right now (negative: past its start).
    pub fn overscroll(&self, key: &str) -> f32 { self.over.get(key).copied().unwrap_or(0.0) }
    /// Whether a fling is going on.
    pub fn flinging(&self) -> bool { self.fling.is_some() }

    /// Start again at `t`: everything on the page condenses in from loose.
    pub fn restart(&mut self, t: f32) {
        self.wall = t;
        self.bclock = crate::beat::Clock::default();
        self.clock = t;
        for b in self.births.values_mut() { *b = t; }
        self.paint();
    }

    /// Logical px.
    pub fn pointer_move(&mut self, x: f32, y: f32) { self.stage_input.0 = true; self.ev = self.event_t.take().unwrap_or(self.clock); self.deferred(|ui| ui.pointer_move_now(x, y)) }
    fn pointer_move_now(&mut self, x: f32, y: f32) {
        if !self.scripted { self.input_t = self.wall; }
        let cursor = self.cursor();
        // A move asks for a frame when something shows where the pointer is: the lens over pictures and 3D
        // views, a morph's particles, the annotation layer, a cursor of another shape, a drag, a selection, a
        // new hover. Handlers that answer it ask for their own. Over a page that shows none of these, a move
        // draws nothing.
        let lens = self.look_of.is_none_or(|f| f(&self.state).fx.lens >= 0.5);
        let shown = self.run.is_some() || self.gaze_of.is_some_and(|f| f(&self.state).mode != GazeMode::Off)
            || lens && self.scene.instances.iter().any(|i| i.paint[3] == PICTURE || i.paint[3] == MESH);
        if shown || self.touching || self.capture.is_some() || self.sel_drag || self.text_capture.is_some() { self.dirty = true; }
        let s = self.scale;
        self.keyboard = false;
        // A finger has no hover: it moves what it holds, and the lens does not follow it.
        let hover = if self.touching { None } else {
            self.field.pointer = [x * s, y * s, 1.0, 0.0];
            self.hit_target(x, y).and_then(|i| self.up(i, Node::active)).map(|i| self.nodes[i].key.clone())
        };
        if hover != self.hover {
            self.dirty = true;
            // A control the pointer comes onto gets a short burst over it (glitchr reads it).
            if let Some(n) = hover.as_deref().and_then(|k| self.node(k)) { self.burst(n.rect, 0.75, 0.22); }
            self.hover = hover;
            if let Some(f) = self.hovered && f(&mut self.state, self.hover.as_deref()) { self.relayout(); }
            // The marks land on the new target in this frame, not on the next tick.
            self.aim_marks();
            self.finish();
        }
        if self.capture.is_some() { self.drag(Phase::Move, x, y); }
        if !self.touching { self.pick_hover(x, y); self.move_hover(Some((x, y))); }
        if !self.touching && let Some(f) = self.pointer_of && f(&mut self.state, x, y) { self.relayout(); }
        if let Some(k) = self.text_capture.clone() && let Some(fi) = self.nodes.iter().position(|n| n.key == k) && self.field_at() == Some(fi)
            && let Some(at) = self.leaf_of(fi).and_then(|l| self.text_hit(l, x, y)) {
            self.send_text(TextInput::Pointer { at, phase: Phase::Move, clicks: self.clicks.3, shift: false });
        }
        if self.sel_drag && let Some(i) = self.sel_target(x, y) && let Some(at) = self.text_hit(i, x, y) {
            let key = self.nodes[i].key.clone();
            if let Some((_, focus)) = &mut self.sel && *focus != (key.clone(), at) { *focus = (key, at); self.paint(); }
        }
        if self.cursor() != cursor { self.dirty = true; }
    }

    /// Tell the element with `on_move` under the pointer where it is (local px), and the one it left
    /// that it left. `None`: the pointer left the window.
    fn move_hover(&mut self, at: Option<(f32, f32)>) {
        let now = at.and_then(|(x, y)| self.hit(x, y).and_then(|i| self.up(i, |n| n.on_move.is_some())));
        let key = now.map(|i| self.nodes[i].key.clone());
        if key != self.move_over && let Some(old) = self.move_over.take()
            && let Some(f) = self.nodes.iter().find(|n| n.key == old).and_then(|n| n.on_move.clone()) && let Some(m) = f(None) { self.send(m); }
        // An element being dragged hears the pointer through its drag alone (`El::on_point` would
        // otherwise hear each move twice, once as a hover, and could not tell a hover from the end).
        let dragged = self.capture.as_ref().is_some_and(|c| key.as_deref() == Some(c.key.as_str()));
        self.move_over = key;
        if dragged { return; }
        if let (Some(i), Some((x, y))) = (now, at) && let Some(f) = self.nodes[i].on_move.clone() {
            let (lx, ly) = self.local_in(i, x, y);
            let (glyph, z) = self.glyph_in(i, x, y);
            if let Some(m) = f(Some(crate::el::Point { x: lx, y: ly, down: false, glyph, z, t: self.ev })) { self.send_at(Some(i), m); }
        }
    }
    /// A window point (logical px) in element `i`'s own coordinates: from its top left, or in a
    /// canvas's drawing space.
    fn local_in(&self, i: usize, x: f32, y: f32) -> (f32, f32) {
        let n = &self.nodes[i];
        match &n.content {
            Content::Canvas(d) => { let k = d.w / n.inner.w.max(1e-3); ((x - n.inner.x) * k, (y - n.inner.y) * k) }
            _ => (x - n.rect.x, y - n.rect.y),
        }
    }
    /// The `scene3d` element under (x, y) with a pick handler, and the copy there (logical px).
    fn pick_at(&self, x: f32, y: f32) -> Option<(usize, Option<crate::mesh::MeshHit>)> {
        let i = self.hit(x, y).and_then(|i| self.up(i, |n| n.on_pick.is_some()))?;
        let n = &self.nodes[i];
        let Content::Scene3d(sc) = &n.content else { return Some((i, None)) };
        let r = n.inner;
        Some((i, sc.pick(&self.meshes, (x - r.x) / r.w.max(1.0), (y - r.y) / r.h.max(1.0), r.w.max(1.0) / r.h.max(1.0))))
    }
    /// Tell a `scene3d` element when the copy under the pointer changes (onto one, another, or off).
    fn pick_hover(&mut self, x: f32, y: f32) {
        let now = self.pick_at(x, y).map(|(i, h)| (self.nodes[i].key.clone(), h));
        if now == self.mesh_hover { return; }
        let old = std::mem::replace(&mut self.mesh_hover, now.clone());
        // Leaving an element tells it the pointer is on nothing there.
        if let Some((k, Some(_))) = old && now.as_ref().is_none_or(|(n, _)| *n != k)
            && let Some(f) = self.nodes.iter().find(|n| n.key == k).and_then(|n| n.on_pick.clone()) && let Some(m) = f(crate::mesh::Pick::Hover(None)) { self.send(m); }
        if let Some((k, h)) = now && let Some(f) = self.nodes.iter().find(|n| n.key == k).and_then(|n| n.on_pick.clone()) && let Some(m) = f(crate::mesh::Pick::Hover(h)) { self.send(m); }
    }
    /// Give the captured handler the pointer at (x, y), logical px. The element's current rect is
    /// used when it still exists, so a drag follows an element that moves.
    fn drag(&mut self, phase: Phase, x: f32, y: f32) {
        let Some(c) = &mut self.capture else { return };
        let at = self.nodes.iter().position(|n| n.key == c.key);
        if let Some(i) = at { c.rect = self.nodes[i].rect; }
        let r = c.rect;
        let mut d = Drag { phase, x: x - r.x, y: y - r.y, w: r.w, h: r.h, dx: x - c.start.0, dy: y - c.start.1, t: self.ev };
        // A canvas is dragged in its drawing's space.
        if let Some(i) = at && let Content::Canvas(dr) = &self.nodes[i].content {
            let inner = self.nodes[i].inner;
            let k = dr.w / inner.w.max(1e-3);
            d = Drag { phase, x: (x - inner.x) * k, y: (y - inner.y) * k, w: dr.w, h: dr.h, dx: d.dx * k, dy: d.dy * k, t: d.t };
        }
        let Some(c) = &self.capture else { return };
        if let Some(m) = (c.handler)(d) { self.send_at(at, m); }
    }
    /// The pointer left the window.
    pub fn pointer_leave(&mut self) {
        if !self.scripted { self.input_t = self.wall; }
        self.settle();
        self.dirty = true;
        self.field.pointer = [-1e5, -1e5, 0.0, 0.0];
        self.move_hover(None);
        if self.hover.take().is_some() && let Some(f) = self.hovered && f(&mut self.state, None) { self.relayout(); }
    }
    /// A burst over a rect (logical px) from now: `strength` 0..1 for `secs`.
    pub fn burst(&mut self, r: Rect, strength: f32, secs: f32) {
        // A press's and a hover's burst is loosening on press (`Off::loosen`).
        if self.timing.off.loosen { return; }
        let s = self.scale;
        self.field.burst(Some([r.x * s, r.y * s, r.right() * s, r.bottom() * s]), self.clock, strength, secs);
    }

    /// A mouse press at (`x`, `y`), logical px.
    pub fn press(&mut self, x: f32, y: f32) { self.press_with(x, y, false); }
    /// A press with Shift held or not (Shift extends a text selection).
    pub fn press_with(&mut self, x: f32, y: f32, shift: bool) {
        self.settle();
        self.stage_input.0 = true;
        // A press without a release before it (a lost pointer-up) ends the old capture.
        self.capture = None;
        self.text_capture = None;
        self.sel_drag = false;
        self.heard();
        self.pointer_move(x, y);
        let (t0, x0, y0, n0) = self.clicks;
        let clicks = if self.clock - t0 < CLICK_GAP.0 && (x - x0).abs() < CLICK_GAP.1 && (y - y0).abs() < CLICK_GAP.1 { n0.saturating_add(1) } else { 1 };
        self.clicks = (self.clock, x, y, clicks);
        let h = self.hit_target(x, y);
        let ctl = h.and_then(|i| self.up(i, Node::active));
        self.front_from(ctl.or(h), x, y, ctl.is_some(), 1.0);
        let target = self.hit_target(x, y).and_then(|i| self.up(i, |n| n.focusable)).map(|i| self.nodes[i].key.clone());
        self.set_focus(target);
        // Losing focus may have changed the page, so look again.
        let hit = self.hit_target(x, y);
        // In a text field the press places the caret (two select a word, three everything) and the drag
        // that follows selects.
        if let Some(fi) = hit.and_then(|i| self.up(i, |n| n.on_text.is_some())) {
            if self.sel.take().is_some() { self.paint(); }
            self.text_capture = Some(self.nodes[fi].key.clone());
            if let Some(at) = self.leaf_of(fi).and_then(|l| self.text_hit(l, x, y)) {
                self.send_text(TextInput::Pointer { at, phase: Phase::Start, clicks, shift });
            }
            return;
        }
        // A press on a 3D element says which copy it was on, then the element's drag (an orbit) goes on.
        if let Some((i, h)) = self.pick_at(x, y) && let Some(f) = self.nodes[i].on_pick.clone() && let Some(m) = f(crate::mesh::Pick::Press(h)) { self.send(m); }
        let hit = self.hit_target(x, y);
        // The nearest element that takes the press decides: a button or link inside a dragged element
        // (a scene that hears the pointer) gets its own press; the drag starts only on the dragged
        // element's own area or on something inside it that takes no press.
        if let Some(i) = hit.and_then(|i| self.up(i, |n| n.on_drag.is_some() || n.on_press.is_some())).filter(|&i| self.nodes[i].on_drag.is_some()) {
            let n = &self.nodes[i];
            let Some(handler) = n.on_drag.clone() else { return };
            self.capture = Some(Capture { key: n.key.clone(), handler, start: (x, y), rect: n.rect });
            self.drag(Phase::Start, x, y);
            return;
        }
        if let Some(t) = hit.and_then(|i| self.up(i, |n| n.skip_to.is_some())).and_then(|i| self.nodes[i].skip_to.clone()) { self.skip(&t); return; }
        let pressed = hit.and_then(|i| self.up(i, |n| n.on_press.is_some()));
        // Nothing to press here: selectable text starts a selection there; anywhere else ends one.
        if pressed.is_none() {
            match hit.filter(|&i| self.nodes[i].selectable && self.nodes[i].text().is_some() && !self.on_plate(i)) {
                Some(i) => {
                    let at = self.text_hit(i, x, y).unwrap_or(0);
                    let s = self.nodes[i].text().unwrap_or_default();
                    let (a, b) = match clicks { 1 => (at, at), 2 => crate::text::word_range(s, at), _ => (0, s.len()) };
                    let key = self.nodes[i].key.clone();
                    self.sel = Some(((key.clone(), a), (key, b)));
                    self.sel_drag = true;
                    self.paint();
                }
                None => if self.sel.take().is_some() { self.paint(); },
            }
        }
        if let Some(i) = pressed {
            self.pressed = Some(self.nodes[i].key.clone());
            let r = self.nodes[i].rect;
            self.burst(r, 1.0, self.timing.press);
            if self.up(i, |n| n.commit).is_some() { self.snapshot(x, y); }
            let m = self.nodes[i].on_press.clone();
            if let Some(m) = m { self.send_at(Some(i), m); } else { self.paint(); }
        }
    }

    /// The mouse button came up at (`x`, `y`).
    pub fn release(&mut self, x: f32, y: f32) {
        if !self.scripted { self.input_t = self.wall; }
        self.settle();
        self.stage_input = (true, true);
        self.dirty = true;
        if let Some(k) = self.text_capture.take() && self.field_at().is_some_and(|f| self.nodes[f].key == k)
            && let Some(at) = self.field_at().and_then(|f| self.leaf_of(f)).and_then(|l| self.text_hit(l, x, y)) {
            self.send_text(TextInput::Pointer { at, phase: Phase::End, clicks: self.clicks.3, shift: false });
        }
        self.sel_drag = false;
        if self.capture.is_some() {
            self.drag(Phase::End, x, y);
            self.capture = None;
        }
        // A held element condenses back from loose when let go.
        if let Some(k) = self.pressed.take() { self.rebirth(&k); }
    }

    /// Scroll the nearest scroll ancestor under the point by `dy` logical px; when it is already at its
    /// end, the next one out takes it.
    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) { self.wheel_xy(x, y, 0.0, dy); }
    /// A wheel with both axes. A horizontal scroll node takes `dx`, or `dy` when there is no `dx`
    /// (most wheels only turn one way).
    pub fn wheel_xy(&mut self, x: f32, y: f32, dx: f32, dy: f32) { self.stage_input.0 = true; self.deferred(|ui| ui.wheel_xy_now(x, y, dx, dy)) }
    fn wheel_xy_now(&mut self, x: f32, y: f32, dx: f32, dy: f32) {
        self.dirty = true;
        self.heard();
        // A step of a gesture under way starts at the node that took the one before (`WHEEL_LATCH`).
        let latched = self.wheel_latch.take().filter(|l| self.clock - l.1 < WHEEL_LATCH && l.2 == (x, y)).and_then(|l| self.nodes.iter().position(|n| n.key == l.0));
        let mut at = latched.or_else(|| self.hit(x, y));
        // Held by the gesture while nothing takes its steps (a scroll at its end), not handed on.
        let mut took = latched;
        // An element that takes the wheel itself (a 3D view's dolly) gets it before any scroll node; when it
        // makes nothing of it, the scroll nodes have it.
        if let Some(i) = at.and_then(|i| self.up(i, |n| n.on_wheel.is_some())) && let Some(f) = self.nodes[i].on_wheel.clone() && let Some(m) = f(dx, dy) {
            self.wheel_latch = Some((self.nodes[i].key.clone(), self.clock, (x, y)));
            self.send_at(Some(i), m);
            return;
        }
        let mut moved = false;
        while let Some(i) = at.and_then(|i| self.up(i, |n| n.scroll)) {
            let d = if self.nodes[i].horizontal { if dx != 0.0 { dx } else { dy } } else { dy };
            if d != 0.0 && self.scroll_by(i, d) { (took, moved) = (Some(i), true); break; }
            at = self.nodes[i].parent;
        }
        if let Some(i) = took { self.wheel_latch = Some((self.nodes[i].key.clone(), self.clock, (x, y))); }
        if moved { self.scrolled(); }
    }

    /// A key press; the flag is Shift. Returns whether anything used it.
    pub fn key(&mut self, k: Key, shift: bool) -> bool { self.key_mods(k, Mods { shift, ..Mods::default() }) }
    /// A key press with its modifiers. A focused text field takes it first: characters as text, Up /
    /// Down / Home / End in a multi-line field as moves by the layout, anything else as a key (Cmd+A,
    /// Cmd+Z…). Returns whether anything used it.
    pub fn key_mods(&mut self, k: Key, mods: Mods) -> bool {
        self.settle();
        self.stage_input.0 = true;
        let shift = mods.shift;
        self.dirty = true;
        self.heard();
        self.keyboard = true;
        if k == Key::Tab { self.cycle_focus(shift); return true; }
        if let Some(fi) = self.field_at() && k != Key::Escape {
            let ev = match k {
                Key::Char(c) if !mods.cmd && !mods.alt => Some(TextInput::Insert(c.to_string())),
                Key::Up | Key::Down | Key::Home | Key::End if self.nodes[fi].multiline && !mods.cmd => self.line_nav(fi, k).map(|at| TextInput::Goto { at, extend: shift }),
                _ => Some(TextInput::Key { key: k, mods }),
            };
            if let Some(ev) = ev && self.send_text(ev) { return true; }
        }
        // Shortcuts with Cmd are only for text fields; the rest belong to the system.
        if mods.cmd {
            if let (Key::Char(c), Some(f)) = (k, self.shortcut) && f(&mut self.state, c.to_ascii_lowercase()) { self.relayout(); return true; }
            return false;
        }
        let lo = self.modal_at.unwrap_or(0);
        let fi = self.focus.as_deref().and_then(|f| self.nodes.iter().position(|n| n.key == f)).filter(|&i| i >= lo);
        if let Some(fi) = fi && let Some(m) = self.nodes[fi].on_key.clone().and_then(|f| f(k, shift)) {
            let key = self.nodes[fi].key.clone();
            if self.up(fi, |n| n.commit).is_some() {
                let (cx, cy) = self.nodes[fi].rect.center();
                self.snapshot(cx, cy);
                self.front_from(Some(fi), cx, cy, true, 1.0);
            }
            self.send_at(Some(fi), m);
            self.reveal_marked(&key);
            return true;
        }
        if k == Key::Escape && let Some(m) = self.modal_at.and_then(|i| self.nodes[i].modal.as_ref()).map(|m| m.dismiss.clone()) {
            self.send(m);
            return true;
        }
        let Some(fi) = fi else { return self.key_scroll(None, k) || self.unused(k, shift) };
        if matches!(k, Key::Enter | Key::Char(' ')) && let Some(t) = self.nodes[fi].skip_to.clone() { self.skip(&t); return true; }
        if matches!(k, Key::Enter | Key::Char(' ')) && let Some(m) = self.nodes[fi].on_press.clone() {
            let (cx, cy) = self.nodes[fi].rect.center();
            if self.up(fi, |n| n.commit).is_some() { self.snapshot(cx, cy); }
            self.front_from(Some(fi), cx, cy, true, 1.0);
            let key = self.nodes[fi].key.clone();
            self.send_at(Some(fi), m);
            self.rebirth(&key);
            return true;
        }
        self.key_scroll(Some(fi), k) || self.unused(k, shift)
    }

    /// A key nothing on the page used goes to the state, if it asked for them.
    fn unused(&mut self, k: Key, shift: bool) -> bool {
        match self.unused_key {
            Some(f) => { let used = f(&mut self.state, k, shift); if used { self.relayout(); } used }
            None => false,
        }
    }

    // ── Accessibility mirror (DESIGN.md §1.14) ───────────────────────────────────────────────────
    /// The mirror of the current tree: every element with a role (set by the widget or `El::role`), every
    /// other pressable control (a button), and text that is not part of a control's name. While a modal
    /// layer is open, only what is in it.
    pub fn a11y(&self) -> Vec<A11yNode> {
        let leafy = |r: Role| !matches!(r, Role::Dialog | Role::List | Role::TabList | Role::Group | Role::RadioGroup | Role::Listbox | Role::Region | Role::Table | Role::Row | Role::Navigation | Role::Menu | Role::Toolbar);
        let lo = self.modal_at.unwrap_or(0);
        let mut out: Vec<A11yNode> = vec![];
        let mut index: Vec<Option<usize>> = vec![None; self.nodes.len()];
        // The nearest ancestor in the mirror, and whether a control above already speaks for this text.
        let mut above: Vec<(Option<usize>, bool)> = vec![(None, false); self.nodes.len()];
        for i in 0..self.nodes.len() {
            let n = &self.nodes[i];
            let (up, spoken) = match n.parent { Some(p) => (index[p].or(above[p].0), above[p].1 || index[p].is_some_and(|k| leafy(out[k].role))), None => (None, false) };
            above[i] = (up, spoken);
            // What is drawn on a plate (`El::plate`) is a picture, not content.
            if n.hidden || i < lo && self.modal_at.is_some() || self.on_plate(i) { continue; }
            let a = n.access.clone().unwrap_or_default();
            let role = a.role.or_else(|| match &n.content {
                Content::Text { .. } if !spoken => Some(Role::Text),
                Content::Picture { .. } | Content::Scene3d(_) | Content::Canvas(_) | Content::Cells(_) | Content::CachedView { .. } => Some(Role::Img),
                Content::None if n.on_press.is_some() && n.focusable && !spoken => Some(Role::Button),
                _ => None,
            });
            let Some(role) = role else { continue };
            if spoken && role == Role::Text { continue; }
            let name = a.label.clone().unwrap_or_else(|| self.name_within(i));
            if role == Role::Text && name.trim().is_empty() { continue; }
            index[i] = Some(out.len());
            out.push(A11yNode {
                key: n.key.clone(), parent: up.map(|k| out[k].key.clone()), role, name, value: a.value, text: a.text, checked: a.checked, selected: a.selected,
                level: a.level, expanded: a.expanded, description: a.description, invalid: a.invalid, required: a.required,
                live: a.live.or((role == Role::Status).then_some(crate::el::Live::Polite)),
                modal: n.modal.is_some() || (role == Role::Dialog && self.modal_at.is_some()), disabled: n.disabled, focusable: n.focusable, rect: n.rect,
            });
        }
        out
    }
    /// The text inside node `i`, joined: a control's name when it has no label.
    fn name_within(&self, i: usize) -> String {
        let mut s = String::new();
        for j in i..self.nodes.len() {
            if !within(&self.nodes, j, i) { if j > i { break; } continue; }
            if let Some(t) = self.nodes[j].text() { if !s.is_empty() { s.push(' '); } s.push_str(t.trim()); }
            if s.len() > 160 { break; }
        }
        s
    }
    /// What changed in the mirror since the last call (the same keyed comparison as layout's): elements
    /// added or changed, elements gone, and where focus is now if it moved.
    pub fn a11y_diff(&mut self) -> Vec<A11yOp> {
        self.settle();
        let mut ops = vec![];
        // While a scroll goes on and nothing was laid out, only where things are changes: the mirror is
        // sent once the scroll rests (`SOFT_STOP` after the last step; `next_wake` asks for that frame).
        if self.scroll_soft() && self.a11y_gen == self.layout_gen {
            self.a11y_owed = true;
            if self.a11y_focus != self.focus { self.a11y_focus = self.focus.clone(); ops.push(A11yOp::Focus(self.focus.clone())); }
            return ops;
        }
        self.a11y_owed = false;
        self.a11y_gen = self.layout_gen;
        let now = self.a11y();
        let keys: HashSet<&str> = now.iter().map(|n| n.key.as_str()).collect();
        for k in self.a11y_sent.keys() { if !keys.contains(k.as_str()) { ops.push(A11yOp::Remove(k.clone())); } }
        let mut sent = HashMap::with_capacity_and_hasher(now.len(), Default::default());
        for n in now {
            if self.a11y_sent.get(&n.key) != Some(&n) { ops.push(A11yOp::Set(n.clone())); }
            sent.insert(n.key.clone(), n);
        }
        self.a11y_sent = sent;
        if self.a11y_focus != self.focus { self.a11y_focus = self.focus.clone(); ops.push(A11yOp::Focus(self.focus.clone())); }
        ops
    }
    /// Assistive technology moved focus to the element `key`.
    pub fn focus_key(&mut self, key: &str) {
        self.settle();
        self.dirty = true;
        if self.node(key).is_some_and(|n| n.focusable) { self.keyboard = true; self.set_focus(Some(key.to_owned())); }
    }
    /// Whether the text element `key`, clamped to lines (`El::clamp`), was cut: it ends in an ellipsis
    /// and does not show all of its text (a place to offer `More`).
    pub fn clamped(&self, key: &str) -> bool { self.node(key).is_some_and(|n| n.shown.is_some()) }
    /// The scale the box `key` (marked `El::fit`) zoomed its subtree by in the last layout.
    pub fn fitted(&self, key: &str) -> Option<f32> { self.fit_k.get(key).copied() }
    /// Take the focus away (`Cmd::ClearFocus`): nothing has it, what is shown only with focus inside it
    /// folds away, the caret and the input method go, and the corners leave as they do when the focus
    /// moves on. The next Tab goes on from the element that had it. Nothing when nothing has the focus.
    pub fn clear_focus(&mut self) {
        let Some(k) = self.focus.clone() else { return };
        self.after_focus = Some(k);
        self.set_focus(None);
    }
    /// Assistive technology activated the element `key` (a click on its mirror): the same message as a
    /// press or Enter, with the same front and commit. Returns whether it did anything.
    pub fn activate(&mut self, key: &str) -> bool {
        self.settle();
        self.dirty = true;
        self.heard();
        let Some(i) = self.nodes.iter().position(|n| n.key == key) else { return false };
        if let Some(t) = self.nodes[i].skip_to.clone() { self.skip(&t); return true; }
        if self.nodes[i].focusable { self.set_focus(Some(key.to_owned())); }
        let Some(i) = self.nodes.iter().position(|n| n.key == key) else { return false };
        let Some(m) = self.nodes[i].on_press.clone() else { return false };
        let (cx, cy) = self.nodes[i].rect.center();
        if self.up(i, |n| n.commit).is_some() { self.snapshot(cx, cy); }
        self.front_from(Some(i), cx, cy, true, 1.0);
        self.send(m);
        self.rebirth(key);
        true
    }

    // ── Text (DESIGN.md §1.8.1) ──────────────────────────────────────────────────────────────────
    /// The focused text field's node, if focus is on one.
    fn field_at(&self) -> Option<usize> {
        let f = self.focus.as_deref()?;
        let i = self.nodes.iter().position(|n| n.key == f)?;
        self.nodes[i].on_text.is_some().then_some(i)
    }
    /// The text a field shows: its first text descendant.
    fn leaf_of(&self, field: usize) -> Option<usize> {
        (field..self.nodes.len()).find(|&j| within(&self.nodes, j, field) && matches!(self.nodes[j].content, Content::Text { .. }))
    }
    /// Whether focus is on a text field: the host then sends typed text, the input method and the
    /// clipboard here (and keeps its hidden text element focused, on the web).
    pub fn text_focused(&self) -> bool { self.field_at().is_some() }
    /// Give the focused field a text event; returns whether it used it. Afterwards its caret is scrolled
    /// into view.
    fn send_text(&mut self, ev: TextInput) -> bool {
        let Some(fi) = self.field_at() else { return false };
        let Some(m) = self.nodes[fi].on_text.clone().and_then(|f| f(ev)) else { return false };
        self.send(m);
        self.reveal_caret();
        true
    }
    /// Text typed (or committed by an input method, or dropped in), for the focused field.
    pub fn text_input(&mut self, s: &str) -> bool {
        self.settle();
        self.stage_input.0 = true;
        self.dirty = true;
        self.heard();
        self.keyboard = true;
        self.send_text(TextInput::Insert(s.into()))
    }
    /// An input method's composition so far (`cursor`: a byte range in it); empty text cancels it.
    pub fn ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        self.settle();
        self.dirty = true;
        self.keyboard = true;
        self.send_text(TextInput::Compose { text: text.into(), cursor })
    }
    /// An input method committed `text` (empty: the composition ended with nothing).
    pub fn ime_commit(&mut self, text: &str) -> bool {
        self.settle();
        self.dirty = true;
        if text.is_empty() { self.send_text(TextInput::Compose { text: String::new(), cursor: None }) } else { self.text_input(text) }
    }
    /// The clipboard's text, pasted into the focused field.
    pub fn paste(&mut self, s: &str) -> bool { self.settle(); self.dirty = true; self.heard(); self.send_text(TextInput::Paste(s.into())) }
    /// What a copy takes: the focused field's selection, or else the read-only selection (its pieces in
    /// separate text elements joined by line breaks).
    pub fn copy(&self) -> Option<String> {
        if let Some(fi) = self.field_at() {
            let leaf = self.leaf_of(fi)?;
            let Content::Text { s, select: Some((a, b)), .. } = &self.nodes[leaf].content else { return None };
            return s.get(*a..*b).map(str::to_owned);
        }
        let mut pieces: Vec<(usize, (usize, usize))> = self.sel_ranges().into_iter().collect();
        if pieces.is_empty() { return None; }
        pieces.sort_by_key(|p| p.0);
        // A clamped text gives what it shows, without the ellipsis (it is not part of the text).
        let end = |i: usize| self.nodes[i].shown.as_deref().map_or(usize::MAX, |sh| sh.strip_suffix('\u{2026}').or_else(|| sh.strip_suffix("...")).unwrap_or(sh).len());
        Some(pieces.iter().filter_map(|&(i, (a, b))| self.nodes[i].text().and_then(|s| s.get(a.min(end(i))..b.min(end(i))))).collect::<Vec<_>>().join("\n"))
    }
    /// Copy, and remove the selection from the focused field.
    pub fn cut(&mut self) -> Option<String> {
        self.settle();
        let c = self.copy()?;
        if self.field_at().is_some() { self.dirty = true; self.send_text(TextInput::Cut); }
        Some(c)
    }
    /// Where the focused field's caret is (logical px): the host puts its hidden text element, and so an
    /// input method's candidate window, there.
    pub fn ime_rect(&mut self) -> Option<Rect> {
        self.settle();
        let leaf = self.field_at().and_then(|f| self.leaf_of(f))?;
        let n = &self.nodes[leaf];
        let Content::Text { caret, .. } = &n.content else { return None };
        let r = self.text.caret(n.para()?, n.origin(), caret.unwrap_or(0))?;
        Some(Rect::new(r.x, r.y, 2.0, r.h).intersect(&n.clip.inset(-2.0, -2.0, -2.0, -2.0)))
    }
    /// The pointer's shape where it is: an element's own (`El::cursor`), else text over a text field
    /// or selectable text, a pointing hand over what can be pressed, grab over what can be dragged
    /// (grabbing while it is).
    pub fn cursor(&self) -> Cursor {
        if self.text_capture.is_some() || self.sel_drag { return Cursor::Text; }
        if let Some(c) = &self.capture {
            return self.nodes.iter().find(|n| n.key == c.key).and_then(|n| n.cursor).filter(|c| *c != Cursor::Grab).unwrap_or(Cursor::Grabbing);
        }
        if self.field.pointer[2] <= 0.0 { return Cursor::Default; }
        let (x, y) = (self.field.pointer[0] / self.scale, self.field.pointer[1] / self.scale);
        let mut at = self.hit_target(x, y);
        while let Some(i) = at {
            let n = &self.nodes[i];
            if let Some(c) = n.cursor { return c; }
            if n.on_text.is_some() { return Cursor::Text; }
            if n.on_drag.is_some() { return Cursor::Grab; }
            if n.on_press.is_some() { return Cursor::Pointer; }
            if n.selectable && n.text().is_some() { return Cursor::Text; }
            at = n.parent;
        }
        Cursor::Default
    }
    /// The read-only selection now: the byte range selected in each selectable text node it spans.
    fn sel_ranges(&self) -> HashMap<usize, (usize, usize)> {
        let mut out = HashMap::default();
        let Some(((ka, a), (kb, b))) = &self.sel else { return out };
        let pos = |k: &str| self.nodes.iter().position(|n| n.key == k);
        let (Some(ia), Some(ib)) = (pos(ka), pos(kb)) else { return out };
        let ((i0, b0), (i1, b1)) = if (ia, *a) <= (ib, *b) { ((ia, *a), (ib, *b)) } else { ((ib, *b), (ia, *a)) };
        for i in i0..=i1 {
            if !self.nodes[i].selectable { continue; }
            let Some(s) = self.nodes[i].text() else { continue };
            let (lo, hi) = (if i == i0 { b0 } else { 0 }, if i == i1 { b1.min(s.len()) } else { s.len() });
            if lo < hi { out.insert(i, (lo, hi)); }
        }
        out
    }
    /// The read-only selection's text pieces, for tests.
    pub fn selection(&self) -> Vec<(String, usize, usize)> {
        let mut v: Vec<_> = self.sel_ranges().into_iter().map(|(i, (a, b))| (self.nodes[i].key.clone(), a, b)).collect();
        v.sort();
        v
    }
    /// The selectable text node under (x, y), or the nearest visible one (by line, then along it).
    fn sel_target(&self, x: f32, y: f32) -> Option<usize> {
        let seen = |i: usize| { let n = &self.nodes[i]; let r = n.rect.intersect(&n.clip); r.w > 0.0 && r.h > 0.0 };
        let cands: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.nodes[i].selectable && self.nodes[i].text().is_some() && !self.nodes[i].hidden && seen(i)).collect();
        if let Some(&i) = cands.iter().find(|&&i| self.nodes[i].rect.contains(x, y)) { return Some(i); }
        let dist = |i: usize| { let r = self.nodes[i].rect; let dy = (r.y - y).max(y - r.bottom()).max(0.0); let dx = (r.x - x).max(x - r.right()).max(0.0); (dy, dx) };
        cands.into_iter().min_by(|&a, &b| dist(a).partial_cmp(&dist(b)).unwrap_or(std::cmp::Ordering::Equal))
    }
    /// The byte under (x, y) in node `i`'s text.
    fn text_hit(&mut self, i: usize, x: f32, y: f32) -> Option<usize> {
        let n = &self.nodes[i];
        Some(self.text.hit(n.para()?, n.origin(), x, y))
    }
    /// The caret of field `fi` moved by a line or to a line's end (multi-line fields).
    fn line_nav(&mut self, fi: usize, k: Key) -> Option<usize> {
        let leaf = self.leaf_of(fi)?;
        let n = &self.nodes[leaf];
        let Content::Text { caret, .. } = &n.content else { return None };
        let to = match k { Key::Up => Nav::Up, Key::Down => Nav::Down, Key::Home => Nav::LineStart, _ => Nav::LineEnd };
        Some(self.text.nav(n.para()?, caret.unwrap_or(0), to))
    }
    /// Scroll the focused field (and anything around it) so its caret is visible.
    fn reveal_caret(&mut self) {
        for _ in 0..4 {
            let Some(leaf) = self.field_at().and_then(|f| self.leaf_of(f)) else { return };
            let n = &self.nodes[leaf];
            let Some(r) = (match &n.content { Content::Text { caret: Some(c), .. } => n.para().and_then(|p| self.text.caret(p, n.origin(), *c)), _ => None }) else { return };
            let mut a = self.nodes[leaf].parent;
            let mut moved = false;
            while let Some(p) = a.and_then(|p| self.up(p, |n| n.scroll)) {
                let view = self.nodes[p].clip.intersect(&self.nodes[p].inner);
                let (lo, hi, vlo, vhi) = if self.nodes[p].horizontal { (r.x - 2.0, r.x + 3.0, view.x, view.right()) } else { (r.y, r.bottom(), view.y, view.bottom()) };
                let d = if lo < vlo { lo - vlo } else if hi > vhi { hi - vhi } else { 0.0 };
                if d != 0.0 && self.scroll_by(p, d) { moved = true; break; }
                a = self.nodes[p].parent;
            }
            if !moved { return; }
            self.scrolled();
        }
    }

    // ── Internals ────────────────────────────────────────────────────────────────────────────────
    /// Update with a handler's message and lay out again; the layout is put down to the handler's call
    /// (`perf::FramePath::layout_from` names which input sent it).
    #[track_caller]
    fn send(&mut self, m: M) {
        let sp = self.span_begin();
        (self.update)(&mut self.state, m);
        self.relayout();
        self.span_end(sp);
    }
    /// `send` for a message from a handler of node `at`: under an element whose messages are its own
    /// (`El::local_messages`), only it and the elements that change with a stage (`El::steps`) are viewed
    /// again, when they are laid out as they were (`Ui::restage`); otherwise the page is.
    #[track_caller]
    fn send_at(&mut self, at: Option<usize>, m: M) {
        let local = at.and_then(|i| self.up(i, |n| n.stage.as_ref().is_some_and(|c| c.local))).filter(|_| self.stage_of.is_some());
        let Some(l) = local else { return self.send(m) };
        let sp = self.span_begin();
        (self.update)(&mut self.state, m);
        let mut keys = vec![self.nodes[l].key.clone()];
        keys.extend(self.nodes.iter().filter(|n| n.stage.as_ref().is_some_and(|c| !c.held && !c.local)).map(|n| n.key.clone()));
        self.restage(keys);
        self.span_end(sp);
    }

    /// What a press at (x, y) acts on: the topmost node there when it (or an ancestor) can be acted on;
    /// otherwise the control whose box, grown to at least `MIN_HIT` each way about its centre, holds the
    /// point (the nearest one), so small controls are easy to hit without looking bigger.
    fn hit_target(&self, x: f32, y: f32) -> Option<usize> {
        let h = self.hit(x, y);
        if h.and_then(|i| self.up(i, Node::active)).is_some() { return h; }
        let grown = |r: Rect| { let (gw, gh) = ((MIN_HIT - r.w).max(0.0) * 0.5, (MIN_HIT - r.h).max(0.0) * 0.5); r.inset(-gw, -gh, -gw, -gh) };
        let d = |r: Rect| { let (cx, cy) = r.center(); (cx - x).powi(2) + (cy - y).powi(2) };
        (self.modal_at.unwrap_or(0)..self.nodes.len()).rev()
            .filter(|&i| { let n = &self.nodes[i]; n.active() && !n.hidden && !n.pass && n.opacity_all > 0.0 && (n.rect.w < MIN_HIT || n.rect.h < MIN_HIT) && n.clip.contains(x, y) && grown(n.rect).contains(x, y) })
            .min_by(|&a, &b| d(self.nodes[a].rect).total_cmp(&d(self.nodes[b].rect)))
            .or(h)
    }

    /// Topmost node under a point, honouring clips; one the pointer passes through (`El::pass`) is not
    /// there. While a modal layer is open only it can be hit.
    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        (self.modal_at.unwrap_or(0)..self.nodes.len()).rev().find(|&i| { let n = &self.nodes[i]; !n.hidden && !n.pass && n.opacity_all > 0.0 && crate::hit::contains(&n.rect, n.xf.as_ref(), x, y) && n.clip.contains(x, y) })
    }
    /// The glyph drawn under (x, y) (logical px) in what was painted last: the nearest one, wherever
    /// its pose put it (on a path, on a form), with its element's key, its number in the text, its
    /// depth and the point in the text's unposed layout. A modal layer hides what is under it.
    pub fn glyph_at(&self, x: f32, y: f32) -> Option<crate::hit::GlyphHit> {
        let s = self.scale;
        let g = crate::hit::glyph_under(&self.scene.instances, &self.glyphs.log, x * s, y * s)?;
        if self.modal_at.is_some_and(|m| g.node < m) { return None; }
        let n = &self.nodes[g.node];
        let local = crate::hit::local_of(&self.scene.instances[g.inst], g.at, x * s, y * s, s);
        Some(crate::hit::GlyphHit { key: n.key.clone(), i: g.i, z: g.z, local })
    }
    /// The glyph under (x, y) if it belongs to node `i` or below it, and its depth.
    fn glyph_in(&self, i: usize, x: f32, y: f32) -> (Option<usize>, f32) {
        let s = self.scale;
        match crate::hit::glyph_under(&self.scene.instances, &self.glyphs.log, x * s, y * s) {
            Some(g) if self.within(g.node, i) => (Some(g.i), g.z),
            _ => (None, 0.0),
        }
    }

    /// Keep the frame as it is now for the renderer, with the commit's origin (logical px).
    fn snapshot(&mut self, x: f32, y: f32) {
        // A press's commit stays within the scope the pressed element is in.
        let scope = self.hit(x, y).and_then(|i| self.up(i, |n| n.commit_scope));
        self.snapshot_in(x, y, scope);
    }
    /// The same, confined to node `scope`'s box as seen (`El::commit_scope`).
    fn snapshot_in(&mut self, x: f32, y: f32, scope: Option<usize>) {
        // Unique across runtimes: a renderer that draws several runtimes (tools, checks) tells commits apart.
        self.splits = next_run();
        let s = self.scale;
        let look = self.look();
        // The scope's box grown by what a material draws round an element (its nearness reach and a
        // front's push), so the new material's marks just outside the box arrive with the box.
        let grow = crate::text::SPREAD as f32 + look.fx.push * s;
        let scope = scope.map(|i| { let r = self.nodes[i].rect.intersect(&self.nodes[i].clip); [(r.x * s).floor() - grow, (r.y * s).floor() - grow, (r.right() * s).ceil() + grow, (r.bottom() * s).ceil() + grow] });
        self.split = Some(Split { id: self.splits, scene: self.scene.clone(), field: self.field, at: [x * s, y * s], t: self.beat_grid(), look, scope });
    }

    /// The node itself or its nearest ancestor that satisfies `f`.
    fn up(&self, mut i: usize, f: impl Fn(&Node<M>) -> bool) -> Option<usize> {
        loop {
            if f(&self.nodes[i]) { return Some(i); }
            i = self.nodes[i].parent?;
        }
    }

    fn within(&self, i: usize, root: usize) -> bool { within(&self.nodes, i, root) }

    /// A skip link's activation: focus `target`, or the first element inside it that takes focus, as
    /// the keyboard would, and scroll it into view.
    fn skip(&mut self, target: &str) {
        self.dirty = true;
        let Some(root) = self.nodes.iter().position(|n| n.key == target) else { return };
        let Some(i) = (root..self.nodes.len()).take_while(|&i| self.within(i, root)).find(|&i| self.nodes[i].focusable && !self.nodes[i].disabled) else { self.reveal_key(target); return };
        self.keyboard = true;
        self.set_focus(Some(self.nodes[i].key.clone()));
    }

    fn set_focus(&mut self, to: Option<String>) {
        if to == self.focus { return; }
        if to.is_some() { self.after_focus = None; }
        let old = std::mem::replace(&mut self.focus, to);
        let blur = old.and_then(|k| self.node(&k).and_then(|n| n.on_blur.clone()));
        if let Some(m) = blur { self.send(m); }
        if let Some(f) = self.focused && f(&mut self.state, self.focus.as_deref()) { self.relayout(); }
        self.reveal();
        // The caret is drawn only where focus is.
        self.paint();
    }

    fn cycle_focus(&mut self, back: bool) {
        // While a modal layer is open, focus goes round inside it only.
        // Positive tab orders first, lowest first; then 0 in document order; negative ones never.
        let mut order: Vec<usize> = (self.modal_at.unwrap_or(0)..self.nodes.len()).filter(|&i| self.nodes[i].focusable && self.nodes[i].tab_order >= 0).collect();
        // Document order is the order as written (`Node::written`, from the root down).
        let written = |mut i: usize| { let mut at = vec![]; loop { match self.nodes[i].parent { Some(p) => { at.push(self.nodes[i].written); i = p; } None => { at.push(i); break; } } } at.reverse(); at };
        order.sort_by_cached_key(|&i| (match self.nodes[i].tab_order { 0 => i32::MAX, t => t }, written(i)));
        if order.is_empty() { return; }
        // After `clear_focus`, Tab goes on from the element that had it.
        let cur = self.focus.as_deref().or(self.after_focus.as_deref()).and_then(|f| order.iter().position(|&i| self.nodes[i].key == f));
        let n = order.len();
        let next = match (cur, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(c), false) => (c + 1) % n,
            (Some(c), true) => (c + n - 1) % n,
        };
        self.set_focus(Some(self.nodes[order[next]].key.clone()));
    }

    /// Scroll so the focused node is visible.
    fn reveal(&mut self) {
        if let Some(key) = self.focus.clone() { self.reveal_key(&key); }
    }

    /// After a key changed the state through `key`'s handler: show the elements inside it that ask to
    /// be seen (a newly selected row).
    fn reveal_marked(&mut self, key: &str) {
        let Some(root) = self.nodes.iter().position(|n| n.key == key) else { return };
        // A virtual list's selected row may not be built: scroll by its index.
        if let Some((_, rh, Some(sel))) = self.nodes[root].virt {
            let n = &self.nodes[root];
            let view = n.rect.h - (n.inner.y - n.rect.y) * 2.0;
            let off = self.scroll.get(&n.key).copied().unwrap_or(0.0);
            let (top, bottom) = (sel as f32 * rh, (sel + 1) as f32 * rh);
            let to = if top < off { top } else if bottom > off + view { bottom - view } else { off };
            if to != off && self.scroll_by(root, to - off) { self.scrolled(); }
            return;
        }
        let marked: Vec<String> = (root..self.nodes.len()).filter(|&i| self.nodes[i].reveal && self.within(i, root)).map(|i| self.nodes[i].key.clone()).collect();
        for k in marked { self.reveal_key(&k); }
    }

    /// Scroll every scroll ancestor of a node so that it is inside, innermost first.
    fn reveal_key(&mut self, key: &str) {
        let mut guard = 0;
        loop {
            guard += 1;
            let Some(i) = self.nodes.iter().position(|n| n.key == key) else { return };
            let mut a = self.nodes[i].parent;
            let mut moved = false;
            while let Some(p) = a.and_then(|p| self.up(p, |n| n.scroll)) {
                // What can be seen of it: a box that clips it (a sheet's frame) cuts what is to be shown.
                let mut r = self.nodes[i].rect;
                let mut c = self.nodes[i].parent;
                while let Some(j) = c && j != p { if self.nodes[j].clips && !self.nodes[j].scroll { r = r.intersect(&self.nodes[j].rect); } c = self.nodes[j].parent; }
                let view = self.nodes[p].clip.intersect(&self.nodes[p].inner);
                let (lo, hi, vlo, vhi) = if self.nodes[p].horizontal { (r.x, r.right(), view.x, view.right()) } else { (r.y, r.bottom(), view.y, view.bottom()) };
                let d = if lo < vlo { lo - vlo - 8.0 } else if hi > vhi { (hi - vhi + 8.0).min(lo - vlo) } else { 0.0 };
                if d != 0.0 && self.scroll_by(p, d) { moved = true; break; }
                a = self.nodes[p].parent;
            }
            if !moved || guard > 4 { break; }
            self.scrolled();
        }
    }

    fn key_scroll(&mut self, from: Option<usize>, k: Key) -> bool {
        let dy = match k { Key::Up => -40.0, Key::Down => 40.0, Key::PageUp => -self.size.1 * 0.8, Key::PageDown => self.size.1 * 0.8, _ => return false };
        let start = from.or_else(|| (!self.nodes.is_empty()).then_some(0));
        let Some(i) = start.and_then(|i| self.up(i, |n| n.scroll && !n.horizontal).or_else(|| self.nodes.iter().position(|n| n.scroll && !n.horizontal))) else { return false };
        if self.scroll_by(i, dy) { self.scrolled(); }
        true
    }

    /// Returns whether the offset changed.
    fn scroll_by(&mut self, i: usize, dy: f32) -> bool {
        let n = &self.nodes[i];
        let max = (n.extent - if n.horizontal { n.rect.w } else { n.rect.h }).max(0.0);
        let cur = self.scroll.get(&n.key).copied().unwrap_or(0.0);
        let next = (cur + dy).clamp(0.0, max);
        if next == cur { return false; }
        self.scroll.insert(n.key.clone(), next);
        true
    }

    /// Give every instance of a node and its descendants a fresh birth time. What is legible condenses
    /// from no looser than `Look::legible_cap` (it is born that far into condensing), what is ignored
    /// not at all (DESIGN.md §1.15).
    fn rebirth(&mut self, key: &str) {
        if self.timing.off.loosen { return; }
        let Some(root) = self.nodes.iter().position(|n| n.key == key) else { return };
        let enter = self.timing.enter;
        for &(owner, id) in &self.owners {
            if self.within(owner, root) {
                let (_, cap, _) = response_caps(&self.nodes[owner], self.timing.legible_cap);
                self.births.insert(id, self.clock - enter * (1.0 - cap));
            }
        }
        self.paint();
    }

    /// The front a press (or Enter, or activation) on node `i` at (x, y), logical px, sends
    /// (DESIGN.md §1.15): to the scope the node has (`El::ripple`, inherited); a control without one
    /// reaches only its own subtree, plain ground nothing; a commit (theme, material) the whole window.
    /// A new press from the same element restarts its front rather than adding one.
    fn front_from(&mut self, i: Option<usize>, x: f32, y: f32, control: bool, strength: f32) {
        let Some(i) = i else { return };
        let commit = self.up(i, |n| n.commit).is_some();
        if (commit && self.timing.off.fronts) || (!commit && self.timing.off.ripples) { return; }
        let scope = if commit { Some(Scope::Global) }
                    else { self.nodes[i].ripple.clone().or(control.then_some(Scope::Own)) };
        let Some(scope) = scope else { return };
        let window = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let seen = |n: &Node<M>| n.rect.intersect(&n.clip);
        // A commit within a commit's scope (`El::commit_scope`) plays inside it: its front too.
        let within_commit = if commit { self.up(i, |n| n.commit_scope) } else { None };
        let r = match &scope {
            Scope::Off => return,
            Scope::Global if within_commit.is_some() => within_commit.map(|c| seen(&self.nodes[c])),
            Scope::Global => None,
            Scope::Own => Some(seen(&self.nodes[i])),
            Scope::Container(None) => Some(self.up(i, |n| n.ripple_scope).map_or(window, |c| seen(&self.nodes[c]))),
            Scope::Container(Some(k)) => Some(self.nodes.iter().find(|n| &n.key == k).map_or(window, seen)),
        };
        let s = self.scale;
        let rect = r.map_or(EVERYWHERE, |r| [r.x * s, r.y * s, r.right() * s, r.bottom() * s]);
        let mut h = DefaultHasher::new();
        self.nodes[i].key.hash(&mut h);
        let source = h.finish() | 1;
        self.field.front_in([x * s, y * s], self.beat_grid(), strength, rect, source, self.timing.wave_life);
    }

    /// Lay out and paint what pointer moves, wheels and ticks left waiting: the renderer does it before
    /// it draws, and every other input before it looks at the page.
    pub fn settle(&mut self) {
        // A fill that answers the pointer is painted again when what it answers changes.
        if self.fill_state.is_some() && self.under_now() != self.fill_state { self.stale_paint = true; }
        if !self.stale() { return; }
        if self.stale_layout { self.relayout(); } else if !self.stale_stages.is_empty() { let k = std::mem::take(&mut self.stale_stages); self.restage(k); } else if self.stale_paint { self.paint(); }
        // The input this paint answered is spent (for the stages: `tick` leaves it while the paint waits).
        self.stage_input = (false, false);
    }
    /// The hovered and the held key, when some element's fill answers them (`El::fill_hover`).
    fn under_now(&self) -> Option<(Option<String>, Option<String>, Option<String>)> {
        if self.nodes.iter().all(|n| n.fill_on.none()) { return None; }
        Some((self.hover.clone(), self.capture.as_ref().map(|c| c.key.clone()).or_else(|| self.pressed.clone()), self.focus.clone()))
    }
    /// Laying out or painting waits for the frame (`Ui::settle`).
    fn stale(&self) -> bool { self.stale_layout || self.stale_paint || !self.stale_stages.is_empty() }
    /// Lay out once per frame what pointer moves, wheels and ticks change, before the frame is drawn
    /// (`Ui::settle`), instead of at each: for a host that hands over several events a frame and ticks
    /// before each (the web and desktop hosts turn it on). Off, each one lays out at once.
    pub fn coalesce(&mut self, on: bool) { self.coalesce = on; if !on { self.settle(); } }
    /// Run `f` with laying out and painting left for the frame when input is coalesced (`Ui::settle`).
    fn deferred<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        if !self.coalesce { return f(self); }
        self.defer += 1;
        let r = f(self);
        self.defer -= 1;
        r
    }
    /// View → taffy tree → absolute rects → scene (inside a pointer move or a wheel, once before the
    /// frame instead: `Ui::settle`).
    #[track_caller]
    pub fn relayout(&mut self) {
        let here = std::panic::Location::caller();
        if self.defer > 0 { self.stale_from.get_or_insert(here); self.stale_layout = true; self.dirty = true; return; }
        let from = self.stale_from.take().unwrap_or(here);
        self.laid = (self.laid.0 + 1, self.laid.1.or(Some(from)));
        self.stale_layout = false;
        self.stale_stages.clear();
        self.held_steps.clear();
        let sp = self.span_begin();
        self.layout_gen += 1;
        self.relayout_inner();
        self.span_end(sp);
    }
    fn relayout_inner(&mut self) {
        // Commands are taken first: a flow needs the frame as it is before the new layout.
        let mut cmds = std::mem::take(&mut self.sent);
        cmds.extend(self.commands.map(|f| f(&mut self.state)).unwrap_or_default());
        // What is turned off goes straight to its end (DESIGN.md §1.34): its command does nothing.
        let off = self.timing.off;
        cmds.retain(|c| !match c {
            Cmd::Flow(_) => off.flows,
            Cmd::Morph { .. } => off.morphs,
            Cmd::Commit { .. } => off.fronts,
            Cmd::Ripplr { .. } | Cmd::RipplrAt { .. } => off.fronts,
            _ => false,
        });
        let flow = cmds.iter().find_map(|c| if let Cmd::Flow(f) = c { Some(*f) } else { None });
        if let Some(f) = flow { self.fade_s = (f.fade_ms / 1000.0).max(1e-3); }
        // A commit keeps the frame as it is before the new view.
        if let Some((x, y, scope)) = cmds.iter().find_map(|c| if let Cmd::Commit { x, y, scope } = c { Some((*x, *y, scope.clone())) } else { None }) {
            match scope {
                Some(k) => { let i = self.nodes.iter().position(|n| n.key == k); self.snapshot_in(x, y, i); }
                None => self.snapshot_in(x, y, None),
            }
        }
        let old = flow.map(|_| self.record());
        // A morph from the last frame takes its source now, before the new view.
        if let Some(Cmd::Morph { to, spec, .. }) = cmds.iter().find(|c| matches!(c, Cmd::Morph { spec, .. } if spec.from_last.is_some())) { let (to, spec) = (to.clone(), spec.clone()); self.morph_from_last(&to, spec); }
        cmds.retain(|c| !matches!(c, Cmd::Morph { spec, .. } if spec.from_last.is_some()));
        // A morph from a stage's content takes its glyphs as the last frame drew them (the new view may
        // pose them elsewhere).
        let drawn: Vec<Vec<(String, Instance)>> = cmds.iter().filter_map(|c| if let Cmd::Morph { from, .. } = c { Some(self.staged_drawn(from)) } else { None }).collect();
        let mut drawn = drawn.into_iter();
        self.no_recompose = flow.is_some() || off.recompose;
        self.layout();
        if std::mem::take(&mut self.focus_moved) && let Some(f) = self.focused && f(&mut self.state, self.focus.as_deref()) { self.layout(); }
        self.morph_last();
        self.no_recompose = false;
        if let (Some(f), Some(old)) = (flow, old) { self.flow_from(old, f); } else { self.flow_late(); }
        for c in cmds {
            match c {
                Cmd::Morph { from, to, spec } => { let d = drawn.next().unwrap_or_default(); self.morph_drawn(&from, &to, spec, d) }
                Cmd::Ripplr { x, y, strength } => { let s = self.scale; let t = self.beat_grid(); self.field.front(x * s, y * s, t, strength); }
                // From an element's centre, reaching what that element's own press would reach.
                Cmd::RipplrAt { key, strength } => if let Some(i) = self.nodes.iter().position(|n| n.key == key) {
                    let (x, y) = self.nodes[i].rect.center();
                    self.front_from(Some(i), x, y, true, strength);
                },
                Cmd::Stir { at, v, energy, radius, scope } => self.stir_cmd(at, v, energy, radius, scope),
                Cmd::Gust { gust, scope } => {
                    self.fit_map();
                    // Across the window at most: the band's slant is measured over its scope, and it is
                    // gone when it has crossed it.
                    if let Some(b) = self.stir_scope(None, &scope) {
                        let b = [b[0].max(0.0), b[1].max(0.0), b[2].min(self.size.0), b[3].min(self.size.1)];
                        let f = self.timing.map.flow;
                        self.fmap.gust(gust, b, f);
                        self.dirty = true;
                    }
                }
                Cmd::Press { key } => { self.scripted = true; self.activate(&key); self.scripted = false; }
                Cmd::Pointer { x, y } => { let s = self.scale; self.field.pointer[0] = x * s; self.field.pointer[1] = y * s; self.dirty = true; }
                Cmd::Focus { key } => self.focus_key(&key),
                Cmd::ClearFocus => self.clear_focus(),
                Cmd::ClearPlate { key } => if let Some(k) = self.plate_slots.iter().position(|p| p.as_ref().is_some_and(|p| p.0 == *key)) { self.plate_clear.push(k as u8); self.dirty = true; },
                Cmd::ScrollTo { key } => self.scroll_to(&key),
                Cmd::Scrub(at) => self.scrub(at),
                Cmd::Hold(n) => {
                    let b = self.timing.beat;
                    self.bclock.hold(&b, self.wall, n);
                    // The frame presented stays as it is, byte for byte, until the hold ends.
                    if self.frozen.is_none() && b.on() { self.frozen = Some(self.scene.clone()); }
                    self.dirty = true;
                }
                Cmd::Retrigger => { let b = self.timing.beat; self.bclock.retrigger(&b, self.wall); self.clock = self.beat_of(self.wall); self.dirty = true; }
                Cmd::Rewind(n) => { let b = self.timing.beat; self.bclock.rewind(&b, self.wall, n); self.clock = self.beat_of(self.wall); self.dirty = true; }
                Cmd::Set { name, value } => {
                    if self.look().set(&name, value).is_ok() {
                        // A switch given 0 goes back to the look's own: its turns are dropped.
                        if value < 0.5 && name.starts_with("post.") { self.sets.retain(|(n, _)| *n != name); } else { self.sets.push((name, value)); }
                        self.timing = Timing::of(&self.look());
                        self.dirty = true;
                    }
                }
                Cmd::Flow(_) | Cmd::Commit { .. } => {}
            }
        }
        // A morph whose two elements are both gone (the page changed under it) ends: nothing is left
        // to fly from or to.
        if let Some(r) = &self.run && !self.nodes.iter().any(|n| n.key == r.from || n.key == r.to) { self.run = None; self.paint(); }
        self.flush();
    }

    /// Hold the morph in progress at a point of its flight, or let it go on from there. While held, the
    /// source and the target are not drawn and the tiles stay where the point puts them.
    pub fn scrub(&mut self, at: Option<f32>) {
        let now = self.clock;
        let tighten = self.run.as_ref().map_or(0.0, |r| r.spec.tighten_ms / 1000.0);
        let Some(r) = &mut self.run else { return };
        match at {
            Some(u) => {
                // A morph still loosening its source jumps to its flight.
                let len = r.flight.map_or(r.spec.duration_ms / 1000.0, |f| f[1] - f[0]);
                if r.flight.is_none() { r.flight = Some([now, now + len]); }
                let first = r.scrub.is_none();
                r.scrub = Some(u.clamp(0.0, 1.0));
                r.src_until = now.min(r.src_until);
                r.dst_from = NEVER;
                r.end = NEVER;
                // Moving a held morph only moves its clock: the tiles stay as they were uploaded, so a
                // scrub costs a uniform, not a new particle set.
                if !first { self.dirty = true; return; }
            }
            None => {
                let Some(u) = r.scrub.take() else { return };
                let len = r.flight.map_or(0.0, |f| f[1] - f[0]);
                let start = now - u * len;
                r.flight = Some([start, start + len]);
                r.dst_from = start + len;
                r.dst_ramp = [1.0, start + len, 0.0, start + len + tighten];
                r.end = start + len + tighten;
            }
        }
        r.upload = next_run();
        self.paint();
    }

    /// View → nodes → the taffy tree, kept from the last layout and changed only where the new view
    /// differs (DESIGN.md §1.13) → absolute rects → scene.
    fn layout(&mut self) {
        if self.look_of.is_some() { self.timing = Timing::of(&self.look()); }
        self.text.begin_pass();
        self.perf.counters.views += 1;
        if let Some(f) = self.locale_of { crate::i18n::set_current(f(&self.state)); }
        let mut el = (self.view)(&self.state);
        // Boxes a subtree is fitted into (`El::fit`): the subtree is first laid out on its own at 1 to
        // find its natural size, then the view is made again and the subtree zoomed to fit.
        if !has_fit(&el) { self.fit_k.clear(); self.fit_short.clear(); self.fit_last = None; }
        // The scales of last time, when nothing layout reads has changed since (the view zoomed by them is
        // what was laid out last): one view, no probe and no layout. A piece that changes only what it
        // draws pays nothing for being fitted.
        else if let Some(ks) = self.fit_last.take() {
            fit_apply(&mut el, &ks);
            match self.lay_pass_if(el, true) {
                Some(mut nodes) => {
                    self.fit_last = Some(ks);
                    self.remeasure = false;
                    self.finish_layout(&mut nodes);
                    return;
                }
                None => { self.perf.counters.views += 1; el = (self.view)(&self.state); }
            }
        }
        if has_fit(&el) {
            self.fit_short.clear();
            // A box that reflows (`FitMode::Reflow`) is probed again at the scale found, until it holds
            // its subtree (a smaller scale wraps it into fewer lines) or it is at its least scale.
            let mut ks: Vec<Fitted> = vec![];
            for pass in 0..4 {
                if pass > 0 { self.perf.counters.views += 1; el = (self.view)(&self.state); }
                let prev: Vec<f32> = ks.iter().map(|f| f.k).collect();
                let nat: Vec<Option<(f32, f32)>> = ks.iter().map(|f| f.nat).collect();
                fit_probe(&mut el, &prev, &mut 0);
                let probe = self.lay_pass(el);
                let again = self.fit_scales(&probe, &prev, &nat, &mut ks);
                el = (self.view)(&self.state);
                if !again { break; }
            }
            self.perf.counters.views += 1;
            fit_apply(&mut el, &ks);
            self.fit_last = Some(ks);
        }
        let mut nodes = self.lay_pass(el);
        self.remeasure = false;
        self.finish_layout(&mut nodes);
    }
    /// One layout pass of a view: lifted layers, the flattened nodes, the taffy tree brought up to date
    /// and laid out (when anything it depends on changed); each node's local box set.
    fn lay_pass(&mut self, el: El<M>) -> Vec<Node<M>> { self.lay_pass_if(el, false).unwrap_or_default() }
    /// `lay_pass`, or with `same` nothing when anything layout reads changed since the last pass (the
    /// tree is then brought up to date but not laid out).
    fn lay_pass_if(&mut self, el: El<M>, same: bool) -> Option<Vec<Node<M>>> {
        let mut nodes = Vec::new();
        // The root is the window, whatever its contents would like.
        let mut el = el;
        el.style.size = Size { width: length(self.size.0), height: length(self.size.1) };
        // Modal layers are lifted out of wherever they were declared and laid over the whole window, last
        // so that they are drawn on top.
        let mut layers = Vec::new();
        if el.preview { in_preview(&mut el); }
        hoist(&mut el, &mut layers);
        for mut l in layers {
            l.style.position = taffy::Position::Absolute;
            l.style.inset = taffy::Rect { left: length(0.0), right: length(0.0), top: length(0.0), bottom: length(0.0) };
            l.style.size = Size { width: length(self.size.0), height: length(self.size.1) };
            el.children.push(l);
        }
        // Virtual lists build the rows their viewport shows, at their scroll offset (the viewport as it
        // was last laid out; the window's height the first time).
        let mut vx = VirtCtx { scroll: &self.scroll, views: &self.virt_views, window: self.size.1, made: HashMap::default() };
        let mut built = Built::default();
        self.root_roles.set(el.props.fonts.unwrap_or([0; 3]));
        flatten(el, None, "root".into(), [0; 3], (false, false), None, Inherit { roles: self.timing.roles, ..Inherit::default() }, &mut vx, &mut built, &mut nodes);
        self.virt_made = vx.made;
        self.stack_margins(&nodes, &mut built);

        // The taffy tree: a node whose key was there last time keeps its taffy node; its style, children
        // and measured content are compared and only what differs is set (which marks it and its
        // ancestors dirty). Children come after their parents in `nodes`, so going backwards every
        // node's children have their taffy nodes already.
        let mut changed = false;
        let mut touched = 0usize;
        let mut used: HashSet<NodeId> = HashSet::with_capacity_and_hasher(nodes.len(), Default::default());
        let mut tids: HashMap<String, NodeId> = HashMap::with_capacity_and_hasher(nodes.len(), Default::default());
        for i in (0..nodes.len()).rev() {
            let was = std::mem::replace(&mut changed, false);
            let leaf = !matches!(nodes[i].content, Content::None);
            let style = std::mem::take(&mut built.styles[i]);
            let mk = measure_key(&nodes[i].content);
            let kids: Vec<NodeId> = built.kids[i].iter().map(|&c| nodes[c].tid).collect();
            let old = self.tids.get(&nodes[i].key).copied().filter(|t| !used.contains(t));
            let tid = match old {
                Some(t) => {
                    if self.tree.style(t).map_or(true, |o| *o != style) { let _ = self.tree.set_style(t, style); changed = true; }
                    if leaf {
                        if self.tree.child_count(t) > 0 { let _ = self.tree.set_children(t, &[]); changed = true; }
                        let _ = self.tree.set_node_context(t, Some(i));
                        if self.remeasure || self.mkeys.get(&t) != Some(&mk) { let _ = self.tree.mark_dirty(t); changed = true; }
                    } else {
                        let _ = self.tree.set_node_context(t, None);
                        if self.tree.children(t).map_or(true, |c| c != kids) { let _ = self.tree.set_children(t, &kids); changed = true; }
                    }
                    t
                }
                None => {
                    changed = true;
                    if leaf { self.tree.new_leaf_with_context(style, i) } else { self.tree.new_with_children(style, &kids) }.expect("taffy creates a node for every element (it only fails when out of memory)")
                }
            };
            if changed { touched += 1; }
            changed |= was;
            used.insert(tid);
            self.mkeys.insert(tid, mk);
            nodes[i].tid = tid;
            tids.insert(nodes[i].key.clone(), tid);
        }
        // What is gone from the view goes from the tree. A node that moved from a parent that is going
        // to a parent that stays is still in the old parent's list (taffy does not take it out when
        // the new parent is set), so the going nodes first drop the children that stay, and the parents
        // that hold those are set again.
        let gone: Vec<NodeId> = self.tids.drain().map(|(_, t)| t).filter(|t| !used.contains(t)).collect();
        let mut moved: HashSet<NodeId> = HashSet::default();
        for &t in &gone {
            let kids = self.tree.children(t).unwrap_or_default();
            if kids.iter().any(|k| used.contains(k)) {
                moved.extend(kids.iter().copied().filter(|k| used.contains(k)));
                let keep: Vec<NodeId> = kids.into_iter().filter(|k| !used.contains(k)).collect();
                let _ = self.tree.set_children(t, &keep);
            }
        }
        for t in gone { let _ = self.tree.remove(t); self.mkeys.remove(&t); self.moved.remove(&t); changed = true; }
        if !moved.is_empty() {
            for i in 0..nodes.len() {
                let kids: Vec<NodeId> = built.kids[i].iter().map(|&c| nodes[c].tid).collect();
                if kids.iter().any(|k| moved.contains(k)) { let _ = self.tree.set_children(nodes[i].tid, &kids); }
            }
        }
        self.tids = tids;
        let root = nodes[0].tid;
        // Nothing changed that layout depends on (taffy's own dirty flag is not kept for the root): the
        // boxes from last time stand.
        if changed && same { return None; }
        if changed {
            let space = Size { width: AvailableSpace::Definite(self.size.0), height: AvailableSpace::Definite(self.size.1) };
            self.perf.counters.layouts += 1;
            self.moved.clear();
            // Laid out by a function of the leaves alone, so taffy is built once and not for each `Ui` type.
            let leaves: Vec<(&Content, u16)> = nodes.iter().map(|n| (&n.content, n.clamp)).collect();
            if compute_layout(&mut self.tree, root, space, &mut self.text, &leaves).is_err() { return Some(nodes); }
            // A stack's children lie over one another by their margins, which come from the widths they
            // took: when those changed, they are set again and the page laid out again (a child's width
            // can depend on the others' once they no longer sit side by side, so up to three times).
            for _ in 0..3 {
                if !self.restack(&nodes, &built) { break; }
                if compute_layout(&mut self.tree, root, space, &mut self.text, &leaves).is_err() { return Some(nodes); }
            }
            if !built.stacks.is_empty() || !self.stack_w.is_empty() {
                let keys: HashSet<&str> = built.stacks.iter().map(|&i| nodes[i].key.as_str()).collect();
                self.stack_w.retain(|k, _| keys.contains(k.as_str()));
            }
        } else {
            self.perf.counters.layouts_skipped += 1;
        }
        self.rebuilt = touched;
        for n in nodes.iter_mut() {
            if let Ok(l) = self.tree.layout(n.tid) { n.local = *l; let u = self.tree.unrounded_layout(n.tid); n.exact = (u.location.x, u.location.y); }
            // Not laid out since a restage moved it: where the restage put it.
            if let Some(&e) = self.moved.get(&n.tid) { n.exact = e; n.local.location = taffy::Point { x: e.0.round(), y: e.1.round() }; }
        }
        Some(nodes)
    }
    /// How much each box marked `El::fit` zooms its subtree, in the order they come in the view, from
    /// the pass that laid the subtree out on its own: the scale, and where the zoomed subtree goes in
    /// the box (left and top insets, logical px). A box that cannot hold its subtree at the least scale
    /// is told in `Ui::problems`.
    /// `prev` holds the scales a reflowing box was probed at (none on the first pass), `nat` the sizes
    /// first measured of subtrees with own-size elements; `out` is replaced. Returns whether a box wants
    /// another probe at its new scale.
    fn fit_scales(&mut self, probe: &[Node<M>], prev: &[f32], nat: &[Option<(f32, f32)>], out: &mut Vec<Fitted>) -> bool {
        out.clear();
        let mut again = false;
        let mut at = 0usize;
        self.fit_k.clear();
        self.fit_short.clear();
        for (i, n) in probe.iter().enumerate() {
            let Some(f) = n.fit else { continue };
            let l = n.local;
            // Insets are from the padding box (inside the border); the content box is the padding in.
            let (pl, pt) = (l.padding.left, l.padding.top);
            let (cw, ch) = ((l.size.width - l.border.left - l.border.right - pl - l.padding.right).max(0.0), (l.size.height - l.border.top - l.border.bottom - pt - l.padding.bottom).max(0.0));
            // The subtree is in the holder `fit_probe` put it in.
            let kp = prev.get(at).copied().unwrap_or(1.0);
            let nat0 = nat.get(at).copied().flatten();
            at += 1;
            if f.mode == crate::el::FitMode::Reflow {
                // Laid out in place at `kp`: what reaches out of the content box (not what scrolls
                // inside a scroller), from its centre, as the subtree is centred in it.
                let Some(c) = probe.iter().position(|m| m.parent == Some(i)) else { out.push(Fitted { k: 1.0, at: (pl, pt), pixel: vec![], w: Some((cw, ch)), nat: None }); continue };
                let mut pos = vec![(0.0f32, 0.0f32); probe.len()];
                let (mut x0, mut y0, mut x1, mut y1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                let (bx, by0) = (l.border.left + pl, l.border.top + pt);
                for j in c..probe.len() {
                    if !within(probe, j, c) { break; }
                    let up = probe[j].parent.unwrap_or(i);
                    let base = if j == c { (-bx, -by0) } else { pos[up] };
                    pos[j] = (base.0 + probe[j].local.location.x, base.1 + probe[j].local.location.y);
                    // Under a scroller (below `c`): its rows are not extent.
                    let mut q = up; let mut scrolled = false;
                    while q != c && q != i { if probe[q].scroll { scrolled = true; break; } q = probe[q].parent.unwrap_or(i); }
                    if scrolled { continue; }
                    let sz = probe[j].local.size;
                    if sz.width <= 0.0 || sz.height <= 0.0 { continue; }
                    if j == c { (x0, y0, x1, y1) = (pos[j].0, pos[j].1, pos[j].0 + sz.width, pos[j].1 + sz.height); }
                    else { x0 = x0.min(pos[j].0); y0 = y0.min(pos[j].1); x1 = x1.max(pos[j].0 + sz.width); y1 = y1.max(pos[j].1 + sz.height); }
                }
                let (hx, hy) = ((cw * 0.5 - x0).max(x1 - cw * 0.5).max(1e-3), (ch * 0.5 - y0).max(y1 - ch * 0.5).max(1e-3));
                // Within a pixel is a fit (a box's edges are on whole pixels, its contents on 1/64 px).
                let fits = hx <= cw * 0.5 + 1.0 && hy <= ch * 0.5 + 1.0;
                let ratio = ((cw * 0.5 - 0.5) / hx).min((ch * 0.5 - 0.5) / hy);
                let k = if fits { kp } else { (kp * ratio).clamp(f.min.min(1.0), kp) };
                if (k - kp).abs() > 1e-3 { again = true; }
                if !fits && k <= f.min + 1e-4 && (kp - f.min).abs() < 1e-3 { self.fit_short.push((n.key.clone(), kp * ratio, f.min)); }
                // Pixel text is zoomed with the rest: a reflow scales by a little, and a pixel face's
                // next whole size down would halve it.
                self.fit_k.insert(n.key.clone(), k);
                out.push(Fitted { k, at: (pl, pt), pixel: vec![], w: Some((cw, ch)), nat: None });
                continue;
            }
            let Some(c) = probe.iter().position(|m| m.parent == Some(i)).and_then(|h| probe.iter().position(|m| m.parent == Some(h))) else { out.push(Fitted { k: 1.0, at: (pl, pt), pixel: vec![], w: None, nat: None }); continue };
            let (mut nw, mut nh) = (probe[c].local.size.width.max(1e-3), probe[c].local.size.height.max(1e-3));
            // Own-size elements (`El::own_size`) do not scale with the rest, so the subtree's size is not
            // its size at 1 times the scale: it is measured again zoomed by the first scale found, and the
            // scale is solved from the two sizes (each a fixed part plus a part that scales).
            let own = probe.iter().enumerate().skip(c).take_while(|(j, _)| within(probe, *j, c)).any(|(_, m)| m.own_size.is_some());
            let mut nat = None;
            let mut lin: Option<([f32; 2], [f32; 2])> = None;
            if own {
                nat = nat0.or(Some((nw, nh)));
                if let Some((w1, h1)) = nat0 && (kp - 1.0).abs() > 1e-4 {
                    let b = [((w1 - nw) / (1.0 - kp)).max(1e-3), ((h1 - nh) / (1.0 - kp)).max(1e-3)];
                    lin = Some(([w1 - b[0], h1 - b[1]], b));
                }
            }
            // Text does not scale exactly with its size (advances and line heights snap): room of a
            // pixel at the scale is kept so the zoomed subtree is never larger than the box.
            let by = |w: f32, h: f32| match f.mode {
                crate::el::FitMode::Contain => (w / nw).min(h / nh),
                crate::el::FitMode::Width => w / nw,
                crate::el::FitMode::Height | crate::el::FitMode::Reflow => h / nh,
            };
            let room = by(cw, ch).max(1.0);
            let raw = match lin {
                Some((a, b)) => {
                    let (sx, sy) = (((cw - 1.0).max(0.0) - a[0]) / b[0], ((ch - 1.0).max(0.0) - a[1]) / b[1]);
                    match f.mode { crate::el::FitMode::Contain => sx.min(sy), crate::el::FitMode::Width => sx, _ => sy }.max(0.0)
                }
                None => by((cw - room).max(0.0), (ch - room).max(0.0)),
            };
            let k = raw.clamp(f.min, f.max.max(f.min));
            if let Some((a, b)) = lin { (nw, nh) = ((a[0] + b[0] * k) / k, (a[1] + b[1] * k) / k); }
            // Measured at 1 with own-size elements in it: once more at the scale found.
            if own && nat0.is_none() && (k - 1.0).abs() > 1e-3 { again = true; }
            // What does not fit at the least scale, in the directions the mode fits (exactly the least
            // scale and in the box is not short).
            let (over_x, over_y) = (k * nw > cw + 0.5, k * nh > ch + 0.5);
            let short = match f.mode { crate::el::FitMode::Contain | crate::el::FitMode::Reflow => over_x || over_y, crate::el::FitMode::Width => over_x, crate::el::FitMode::Height => over_y };
            if raw < f.min && short { self.fit_short.push((n.key.clone(), raw, f.min)); }
            // Pixel faces are drawn at whole multiples of their native size: each pixel text in the
            // subtree goes down to the largest that is no larger than the scale makes it, so it never
            // overflows; the scale itself is not changed.
            let mut pixel = vec![];
            // Own-size type is not zoomed, and a probe zoomed by `kp` holds sizes `kp` times their own.
            let kz = if lin.is_some() { kp } else { 1.0 };
            for (j, m) in probe.iter().enumerate().skip(c).take_while(|(j, _)| within(probe, *j, c)) {
                let Some(p) = m.para() else { continue };
                if self.text.native(p.face).is_none() || p.face & crate::text::FINE != 0 || k >= 1.0 - 1e-4 { continue };
                if own && probe.iter().enumerate().skip(c).take(j + 1 - c).any(|(q, o)| o.own_size.is_some() && within(probe, j, q)) { continue; }
                let at1 = p.size / kz;
                pixel.push((p.s.to_owned(), at1, self.text.size_down(p.face, k * at1)));
            }
            let (ax, ay) = f.align;
            self.fit_k.insert(n.key.clone(), k);
            // On whole pixels, as the layout puts boxes; in a direction it overflows, from the start, so
            // its beginning is seen and its end is cut.
            let x = if over_x { pl } else { pl + ax.clamp(0.0, 1.0) * (cw - k * nw) };
            let y = if over_y { pt } else { pt + ay.clamp(0.0, 1.0) * (ch - k * nh) };
            out.push(Fitted { k, at: (x.round(), y.round()), pixel, w: None, nat });
        }
        again
    }
    fn finish_layout(&mut self, nodes: &mut Vec<Node<M>>) {
        let nodes = std::mem::take(nodes);
        self.forget_gone(&nodes);
        self.nodes = nodes;
        self.place();
        // What clamped text shows, at the width it is drawn at.
        for i in 0..self.nodes.len() {
            if self.nodes[i].clamp == 0 { continue; }
            self.nodes[i].shown = None;
            let around = matches!(&self.nodes[i].content, Content::Text { around: Some(_), .. });
            if around { continue; }
            let cut = self.nodes[i].para().and_then(|p| if p.width.is_finite() { self.text.clamp_lines(p, self.nodes[i].clamp) } else { None });
            self.nodes[i].shown = cut;
        }
        self.modal_at = self.nodes.iter().rposition(|n| n.modal.is_some());
        self.layer_changed();
        self.paint();
    }

    /// What the elements that are about to go leave behind (DESIGN.md §1.29): a keyed element that goes
    /// with the element it was in (its nearest keyed ancestor is gone too) went with its page, and leaves
    /// nothing; where such a part of the page put something in the field map (it stirred it, drew a flow
    /// or a decode, or was a ripple's scope), the map is cleared over its box. When none of the old keyed
    /// elements is left, the whole map is.
    fn forget_gone(&mut self, next: &[Node<M>]) {
        self.left.clear();
        let kept: std::collections::HashSet<&str> = next.iter().filter(|n| n.keyed).map(|n| n.key.as_str()).collect();
        let old = &self.nodes;
        let keyed_up = |mut i: usize| -> Option<&str> { while let Some(p) = old[i].parent { if old[p].keyed { return Some(old[p].key.as_str()); } i = p; } None };
        let mut clear: Vec<[f32; 4]> = vec![];
        let mut any_kept = false;
        for (i, n) in old.iter().enumerate() {
            if !n.keyed { continue; }
            if kept.contains(n.key.as_str()) { any_kept = true; continue; }
            let up = keyed_up(i);
            if up.is_some_and(|k| !kept.contains(k)) { self.left.insert(n.key.clone()); }
            // The root of what went: did it (or anything in it) put something in the map?
            if up.is_some_and(|k| !kept.contains(k)) { continue; }
            let marks = old.iter().enumerate().any(|(j, m)| within(old, j, i) && (m.stirr.is_some() || m.flow_field.is_some() || m.decode.is_some() || m.ripple_scope));
            if marks { let r = n.rect.intersect(&n.clip); clear.push([r.x, r.y, r.right(), r.bottom()]); }
            // Its own decode goes with it when its container went too.
        }
        for k in &self.left { self.decodes.remove(k); }
        if !any_kept && old.iter().any(|n| n.keyed) {
            clear = vec![crate::fieldmap::EVERYWHERE];
            // A press's front still running belongs to the press, not to what it pressed: it goes on
            // over the new page, at half its strength, as a whole ring (its old scope went with its page).
            for k in 0..self.field.fronts.len() {
                if self.field.fronts[k][3] > 0.0 {
                    self.field.scopes[k] = crate::scene::EVERYWHERE;
                    self.field.fronts[k][3] *= 0.5;
                    self.field.sources[k] = 0;
                }
            }
        }
        for b in clear { self.fmap.clear_in(b); }
    }
    /// Absolute rects, clips and scroll extents from the laid-out (relative) boxes and the scroll
    /// offsets: all that scrolling changes. No view, no layout, no shaping.
    fn place(&mut self) {
        let nodes = &mut self.nodes;
        // How long each scroll node's contents are, so stored offsets can be clamped before placing.
        for n in nodes.iter_mut() { n.extent = 0.0; }
        for i in 0..nodes.len() {
            let l = nodes[i].local;
            if let Some(p) = nodes[i].parent && nodes[p].scroll {
                let pl = nodes[p].local;
                let end = if nodes[p].horizontal { l.location.x + l.size.width + pl.padding.right + pl.border.right }
                          else { l.location.y + l.size.height + pl.padding.bottom + pl.border.bottom };
                nodes[p].extent = nodes[p].extent.max(end);
            }
        }
        for n in nodes.iter().filter(|n| n.scroll) {
            let size = if n.horizontal { n.local.size.width } else { n.local.size.height };
            let max = (n.extent - size).max(0.0);
            if let Some(o) = self.scroll.get_mut(&n.key) { *o = o.clamp(0.0, max); }
        }
        let screen = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let mut inside = vec![screen; nodes.len()];
        // The corner radius of each node's inside clip (`El::clip_round`).
        let mut inside_r = vec![0.0f32; nodes.len()];
        // Each node's top left before taffy rounded it, so an own-size element's anchor is placed exactly.
        let mut exact = vec![(0.0f32, 0.0f32); nodes.len()];
        for i in 0..nodes.len() {
            let l = nodes[i].local;
            let (origin, clip, up) = match nodes[i].parent {
                Some(p) => {
                    // Content moves by whole device pixels, so a scroll never resamples it (DESIGN.md §1.8.3).
                    let off = if nodes[p].scroll { (self.scroll.get(&nodes[p].key).copied().unwrap_or(0.0) * self.scale).round() / self.scale } else { 0.0 };
                    let (ox, oy) = if nodes[p].horizontal { (off, 0.0) } else { (0.0, off) };
                    ((nodes[p].rect.x - ox, nodes[p].rect.y - oy), inside[p], (exact[p].0 - ox, exact[p].1 - oy))
                }
                None => ((0.0, 0.0), screen, (0.0, 0.0)),
            };
            exact[i] = (up.0 + nodes[i].exact.0 + nodes[i].shift.0, up.1 + nodes[i].exact.1 + nodes[i].shift.1);
            // An anchored element goes just below its anchor (laid out earlier in this pass), kept
            // inside the window.
            let at = nodes[i].anchor.as_ref().and_then(|k| nodes[..i].iter().find(|n| &n.key == k)).map(|a| a.rect);
            let up_xf = nodes[i].parent.and_then(|p| nodes[p].xf);
            let n = &mut nodes[i];
            n.rect = Rect::new(origin.0 + l.location.x + n.shift.0, origin.1 + l.location.y + n.shift.1, l.size.width, l.size.height);
            // At its own size in a zoomed subtree: its anchor where the zoomed box would put it.
            // Placed from where taffy put it before rounding, then on whole device pixels, so its type
            // never sits on half a pixel.
            if let Some((a, k)) = n.own_size {
                n.rect.x = ((exact[i].0 + a[0] * l.size.width * (k - 1.0)) * self.scale).round() / self.scale;
                n.rect.y = ((exact[i].1 + a[1] * l.size.height * (k - 1.0)) * self.scale).round() / self.scale;
            }
            if let Some(a) = at {
                n.rect.x = a.x.min(screen.w - l.size.width - 8.0).max(0.0);
                n.rect.y = a.bottom() + 4.0;
            }
            n.clip = clip;
            // Turned after layout: its subtree is drawn and hit through the turns above it and its own.
            n.xf = crate::hit::turned(up_xf, n.turn, &n.rect);
            let (b, p) = (l.border, l.padding);
            let inner_border = n.rect.inset(b.left, b.top, b.right, b.bottom);
            n.inner = inner_border.inset(p.left, p.top, p.right, p.bottom);
            // A box clips inside its border, so what it holds never covers it.
            inside[i] = if n.preview { clip.intersect(&inner_border.inset(1.0, 1.0, 1.0, 1.0)) } else if n.scroll || n.clips { clip.intersect(&inner_border) } else { clip };
            // A round clip (`El::clip_round`) is the box's; a square one inside it that cuts it is square.
            n.clip_r = n.parent.map_or(0.0, |p| inside_r[p]);
            inside_r[i] = if n.clip_radius > 0.0 && n.clips { n.clip_radius } else if inside[i] == clip { n.clip_r } else { 0.0 };
            if n.virt.is_some() { self.virt_views.insert(n.key.clone(), n.rect.h); }
        }
        // Who came into view or left it (`El::on_visible`); what is gone is forgotten, so it is told
        // again when it comes back.
        let keys: HashSet<&str> = self.nodes.iter().filter(|n| n.on_visible.is_some()).map(|n| n.key.as_str()).collect();
        self.seen.retain(|k, _| keys.contains(k.as_str()));
        for n in self.nodes.iter().filter(|n| n.on_visible.is_some()) {
            let r = n.rect.intersect(&n.clip);
            let v = !n.hidden && r.w > 0.0 && r.h > 0.0;
            if self.seen.get(&n.key) != Some(&v) {
                self.seen.insert(n.key.clone(), v);
                if let Some(m) = n.on_visible.as_ref().and_then(|f| f(v)) { self.queued.push(m); }
            }
        }
    }
    /// Send what the runtime itself had to say after a layout or paint (visibility, outlines), then lay
    /// out once for all of it.
    fn flush(&mut self) {
        if !self.flushing && let Some(f) = self.kept_to && !self.kept_flips.is_empty() {
            for (k, kept) in std::mem::take(&mut self.kept_flips) { f(&mut self.state, &k, kept); }
            self.relayout();
        }
        if self.flushing || self.queued.is_empty() { return; }
        self.flushing = true;
        for _ in 0..4 {
            if self.queued.is_empty() { break; }
            for m in std::mem::take(&mut self.queued) { (self.update)(&mut self.state, m); }
            self.relayout();
        }
        self.queued.clear();
        self.flushing = false;
    }
    /// The pens this paint laid for each `on_pens` text, sent when they differ from those last sent; a
    /// text that went is forgotten, so it is told again when it comes back.
    fn pens_pass(&mut self) {
        let keys: HashSet<&str> = self.nodes.iter().filter(|n| n.on_pens.is_some()).map(|n| n.key.as_str()).collect();
        self.pens_told.retain(|k, _| keys.contains(k.as_str()));
        for (i, v) in std::mem::take(&mut self.pens_found) {
            let n = &self.nodes[i];
            let Some(f) = &n.on_pens else { continue };
            if self.pens_told.get(&n.key).is_some_and(|o| o.len() == v.len() && o.iter().zip(&v).all(|(a, b)| (a - b).abs() < 1e-4)) { continue; }
            let v = Rc::new(v);
            self.pens_told.insert(n.key.clone(), v.clone());
            if let Some(m) = f(v) { self.queued.push(m); }
        }
    }
    /// The true outlines asked for (`El::on_outline`): each subtree's coverage on its grid, sent only
    /// when its instances changed.
    fn outline_pass(&mut self) {
        let s = self.scale;
        for i in 0..self.nodes.len() {
            let Some(o) = &self.nodes[i].on_outline else { continue };
            let nodes = &self.nodes;
            let insts: Vec<Instance> = self.scene.instances.iter().zip(&self.owners).filter(|(_, (k, _))| within(nodes, *k, i)).map(|(x, _)| *x)
                .chain(self.unseen.iter().filter(|(k, _)| within(nodes, *k, i)).map(|(_, x)| *x)).collect();
            let r = nodes[i].rect;
            let sig = {
                let mut h = DefaultHasher::new();
                (o.cols, o.rows, r.w.to_bits(), r.h.to_bits()).hash(&mut h);
                // Where it is in its own box, not on the page: scrolling it sends nothing.
                for x in &insts { for v in [x.rect[0] - r.x * s, x.rect[1] - r.y * s, x.rect[2], x.rect[3]].iter().chain(&x.uv).chain(&x.paint[..3]).chain(&[x.extra[0]]).chain(&x.xf) { v.to_bits().hash(&mut h); } }
                h.finish()
            };
            if self.outlines.get(&nodes[i].key) == Some(&sig) { continue; }
            self.outlines.insert(nodes[i].key.clone(), sig);
            let mut g = crate::field::Grid::new(o.cols, o.rows, r.w, r.h);
            for j in 0..g.rows { for k in 0..g.cols {
                let (x, y) = g.node(k, j);
                let (px, py) = ((r.x + x) * s, (r.y + y) * s);
                let open = insts.iter().fold(1.0, |a, inst| a * (1.0 - morph::cover_at(inst, px, py, &self.text.pixels).min(1.0)));
                g.v[j * g.cols + k] = 1.0 - open;
            } }
            if let Some(m) = (o.f)(Rc::new(g)) { self.queued.push(m); }
        }
    }

    /// After a scroll offset changed: move everything (no layout) — unless a virtual list scrolled past
    /// the rows it has built, which then builds the rows it now shows.
    fn scrolled(&mut self) {
        let past = self.nodes.iter().any(|n| {
            let (Some(v), Some(&(a, b))) = (&n.virt, self.virt_made.get(&n.key)) else { return false };
            let off = self.scroll.get(&n.key).copied().unwrap_or(0.0);
            let (first, last) = visible_rows(off, n.rect.h, v.0, v.1);
            first < a || last > b
        });
        self.scroll_paint = true;
        if past { self.relayout(); } else { self.place(); self.paint(); }
        self.scroll_paint = false;
        self.tell_scrolled();
        self.flush();
    }

    /// Tell the state where the scroll nodes are (`on_scrolled`).
    fn tell_scrolled(&mut self) {
        let Some(f) = self.scrolled_of else { return };
        let mut changed = false;
        let scrollers: Vec<(String, Rect, f32)> = self.nodes.iter().filter(|n| n.scroll && !n.horizontal).map(|n| (n.key.clone(), n.inner, self.scroll.get(&n.key).copied().unwrap_or(0.0))).collect();
        for (k, r, off) in scrollers {
            let nodes = &self.nodes;
            let at = |key: &str| nodes.iter().find(|n| n.key == key).map(|n| n.rect.y - r.y);
            changed |= f(&mut self.state, &k, off, &at);
        }
        if changed { self.relayout(); }
    }
    /// Scroll the nearest scroll ancestor of `key` so the element's top is at the scroller's top.
    fn scroll_to(&mut self, key: &str) {
        let Some(i) = self.nodes.iter().position(|n| n.key == key) else { return };
        let Some(sc) = self.up(i, |n| n.scroll && !n.horizontal).filter(|&s| s != i) else { return };
        let d = self.nodes[i].rect.y - self.nodes[sc].inner.y - 8.0;
        if self.scroll_by(sc, d) { self.scrolled(); }
    }
    /// A modal layer opened or closed: focus moves into it (remembering where it was), or back.
    fn layer_changed(&mut self) {
        let key = self.modal_at.map(|m| self.nodes[m].key.clone());
        if key == self.modal_key { return; }
        let had = self.focus.clone();
        match self.modal_at {
            Some(m) => {
                if self.modal_key.is_none() { self.return_focus = self.focus.clone(); }
                // The first field that takes keys (a text field), else the first control.
                let first = (m..self.nodes.len()).find(|&i| self.nodes[i].focusable && (self.nodes[i].on_key.is_some() || self.nodes[i].on_text.is_some()))
                    .or_else(|| (m..self.nodes.len()).find(|&i| self.nodes[i].focusable));
                self.focus = first.map(|i| self.nodes[i].key.clone());
            }
            None => {
                let back = self.return_focus.take();
                self.focus = back.filter(|k| self.nodes.iter().any(|n| &n.key == k && n.focusable));
            }
        }
        if self.focus != had { self.focus_moved = true; }
        self.modal_key = key;
    }

    /// Nodes → instances. Identity is key + part (+ glyph id), so unchanged things keep their birth time.
    fn paint(&mut self) {
        if self.defer > 0 { self.stale_paint = true; self.dirty = true; return; }
        self.stale_paint = false;
        self.show_clear();
        self.sweeping = false;
        self.swept_nodes.clear();
        self.paint_once();
        // The glyph atlas filled up (many sizes over a long run): start it over and paint again, so every
        // glyph on the page is in the new one. What was kept from before (tiles, ghosts) is dropped.
        if self.text.is_full() {
            self.text.refill();
            self.recomps.clear();
            self.strand_cache.clear();
            self.recs.clear();
            self.ghosts.clear();
            self.paint_once();
        }
    }
    fn paint_once(&mut self) {
        let t0 = crate::perf::now_ms();
        self.painting = true;
        // The scene is new: nothing in it is faded by scribblr yet.
        self.strand_faded.clear();
        self.paint_inner();
        self.scroll_field();
        self.painting = false;
        self.perf.counters.scenes += 1;
        self.perf.pending.scene += (crate::perf::now_ms() - t0) as f32;
    }
    fn paint_inner(&mut self) {
        let sel = self.look().select;
        // Only some stages drawn again (`Ui::restage`): what the rest of the page drew last time.
        let mut kept = self.restaging.take().and_then(|roots| self.kept(roots));
        if kept.is_some() { self.perf.counters.restages += 1; }
        self.scene.clear();
        if let Some(k) = &mut kept { self.scene.meshes = std::mem::take(&mut k.meshes); }
        self.glyphs.clear();
        self.owners.clear();
        self.tags.clear();
        self.unseen.clear();
        let (s, now) = (self.scale, self.clock);
        let dev = |r: Rect| {
            let (x0, y0, x1, y1) = ((r.x * s).round(), (r.y * s).round(), (r.right() * s).round(), (r.bottom() * s).round());
            [x0, y0, x1 - x0, y1 - y0]
        };
        let (mut births, mut text_births): (HashMap<u64, f32>, HashMap<u64, f32>) = if kept.is_some() { (self.births.clone(), self.text_births.clone()) }
            else { (HashMap::with_capacity_and_hasher(self.births.len(), Default::default()), HashMap::with_capacity_and_hasher(self.text_births.len(), Default::default())) };
        if kept.is_none() { self.field.wipe = [0.0; 4]; }
        // What the stages drawn again had is found again as a whole paint finds it (from the last frame).
        if let Some(k) = &kept {
            for (j, id) in &k.owners { if k.has(&self.nodes, *j) { births.remove(id); } }
            for (j, n) in self.nodes.iter().enumerate() {
                if k.has(&self.nodes, j) { use std::hash::{Hash, Hasher}; let mut h = crate::fasthash::FastHasher::new(); n.key.hash(&mut h); text_births.remove(&h.finish()); }
            }
        }
        self.field.calm = [[0.0; 4]; 4];
        let mut calms = 0;
        let mut held = vec![false; self.nodes.len()];
        // Under the pointer (1) or held (2), itself or the control it is part of: what `El::fill_hover`
        // and `El::fill_held` answer.
        let mut hand = vec![0u8; self.nodes.len()];
        self.fill_state = self.under_now();
        let (over, grip, focused) = self.fill_state.clone().unwrap_or_default();
        // Under the pointer or with focus, itself or the control it is part of: what `El::border_hover` answers.
        let mut lit = vec![false; self.nodes.len()];
        // The drawings' own colours (`Drawing::palette`), in the order first drawn.
        let mut own_cols: Vec<[f32; 3]> = kept.as_ref().map_or(vec![], |k| k.own.clone());
        // `El::focus_shown` subtrees without focus inside are not drawn.
        let focus_at = self.focus.as_deref().and_then(|f| self.nodes.iter().position(|n| n.key == f));
        let mut veiled = vec![false; self.nodes.len()];
        // Whether a node is in the subtree of something that stirs the field map (it does not read the
        // map: a floating window would loosen itself by its own trail).
        let mut stirred = vec![false; self.nodes.len()];
        // The outermost control each node is in (`Node::piece`): legible things are pushed as one piece with it.
        let mut unit: Vec<Option<usize>> = vec![None; self.nodes.len()];
        // Everything beneath an open modal layer is held partly loose.
        let beneath = self.modal_at.and_then(|m| self.nodes[m].modal.as_ref()).map_or(0.0, |m| m.beneath);
        let caret_at = self.focus.as_deref().and_then(|f| self.nodes.iter().position(|n| n.key == f));
        // A morph decides whether its two elements are drawn, whatever the view says.
        let roots = self.run.as_ref().map(|r| (self.nodes.iter().position(|n| n.key == r.from), self.nodes.iter().position(|n| n.key == r.to)));
        let nodes = &self.nodes;
        self.roles = (0..nodes.len()).map(|i| match roots {
            Some((a, _)) if a.is_some_and(|a| within(nodes, i, a)) => 1,
            Some((_, b)) if b.is_some_and(|b| within(nodes, i, b)) => 2,
            _ => 0,
        }).collect();
        let mut counters: HashMap<usize, [u32; 2]> = HashMap::default();
        let (mut latest, mut any_loose) = (f32::NEG_INFINITY, false);
        let global = self.timing.material;
        let mut caret_found: Option<(usize, [f32; 4])> = kept.as_ref().and(self.caret).filter(|c| kept.as_ref().is_some_and(|k| !k.has(&self.nodes, c.0))).map(|(i, _, r)| (i, r));
        let mut jobs: Vec<RecJob> = vec![];
        let mut tx = std::mem::take(&mut self.text_scratch);
        let mut seen_recs: HashSet<String> = if kept.is_some() { self.recs.keys().cloned().collect() } else { HashSet::default() };
        let ro = self.sel_ranges();
        // A scroll node pulled past its end: what is near that edge loosens, the more the further it is
        // pulled (no box moves past the end).
        let mut pull_of: Vec<Option<(usize, f32)>> = vec![None; self.nodes.len()];
        for i in 0..self.nodes.len() {
            if let Some(p) = self.nodes[i].parent {
                pull_of[i] = match self.over.get(&self.nodes[p].key) { Some(&o) if self.nodes[p].scroll => Some((p, o)), _ => pull_of[p] };
            }
        }
        let max_over = self.scrolling.max_over;
        let pulled = |nodes: &[Node<M>], i: usize| pull_of[i].map_or(0.0, |(p, o)| {
            let (r, inner, h) = (nodes[i].rect, nodes[p].inner, nodes[p].horizontal);
            let dist = match (h, o < 0.0) {
                (false, true) => r.y - inner.y, (false, false) => inner.bottom() - r.bottom(),
                (true, true) => r.x - inner.x, (true, false) => inner.right() - r.right(),
            };
            0.85 * (o.abs() / max_over).min(1.0).sqrt() * (1.0 - dist.max(0.0) / PULL_REACH).max(0.0)
        });
        let mut group: Option<(usize, u32, f32)> = None;
        let plates = self.plate_plan();
        self.note_kept(&plates);
        let mut plate: Option<(usize, u32, u8)> = None;
        // The stages (`El::stage`): one that holds is its last image, and its subtree is not painted; the
        // one being drawn (its root, its first instance, when it began, ms).
        let (input, released) = self.stage_input;
        let mut stage_skip: Option<usize> = None;
        let mut stage_open: Option<(usize, usize, f64)> = None;
        let mut stage_seen: Vec<String> = vec![];
        // A cached subtree passed by (drawn from what its plate holds, nothing it draws from changed), and
        // the one being painted.
        let mut keep_skip: Option<usize> = None;
        let mut keep_open: Option<KeepOpen> = None;
        for i in 0..self.nodes.len() {
            if let Some(root) = stage_skip { if within(&self.nodes, i, root) { continue; } stage_skip = None; }
            if let Some(root) = keep_skip { if within(&self.nodes, i, root) { continue; } keep_skip = None; }
            if let Some((root, start, t0)) = stage_open && !within(&self.nodes, i, root) {
                self.stage_close(root, start, t0, &text_births, &seen_recs);
                stage_open = None;
            }
            // Restaging: a stage that did not step is the last paint's, as the rest of the page is.
            let copied = kept.as_ref().is_some_and(|k| !k.has(&self.nodes, i));
            let is_stage = self.nodes[i].stage.as_ref().is_some_and(|c| c.held);
            if copied && is_stage { stage_seen.push(self.nodes[i].key.clone()); }
            if stage_open.is_none() && is_stage && !copied {
                let key = self.nodes[i].key.clone();
                let size = (i + 1..self.nodes.len()).take_while(|&j| within(&self.nodes, j, i)).count() + 1;
                let (rect, clip) = (self.nodes[i].rect, self.nodes[i].clip);
                stage_seen.push(key.clone());
                let at = self.stages.iter().position(|(k, _)| *k == key).unwrap_or_else(|| { self.stages.push((key, Default::default())); self.stages.len() - 1 });
                let st = &mut self.stages[at].1;
                if st.draws_now(rect, clip, size, now, input, released) {
                    let t0 = crate::perf::now_ms();
                    // For checks: a stage made slower than it is (`Ui::slow_stage`).
                    // It costs by its pixels: at a lower scale, less.
                    let k = if self.perf.stage_scale > 0.0 { self.perf.stage_scale / self.scale.max(0.01) } else { 1.0 };
                    while crate::perf::now_ms() - t0 < (self.stage_add_ms * k * k) as f64 {}
                    stage_open = Some((i, self.scene.instances.len(), t0));
                } else {
                    st.behind = true;
                    let base = self.scene.instances.len() as u32;
                    for (k, inst) in st.snap.iter().enumerate() {
                        let (rel, id) = st.owners[k];
                        self.scene.push(*inst);
                        self.owners.push((i + rel, id));
                        self.tags.push(st.tags[k].clone());
                        if let Some(&b) = self.births.get(&id) { births.insert(id, b); }
                    }
                    for g in &st.groups { self.scene.groups.push(crate::scene::Group { start: g.start + base, end: g.end + base, opacity: g.opacity }); }
                    for &(h, b) in &st.texts { text_births.insert(h, b); }
                    for k in &st.recs { seen_recs.insert(k.clone()); }
                    stage_skip = Some(i);
                    continue;
                }
            }
            // A plate's subtree (`El::plate`): its instances, kept by the renderer.
            if let Some((root, start, slot)) = plate && !within(&self.nodes, i, root) {
                if let Some(o) = keep_open.take() { self.keep_close(o, &births, &text_births, &jobs, [calms, own_cols.len()]); }
                self.plate_span(root, slot, start);
                plate = None;
            }
            if plate.is_none() && let Some(&slot) = plates.get(&i) {
                let start = self.scene.instances.len();
                if self.nodes[i].cached && kept.is_none() {
                    let n = &self.nodes[i];
                    let p = n.parent;
                    let pull: Vec<f32> = (i..self.nodes.len()).take_while(|&j| within(&self.nodes, j, i)).map(|j| pulled(&self.nodes, j)).collect();
                    let unit_r = p.and_then(|p| unit[p]).map(|u| { let r = self.nodes[u].rect; [r.x, r.y, r.w, r.h].map(f32::to_bits) });
                    let ctx = (s.to_bits(), p.is_some_and(|p| held[p]), p.is_some_and(|p| veiled[p]), p.is_some_and(|p| stirred[p]), unit_r, self.modal_at.is_some_and(|m| i < m), beneath.to_bits());
                    let sig = self.keep_sig(i, ctx, &pull, &ro);
                    let k = slot as usize;
                    if let Some(sig) = sig && self.plate_sig[k].is_some() && !self.plate_clear.contains(&slot) && let Some(keep) = &self.plate_keep[k] && keep.sig == sig && keep.timing == self.timing {
                        // Laid on from the plate: what it drew besides is as it was.
                        keep.lay(i, &mut births, &mut text_births, &mut seen_recs, &mut self.glyphs.laid);
                        latest = latest.max(keep.latest);
                        any_loose |= keep.loose;
                        let fade = self.plate_slots[k].as_ref().map_or(0.0, |p| p.2);
                        self.scene.plates.push(crate::scene::PlateSpan { slot, start: start as u32, end: start as u32, fade_s: fade, replace: fade == 0.0 });
                        self.perf.counters.kept += 1;
                        keep_skip = Some(i);
                        continue;
                    }
                    let lens = [self.unseen.len(), self.scene.groups.len(), self.scene.meshes.len(), self.glyphs.laid.len(), jobs.len(), calms, own_cols.len()];
                    keep_open = Some(KeepOpen { root: i, slot, start, sig, lens });
                }
                plate = Some((i, start as u32, slot));
            }
            let n = &self.nodes[i];
            if Some(i) == self.modal_at { self.scene.begin_layer(); }
            // A filled element that leaves a trail (a floating window, a sheet's panel) is drawn over the
            // page like the modal layer: where it has ink it replaces the near image, so the loose
            // decoration it passes over never shows through it as loose marks (DESIGN.md §1.29).
            if self.modal_at.is_none() && self.scene.layer.is_none() && n.stirr.is_some() && n.fill.is_some() && !n.hidden { self.scene.begin_layer(); }
            veiled[i] = n.parent.is_some_and(|p| veiled[p]) || n.focus_shown && !focus_at.is_some_and(|f| within(&self.nodes, f, i));
            hand[i] = if grip.as_deref() == Some(n.key.as_str()) { 2 } else if over.as_deref() == Some(n.key.as_str()) { 1 } else { 0 }.max(n.parent.map_or(0, |p| hand[p]));
            lit[i] = hand[i] > 0 || focused.as_deref() == Some(n.key.as_str()) || n.parent.is_some_and(|p| lit[p]);
            held[i] = !self.timing.off.loosen && (self.pressed.as_deref() == Some(n.key.as_str()) || n.parent.is_some_and(|p| held[p]));
            stirred[i] = n.stirr.is_some() || n.parent.is_some_and(|p| stirred[p]);
            unit[i] = n.parent.and_then(|p| unit[p]).or(n.piece(self.size).then_some(i));
            // A see-through subtree (`El::opacity`) is drawn as one group: it ends where the subtree does.
            if let Some((root, start, op)) = group && !within(&self.nodes, i, root) {
                let end = self.scene.instances.len() as u32;
                if end > start { self.scene.groups.push(crate::scene::Group { start, end, opacity: op }); }
                group = None;
            }
            // Fully see-through: not drawn (a screen reader still finds it).
            if n.opacity_all <= 0.0 { continue; }
            if group.is_none() && n.opacity < 1.0 { group = Some((i, self.scene.instances.len() as u32, n.opacity_all)); }
            // Within a group, a see-through descendant is made so by its own alpha.
            let fade = group.map_or(1.0, |(_, _, op)| n.opacity_all / op.max(1e-6));
            let seen = n.rect.intersect(&n.clip);
            // Text whose glyphs are posed (`El::pose`) may be drawn away from its box: it is culled by
            // where its glyphs go (below), not by its box.
            let posed_text = matches!(n.content, Content::Text { pose: Some(_), .. });
            let unseen = seen.w <= 0.0 || seen.h <= 0.0;
            if veiled[i] || unseen && (!posed_text || n.clip.w <= 0.0 || n.clip.h <= 0.0) { continue; }
            let role = self.roles[i];
            // Reads the field map (DESIGN.md §1.29): not a stirring element's own subtree, not what a
            // morph shows (its hand-over must match the first stage exactly).
            let reads = if stirred[i] || role != 0 { 0.0 } else { 1.0 };
            let hidden = match (role, &self.run) { (1, Some(r)) => now >= r.src_until, (2, Some(r)) => now < r.dst_from, _ => n.hidden };
            let c = n.clip;
            let clip = [c.x * s, c.y * s, c.right() * s, c.bottom() * s];
            // A round clip's radius rides in look[3] above its flag (device px in quarters, × 2).
            let clip_q = 2.0 * (n.clip_r * s * 4.0).round();
            if n.calm && !hidden && !unseen && calms < self.field.calm.len() {
                // Its box as seen, a little larger, so a front's push cannot carry an edge across it.
                let r = seen;
                self.field.calm[calms] = [(r.x - 4.0) * s, (r.y - 4.0) * s, (r.right() + 4.0) * s, (r.bottom() + 4.0) * s];
                calms += 1;
            }
            if copied && let Some(k) = &kept {
                let (a, b) = k.runs[i];
                let base = self.scene.instances.len();
                for idx in a..b {
                    let inst = k.insts[idx];
                    self.scene.push(inst);
                    self.owners.push(k.owners[idx]);
                    latest = latest.max(inst.extra[3]);
                    let m = if inst.look[0] >= 127.5 { (inst.look[0].round() as u32 % 16) as u8 } else { global };
                    any_loose |= inst.extra[2] > 0.0 && crate::style::Material::from_id(m).time_varying();
                    // The flows' counters go on from what it drew.
                    if let Some(t) = &k.tags[idx] && let Some((_, root)) = &n.flow { let c = &mut counters.entry(*root).or_default()[t.glyph as usize]; *c = (*c).max(t.n + 1); }
                    self.tags.push(k.tags[idx].clone());
                }
                let (ga, gb) = k.log_runs[i];
                for g in &k.log[ga..gb] { self.glyphs.log.push(crate::hit::DrawnGlyph { inst: g.inst - a + base, ..*g }); }
                if let Some(l) = k.laid.iter().find(|l| l.0 == i) { self.glyphs.laid.push(l.clone()); }
                for u in k.unseen.iter().filter(|u| u.0 == i) { self.unseen.push(*u); }
                continue;
            }
            let under = self.modal_at.is_some_and(|m| i < m);
            // What the field may do to it (DESIGN.md §1.15): fronts add at most `front_cap`; a press held
            // and a pull past a scroll end at most `own_cap`.
            let (_, own_cap, mut resp) = response_caps(n, self.timing.legible_cap);
            // On a beat a front moves in steps, and text to read pushed a step at a time reads as the page
            // shaking: it is not pushed then, only loosened up to `legible_cap` (DESIGN.md §1.32).
            if n.legible && self.timing.beat.on() && !matches!(n.content, Content::Picture { .. }) { resp[1] = 0.0; }
            // Text to read and controls are pushed as one piece (the control they are in, or the text
            // element itself), so a front never pulls a word apart: the push is measured at that
            // piece's centre (device px), carried in the spare numbers.
            if n.legible && resp[1] > 0.0 {
                let (cx, cy) = unit[i].map_or(n.rect, |u| self.nodes[u].rect).center();
                resp = [resp[0], 2.0, cx * s, cy * s];
            }
            // The lens (DESIGN.md §1.15): a picture or a 3D element that fronts may loosen and that the
            // pointer is not there to operate (no handler on it or above it) shows its marks under it.
            if resp[0] > 0.0 && resp[1] < 1.5 && matches!(n.content, Content::Picture { .. } | Content::Scene3d(_)) && !operated(&self.nodes, i) { resp[3] = 1.0; }
            // A subtree held loose (`El::loosen`, as an effect on existing text puts it) holds its text to
            // read and controls at most `legible_cap`; a heading is a title and takes the whole number.
            let heading = n.access.as_ref().is_some_and(|a| a.role == Some(crate::el::Role::Heading));
            let held_at = if n.legible && !heading { n.loosen.min(self.timing.legible_cap) } else { n.loosen };
            let loose = if held[i] { own_cap } else if under { n.loose.max(held_at).max(beneath) } else { n.loose.max(held_at) };
            let loose = loose.max(pulled(&self.nodes, i).min(own_cap));
            let mat = n.mat;
            let dim = if n.disabled { 0.4 } else { 1.0 } * fade;
            let mut sink = Sink { scene: &mut self.scene, owners: &mut self.owners, tags: &mut self.tags, unseen: &mut self.unseen, counters: &mut counters, latest: &mut latest, loose: &mut any_loose };
            let nk = { use std::hash::{Hash, Hasher}; let mut h = crate::fasthash::FastHasher::new(); n.key.hash(&mut h); h.finish() };
            let text_born = self.text_births.get(&nk).copied();
            // A drawing's marks, and ink on a plate (`El::plate`: what is drawn is there at once).
            let drawn = matches!(n.content, Content::Canvas(_)) || plate.is_some();
            // Under scribblr, text to read and controls hold where they are: the map's flow never carries
            // them (DESIGN.md §1.30).
            let in_place = n.legible && self.timing.material == crate::style::Material::Scribblr.id();
            let turned = n.xf;
            let mut put = |sink: &mut Sink, mut inst: Instance, id: u64, glyph: Option<u32>| -> bool {
                inst.clip = clip;
                // Turned after layout (`El::turn`): drawn about the turned ancestors' centres.
                // (Not a blade: its numbers are where a transform would go.)
                if let Some(a) = &turned && !inst.blade() { a.instance(&mut inst, s); }
                // A negative push marks what fronts pass by whatever its element says (the caret, a
                // selection's halo).
                // A blade keeps its own spare numbers (`resp` z, w: its colour's lean range).
                inst.resp = if inst.resp[1] < 0.0 { [0.0; 4] } else if inst.blade() { [resp[0], resp[1], inst.resp[2], inst.resp[3]] } else { resp };
                inst.look[2] = reads;
                // A picture or a 3D view loosens where the map is, in place: its flow never carries it
                // off its own box (so a trail never shows the page under a picture, §1.29).
                inst.look[3] = if in_place || inst.paint[3] == PICTURE || inst.paint[3] == MESH { 1.0 } else { 0.0 } + clip_q;
                inst.paint[2] *= dim;
                inst.extra[2] = loose;
                // Hidden instances are kept for sampling only: they get no birth time, so a view that
                // shows them later lets them condense as usual.
                if hidden { inst.extra[3] = -1e4; sink.unseen.push((i, inst)); return false; }
                // What a morph shows has been there all along: it must not condense on top of its ramp.
                // A drawing's marks change every frame: they are there at once, never condensing.
                let born = if role != 0 || drawn || self.timing.off.close { *births.entry(id).or_insert(-1e4) }
                           else { *births.entry(id).or_insert_with(|| self.births.get(&id).copied().or(if glyph.is_some() { text_born } else { None }).unwrap_or(if self.scroll_paint { -1e4 } else { now })) };
                if glyph.is_some() { let e = text_births.entry(nk).or_insert(born); *e = e.min(born); }
                inst.extra[3] = born;
                *sink.latest = sink.latest.max(born);
                // Loose under a material that moves loose things by itself (its own override or the global one).
                let m = if inst.look[0] >= 127.5 { (inst.look[0].round() as u32 % 16) as u8 } else { global };
                *sink.loose |= loose > 0.0 && crate::style::Material::from_id(m).time_varying();
                let tag = n.flow.as_ref().map(|(f, root)| {
                    let c = &mut sink.counters.entry(*root).or_default()[glyph.is_some() as usize];
                    let t = Tag { flow: f.clone(), glyph: glyph.is_some(), n: *c, gid: glyph.unwrap_or(0) };
                    *c += 1;
                    t
                });
                sink.scene.push(inst);
                sink.owners.push((i, id));
                sink.tags.push(tag);
                true
            };
            let shape = |p: Paint, bw: f32| Instance { rect: dev(n.rect), uv: [n.radius * s, 0.0, 0.0, 0.0], paint: [p.tone, p.accent, p.alpha, SHAPE], clip, extra: [bw * s, 0.0, 0.0, 0.0], look: [mat, p.theme_hue(), (n.rect.y * s).round(), (n.rect.h * s).round().max(1.0)], resp, xf: [0.0; 4] };
            // A skeleton (DESIGN.md §1.23): faint fills and borders, a bar for every line of text, a block
            // for every picture; its own identities, so the content that replaces it condenses in.
            if n.skeleton {
                let faint = |p: Paint| Paint { tone: Paint::FAINT.tone, accent: 0.0, alpha: p.alpha.min(1.0), hue: 0 };
                if let Some(p) = n.fill { put(&mut sink, shape(faint(p), 0.0), ident(&n.key, u32::MAX - 20, 0), None); }
                if let Some((w, p)) = n.border { put(&mut sink, shape(faint(p), w), ident(&n.key, u32::MAX - 21, 0), None); }
                let block = |r: Rect, radius: f32| Instance { rect: dev(r), uv: [radius * s, 0.0, 0.0, 0.0], paint: [Paint::FAINT.tone, 0.0, 1.0, SHAPE], clip, look: [mat, 0.0, (r.y * s).round(), (r.h * s).round().max(1.0)], resp, ..Default::default() };
                match &n.content {
                    Content::Text { .. } => if let Some(p) = n.para() {
                        for (k, r) in self.text.line_boxes(p, n.origin()).into_iter().enumerate() {
                            let h = (r.h * SKELETON_BAR).round().max(2.0);
                            let bar = Rect::new(r.x, r.y + ((r.h - h) * 0.5).round(), r.w, h);
                            put(&mut sink, block(bar, h * 0.5), ident(&n.key, u32::MAX - 32 - k as u32, 0), None);
                        }
                    },
                    Content::Picture { .. } | Content::Scene3d(_) | Content::Canvas(_) | Content::Cells(_) | Content::CachedView { .. } => { put(&mut sink, block(n.inner, n.radius.max(4.0)), ident(&n.key, u32::MAX - 22, 0), None); }
                    Content::None => {}
                }
                continue;
            }
            if let Some(p) = n.fill_on.at(n.fill, hand[i]) { put(&mut sink, shape(p, 0.0), ident(&n.key, u32::MAX, 0), None); }
            if let Some((w, p)) = n.fill_on.border_at(n.border, lit[i]) { put(&mut sink, shape(p, w), ident(&n.key, u32::MAX - 1, 0), None); }
            match &n.content {
                Content::Text { line, paint, caret, weight, select, mark, pose, .. } => {
                    let proto = Instance { paint: [paint.tone, paint.accent, paint.alpha, 0.0], look: [mat, paint.theme_hue(), (n.inner.y * s).round(), (line * s).round().max(1.0)], ..Default::default() };
                    let Some(para) = n.para() else { continue };
                    let wpx = weight * s;
                    let focused = caret_at.is_some_and(|f| within(&self.nodes, i, f));
                    // Lines the clip hides are not made into instances at all (posed glyphs go elsewhere: all
                    // of them are posed, and those the clip hides then left out).
                    let rows = if pose.is_some() { None } else { Some((clip[1], clip[3])) };
                    let TextScratch { glyphs, swept, pens, posed, out, chosen } = &mut tx;
                    glyphs.clear();
                    swept.clear();
                    pens.clear();
                    // A face arriving (DESIGN.md §1.6.5) sweeps along each line of text on screen.
                    let window = Rect::new(0.0, 0.0, self.size.0, self.size.1);
                    let arriving = self.arriving.get(&para.face).copied().filter(|_| { let r = n.rect.intersect(&n.clip).intersect(&window); r.w > 0.0 && r.h > 0.0 });
                    // `pens`: each glyph's pen as laid out (device px: x, baseline, advance), for a pose.
                    match arriving {
                        None => self.text.emit_pens(para, n.origin(), proto, s, rows, |mut inst, glyph, p| { inst.extra[0] = wpx; glyphs.push((inst, glyph)); pens.push([p[0], p[1], p[4]]); }),
                        Some((t0, d)) => {
                            self.sweeping = true;
                            self.swept_nodes.push(i);
                            let heading = n.access.as_ref().is_some_and(|a| a.role == Some(crate::el::Role::Heading));
                            let cap = if self.timing.material == crate::style::Material::Plain.id() || self.fmap.reduced { 0.0 } else if heading { 0.6 } else { self.timing.legible_cap };
                            for (mut inst, glyph, l) in sweep_glyphs(&mut self.text, para, n.origin(), proto, s, rows, (now - t0, d), self.fmap.reduced, cap) {
                                inst.extra[0] = wpx;
                                glyphs.push((inst, glyph));
                                swept.push(l);
                            }
                        }
                    }
                    // Each glyph's number in the text, depth, and where layout put it (its centre in the
                    // content box), kept through poses, sorting and thinning for identities and hits.
                    let (ox, oy) = (n.inner.x, n.inner.y);
                    if n.on_pens.is_some() && arriving.is_none() && pens.len() == glyphs.len() { self.pens_found.push((i, pens.iter().map(|p| p[0] / s - ox).collect())); }
                    let centre = |g: &Instance| ((g.rect[0] + g.rect[2] * 0.5) / s - ox, (g.rect[1] + g.rect[3] * 0.5) / s - oy);
                    let laid: Vec<(f32, f32)> = glyphs.iter().map(|(g, _)| centre(g)).collect();
                    posed.clear();
                    posed.extend(glyphs.iter().enumerate().map(|(k, (g, gid))| crate::form::Posed { inst: *g, glyph: *gid, seq: k, z: 0.0, facing: 1.0, at: laid[k], group: None }));
                    // Glyphs drawn by their poses (on a path, a sphere, under a light, on a form): each keeps
                    // its outline and is moved, turned and scaled as a whole (DESIGN.md §1.26).
                    if let Some(f) = pose {
                        let (bw, bh) = (n.inner.w, n.inner.h);
                        let span = laid.iter().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |b, &(x, y)| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]);
                        let count = posed.len();
                        for g in posed.iter_mut() {
                            let (x, y) = g.at;
                            // While a face arrives (its glyphs swept from the stand-in's) the pen is the centre.
                            let [px, py, adv] = pens.get(g.seq).copied().filter(|_| pens.len() == count).map_or([x, y, 0.0], |p| [p[0] / s - ox, p[1] / s - oy, p[2] / s]);
                            let p = f(crate::el::GlyphAt { i: g.seq, n: count, x, y, w: bw, h: bh, span, pen: px, base: py, advance: adv });
                            let gi = &mut g.inst;
                            gi.rect[0] += p.dx * s;
                            gi.rect[1] += p.dy * s;
                            gi.xf = Instance::turned(p.angle, p.sx, p.sy);
                            gi.paint[0] *= p.tone;
                            gi.paint[1] = gi.paint[1].max(p.accent);
                            gi.paint[2] *= p.alpha;
                            if p.hue >= 0.0 { gi.look[1] = p.hue.round().min(2.0); }
                            (g.z, g.facing) = (p.z, p.facing);
                        }
                    }
                    if let (Some(fx), true) = (&n.form, pose.is_some()) { crate::form::face(posed, fx); }
                    // Whole words or lines (`El::reveal_by`): a pose fades a word as one, and a word the
                    // clip or the window cuts is not drawn at all.
                    if n.reveal_by != crate::el::Reveal::Glyph {
                        // The pieces from the content box's corner, kept while the text is the same.
                        let groups = self.text.reveal_pieces(para, n.reveal_by);
                        let window = [0.0, 0.0, self.size.0 * s, self.size.1 * s];
                        crate::form::reveal_whole(posed, &groups, (0.0, 0.0), [clip[0].max(window[0]), clip[1].max(window[1]), clip[2].min(window[2]), clip[3].min(window[3])]);
                    }
                    if pose.is_some() || n.reveal_by != crate::el::Reveal::Glyph { crate::form::arrange(posed, n.form.as_ref().filter(|_| pose.is_some()), &self.text.pixels); }
                    if pose.is_some() { posed.retain(|g| { let b = g.inst.bounds(0.0); b[0] < clip[2] && b[2] > clip[0] && b[1] < clip[3] && b[3] > clip[1] }); }
                    out.clear();
                    out.extend(posed.iter().map(|g| (g.inst, g.glyph)));
                    let glyphs = &*out;
                    // A selection (the field's, while it has focus, or the read-only one) is drawn by
                    // nearness: each selected glyph again, under itself, with its outline moved out, so
                    // neighbouring glyphs' halos run together and follow the letters, not a box.
                    // The halo is opaque, the accent mixed into the ground (`Look::select`), so halos that
                    // overlap make no darker band; it is there at once (born long ago) and goes at once.
                    let range = select.filter(|_| focused).or_else(|| ro.get(&i).copied());
                    chosen.clear();
                    chosen.resize(glyphs.len(), false);
                    if let Some((a, b)) = range {
                        let boxes = self.text.boxes(para, n.origin(), a, b);
                        for (k, (g, gid)) in glyphs.iter().enumerate() {
                            let (cx, cy) = ((g.rect[0] + g.rect[2] * 0.5) / s, (g.rect[1] + g.rect[3] * 0.5) / s);
                            if !boxes.iter().any(|r| r.inset(-1.0, -1.0, -1.0, -1.0).contains(cx, cy)) { continue; }
                            chosen[k] = true;
                            let mut halo = *g;
                            halo.extra[0] = wpx + sel.halo * s;
                            halo.paint = [0.0, sel.alpha, 1.0, halo.paint[3]];
                            halo.look[1] = 0.0;
                            halo.resp = IGNORE;
                            if put(&mut sink, halo, ident(&n.key, (k as u32) | 0x4000_0000, *gid), None) && let Some(last) = sink.scene.instances.last_mut() { last.extra[3] = -1e4; }
                        }
                    }
                    // Text that recomposes remembers its glyphs, to see what changes in the next view.
                    if n.recompose && role == 0 && !hidden && !under && (!self.no_recompose || n.recompose_always) {
                        let ids = posed.iter().map(|g| (ident(&n.key, g.seq as u32, g.glyph), g.inst)).collect();
                        seen_recs.insert(n.key.clone());
                        jobs.push(RecJob { node: i, rec: TextRec { s: para.s.to_owned(), face: para.face, size: para.size, line: para.line, width: para.width, print: para.print, track: para.track, origin: n.origin(), glyphs: ids, changed: f32::NEG_INFINITY } });
                    }
                    let mut drawn = vec![false; laid.len()];
                    for (k, g) in posed.iter().enumerate() {
                        let at = sink.scene.instances.len();
                        // Selected, in the ink (`Select::ink`).
                        let mut inst = g.inst;
                        if sel.ink && chosen[k] { inst.paint = [1.0, 0.0, 1.0, inst.paint[3]]; inst.look[1] = 0.0; }
                        if put(&mut sink, inst, ident(&n.key, g.seq as u32, g.glyph), Some(g.glyph)) {
                            // Loosened by the material as its face changes.
                            if let Some(&l) = swept.get(g.seq) && let Some(last) = sink.scene.instances.last_mut() { last.extra[2] = last.extra[2].max(l); }
                            drawn[g.seq] = true;
                            self.glyphs.log.push(crate::hit::DrawnGlyph { inst: at, node: i, i: g.seq, z: g.z, at: g.at });
                        }
                    }
                    self.glyphs.laid.push((i, laid, drawn));
                    // Text being composed is underlined.
                    if let Some((a, b)) = mark {
                        for (k, r) in self.text.boxes(para, n.origin(), *a, *b).into_iter().enumerate() {
                            let bar = Instance { rect: dev(Rect::new(r.x, r.bottom() - 2.5, r.w, 1.5)), paint: [paint.tone, 0.0, 1.0, SHAPE], ..Default::default() };
                            put(&mut sink, bar, ident(&n.key, u32::MAX - 8 - k as u32, 0), None);
                        }
                    }
                    // The caret, only while the field it belongs to has focus. It keeps one identity, so
                    // moving it does not make it condense again.
                    if let Some(at) = caret.filter(|_| focused) && let Some(r) = self.text.caret(para, n.origin(), at) {
                        let bar = Instance { rect: dev(Rect::new(r.x, r.y, 1.5, r.h)), paint: [paint.tone, 1.0, 1.0, SHAPE], resp: IGNORE, ..Default::default() };
                        let before = sink.scene.instances.len();
                        put(&mut sink, bar, ident(&n.key, u32::MAX - 4, 0), None);
                        if sink.scene.instances.len() > before { caret_found = Some((before, bar.rect)); }
                    }
                }
                Content::Picture { id, fx, .. } => {
                    let inst = Instance { rect: dev(n.inner), uv: [0.0, 0.0, 1.0, 1.0], paint: [1.0, *fx as f32, 1.0, PICTURE], clip, extra: [0.0, *id as f32, 0.0, 0.0], look: [mat, 0.0, 0.0, 1.0], resp, xf: [0.0; 4] };
                    put(&mut sink, inst, ident(&n.key, u32::MAX - 2, *id), None);
                    // The wipe is a half-plane source in the field, acting over the visible picture.
                    if let Some(x) = n.wipe && !hidden {
                        let r = n.inner.intersect(&c);
                        self.field.wipe = [(n.inner.x + x * n.inner.w) * s, 1.0, 0.0, 0.0];
                        self.field.wipe_rect = [r.x * s, r.y * s, r.right() * s, r.bottom() * s];
                    }
                }
                Content::Scene3d(sc) => {
                    // The renderer draws the meshes off-screen and resolves them into this one instance.
                    let k = sink.scene.meshes.len();
                    let r = dev(n.inner);
                    sink.scene.meshes.push(crate::scene::MeshDraw { scene: sc.clone(), rect: r });
                    let inst = Instance { rect: r, uv: [0.0, 0.0, 1.0, 1.0], paint: [sc.spec.outline.max(0.0) * s, sc.picture.unwrap_or(0) as f32, 1.0, MESH], clip, extra: [sc.picture.map_or(0.0, |_| 1.0), k as f32, 0.0, 0.0], look: [mat, 0.0, r[1], r[3].max(1.0)], resp, xf: [0.0; 4] };
                    put(&mut sink, inst, ident(&n.key, u32::MAX - 5, 0), None);
                }
                Content::Cells(c) => {
                    // One instance over the whole box: the picture it reads is bound as for the picture
                    // itself, and the cells, the ramp and the ink are packed into its spare numbers
                    // (scene.rs `Instance`, "cells").
                    let Some(pic) = self.nodes.iter().find(|p| p.key == c.over && matches!(p.content, Content::Picture { .. })) else { continue };
                    let Content::Picture { id, fx, .. } = pic.content else { continue };
                    let pr = pic.inner;
                    let (cw, ch) = ((c.pitch[0] * s).round().clamp(1.0, 255.0), (c.pitch[1] * s).round().clamp(1.0, 255.0));
                    let face = c.font.resolve([0; 3]);
                    let ramp: Vec<char> = c.ramp.chars().take(63).collect();
                    let Some((ax, ay)) = self.text.ramp(face, &ramp, c.size * s, cw as u32, ch as u32) else { continue };
                    let r = dev(n.inner);
                    // The cells are whole device px from the box's corner: the box starts on a pixel.
                    let r = [r[0].round(), r[1].round(), r[2], r[3]];
                    let uv = [(r[0] / s - pr.x) / pr.w.max(1e-3), (r[1] / s - pr.y) / pr.h.max(1e-3), (r[0] / s + r[2] / s - pr.x) / pr.w.max(1e-3), (r[1] / s + r[3] / s - pr.y) / pr.h.max(1e-3)];
                    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round();
                    let packed_ink = q(c.mix) + 256.0 * q(c.ink.tone) + 65536.0 * q(c.ink.accent);
                    let hue = if c.ink.hue == Paint::EXTREME_HUE { 3.0 } else { c.ink.hue.min(2) as f32 };
                    let inst = Instance { rect: r, uv, paint: [packed_ink, fx as f32, c.ink.alpha, PICTURE], clip,
                        extra: [ramp.len() as f32 + 64.0 * (cw + 256.0 * ch) + if c.light_dense { 1.0 / 4.0 } else { 0.0 }, id as f32, 0.0, 0.0],
                        look: [mat, hue + 4.0 * (ax as f32 + 2048.0 * ay as f32), 0.0, 1.0], resp, xf: [0.0; 4] };
                    put(&mut sink, inst, ident(&n.key, u32::MAX - 2, id), None);
                }
                Content::CachedView { over, from, ink } => {
                    // One glyph-like instance whose coverage is read, nearest, from the cached plate's ink
                    // (window-sized, device px); its coordinates there are packed below −1 with the slot
                    // (scene.rs `Instance::cached_slot`).
                    let Some(slot) = self.plate_slots.iter().position(|p| p.as_ref().is_some_and(|p| &p.0 == over)) else { continue };
                    // Not inside what it reads (a pass cannot read the image it draws into).
                    if self.plate_sig[slot].is_none() || plate.is_some_and(|(_, _, k)| k as usize == slot) { continue; }
                    let (ww, wh) = (self.size.0.max(1e-3), self.size.1.max(1e-3));
                    let e = |v: f32| -1.0 - 4.0 * slot as f32 - v.clamp(0.0, 1.0);
                    let uv = [e(from[0] / ww), e(from[1] / wh), e((from[0] + from[2]) / ww), e((from[1] + from[3]) / wh)];
                    let inst = Instance { rect: dev(n.inner), uv, paint: [ink.tone, ink.accent, ink.alpha, GLYPH], clip,
                        extra: [0.0; 4], look: [mat, ink.hue.min(2) as f32, 0.0, 0.0], resp, xf: [0.0; 4] };
                    put(&mut sink, inst, ident(&n.key, u32::MAX - 4, slot as u32), None);
                }
                Content::Canvas(d) => {
                    // Each mark a shape with its true outline, in the drawing's space scaled to the box.
                    let k = n.inner.w / d.w.max(1e-3) * s;
                    let (ox, oy) = (n.inner.x * s, n.inner.y * s);
                    // A drawing shows at most the budget's count of marks, evenly thinned (DESIGN.md §1.27).
                    let stride = d.marks.len().div_ceil(crate::budget::cap().max(1)).max(1);
                    // A hue of its own colours (`Drawing::palette`) is its place in the frame's table, after
                    // the theme's three; past the palette or the table it is hue 0.
                    let mut hue = |p: Paint| own_hue(p, &d.palette, &mut own_cols);
                    for (j, m) in d.marks.iter().enumerate().step_by(stride) {
                        if let crate::draw::Mark::Blade(b) = m {
                            // Its segments (6, or 2 when it is short) and its head, each worked out on the GPU
                            // from the map at its root (scene.rs `Instance::xf`).
                            let (rx, ry, len, w) = (ox + b.root.0 * k, oy + b.root.1 * k, b.len * k, (b.width * k / n.pen).max(0.5));
                            let segs = crate::draw::Blade::segments(b.len * k / s) as u32;
                            let bounds = [rx - len - w, ry - len - w, 2.0 * (len + w), len + 2.0 * w];
                            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round();
                            let lp = b.lean_paint;
                            // Where its colour starts towards the lean paint and where it is all of it (radians;
                            // in the spare numbers of `resp`, which a blade is never pushed by).
                            let mix = if b.lean_full > b.lean_from { (b.lean_from.abs(), b.lean_full.abs()) } else { (0.0, 0.0) };
                            let wq = (w * 16.0).round().min(4095.0);
                            for g in 0..segs {
                                let inst = Instance { rect: bounds, uv: [b.stiff.clamp(0.0, 1.0), b.max_lean.abs().min(1.5), b.lean, g as f32 + 16.0 * segs as f32 + 256.0 * wq], paint: [b.paint.tone, b.paint.accent, b.paint.alpha, SHAPE], clip,
                                    extra: [0.0, 1.0 + q(lp.tone) + 256.0 * q(lp.accent), 0.0, 0.0], look: [mat, hue(b.paint), 0.0, 0.0], resp: [resp[0], resp[1], mix.0, mix.1], xf: [0.0; 4] };
                                put(&mut sink, inst, ident(&n.key, (j as u32 * 16 + g) | 0x2000_0000, 1), None);
                            }
                            if let Some(h) = b.head {
                                let face = h.font.resolve([0; 3]);
                                let mut chb = [0u8; 4];
                                let para = crate::text::Para { s: h.ch.encode_utf8(&mut chb), face, size: h.size * k / s, line: h.size * k / s * 1.2, width: 1e4, print: 0.0, around: None, track: 0.0 };
                                let proto = Instance { paint: [h.paint.tone, h.paint.accent, h.paint.alpha, 0.0], look: [mat, hue(h.paint), 0.0, 0.0], ..Default::default() };
                                let mut quads = vec![];
                                self.text.emit(para, (0.0, 0.0), proto, s, |inst, _| quads.push(inst));
                                let sp = crate::text::SPREAD as f32;
                                let (sq, mq) = ((b.stiff.clamp(0.0, 1.0) * 100.0).round(), (b.max_lean.abs().min(1.5).to_degrees() * 10.0).round());
                                let lq = ((b.lean.to_degrees().clamp(-90.0, 90.0) + 90.0) * 10.0).round();
                                for mut g in quads {
                                    // At rest the bottom centre of its ink is on the upright tip.
                                    let (ax, ay) = (g.rect[0] + g.rect[2] * 0.5, g.rect[1] + g.rect[3] - sp);
                                    g.rect[0] += rx - ax; g.rect[1] += ry - len - ay;
                                    g.xf = [rx, ry, len, sq + 128.0 * mq];
                                    g.extra[1] = -(1.0 + lq);
                                    g.extra[0] = 1.0 + q(lp.tone) + 256.0 * q(lp.accent);
                                    g.resp = [0.0, 0.0, mix.0, mix.1];
                                    g.clip = clip;
                                    put(&mut sink, g, ident(&n.key, (j as u32 * 16 + 15) | 0x2000_0000, 2), None);
                                }
                            }
                            continue;
                        }
                        let (rect, radius, xf, p, ramp) = match *m {
                            // Under a view scale (`El::view_scale`) a dot under 1.5 device px across is one whole
                            // device pixel, the one its centre is in: never gone, never fainter.
                            crate::draw::Mark::Dot { x, y, r, paint } if n.pen != 1.0 && r * k < 0.75 => {
                                let (cx, cy) = ((ox + x * k).floor(), (oy + y * k).floor());
                                ([cx, cy, 1.0, 1.0], 0.0, [0.0; 4], paint, None)
                            }
                            crate::draw::Mark::Dot { x, y, r, paint } => {
                                let rr = (r * k).max(0.5);
                                ([ox + x * k - rr, oy + y * k - rr, rr * 2.0, rr * 2.0], rr, [0.0; 4], paint, None)
                            }
                            crate::draw::Mark::Line { a, b, w, paint, far, mid } => {
                                let (dx, dy) = ((b.0 - a.0) * k, (b.1 - a.1) * k);
                                // Its width as at 1 under a view scale: the pen does not scale.
                                let (len, ww) = ((dx * dx + dy * dy).sqrt(), (w * k / n.pen).max(0.5));
                                let (cx, cy) = (ox + (a.0 + b.0) * 0.5 * k, oy + (a.1 + b.1) * 0.5 * k);
                                ([cx - (len + ww) * 0.5, cy - ww * 0.5, len + ww, ww], ww * 0.5, Instance::turned(dy.atan2(dx), 1.0, 1.0), paint, far.map(|f| (f, mid)))
                            }
                            crate::draw::Mark::Blade(_) => unreachable!("drawn above"),
                        };
                        // A ramp (`Drawing::ramp`): its far and middle paints in uv.zw, their hues (6 bits each, own
                        // colours included) and where the middle is in extra.x, all negative (a stroke has no
                        // border; a blade's joints are positive). Its opacity is its strongest stop's, and each
                        // stop's (the start's in extra.y, −1 − it) is a share of that, so whatever later scales
                        // the instance's opacity (a fade, a flow, a group) scales every stop.
                        let (mut uv, mut ex, mut ey, mut alpha, h0) = ([radius, 0.0, 0.0, 0.0], 0.0, 0.0, p.alpha, hue(p));
                        if let Some((far, mid)) = ramp {
                            let (m, at) = mid.unwrap_or((far, 0.5));
                            alpha = p.alpha.max(far.alpha).max(m.alpha);
                            let share = |q: Paint| Paint { alpha: if alpha > 0.0 { q.alpha / alpha } else { 0.0 }, ..q };
                            uv[2] = -(1.0 + ramp_pack(share(far)));
                            uv[3] = if mid.is_some() { -(1.0 + ramp_pack(share(m))) } else { 0.0 };
                            ex = -(1.0 + hue(far) + 64.0 * hue(m) + 4096.0 * (at.clamp(0.0, 1.0) * 255.0).round());
                            ey = -(1.0 + share(p).alpha);
                        }
                        let inst = Instance { rect, uv, paint: [p.tone, p.accent, alpha, SHAPE], clip, look: [mat, h0, rect[1], rect[3].max(1.0)], resp, xf, extra: [ex, ey, 0.0, 0.0] };
                        put(&mut sink, inst, ident(&n.key, j as u32 | 0x2000_0000, 0), None);
                    }
                }
                Content::None => {}
            }
            // Where a scroll node is: a thin bar along its far edge, when it holds more than it shows.
            if n.scroll && n.bar {
                let off = self.scroll.get(&n.key).copied().unwrap_or(0.0);
                let len = if n.horizontal { n.rect.w } else { n.rect.h };
                if n.extent > len + 0.5 {
                    let thumb = (len * len / n.extent).max(16.0);
                    let at = off / (n.extent - len) * (len - thumb);
                    let r = if n.horizontal { Rect::new(n.rect.x + at, n.rect.bottom() - 4.0, thumb, 2.0) } else { Rect::new(n.rect.right() - 4.0, n.rect.y + at, 2.0, thumb) };
                    let inst = Instance { rect: dev(r), uv: [s, 0.0, 0.0, 0.0], paint: [0.5, 0.0, 0.8, SHAPE], ..Default::default() };
                    put(&mut sink, inst, ident(&n.key, u32::MAX - 3, 0), None);
                }
            }
        }
        if let Some((_, start, op)) = group && (self.scene.instances.len() as u32) > start { self.scene.groups.push(crate::scene::Group { start, end: self.scene.instances.len() as u32, opacity: op }); }
        if let Some((root, start, slot)) = plate {
            if let Some(o) = keep_open.take() { self.keep_close(o, &births, &text_births, &jobs, [calms, own_cols.len()]); }
            self.plate_span(root, slot, start);
        }
        if let Some((root, start, t0)) = stage_open { self.stage_close(root, start, t0, &text_births, &seen_recs); }
        // A drawn stage keeps its see-through groups with its image; a stage no longer in the view is gone.
        for (_, st) in &mut self.stages {
            let (a, b) = st.span;
            if st.behind || b <= a { continue; }
            st.groups = self.scene.groups.iter().filter(|g| (g.start as usize) >= a && (g.end as usize) <= b).map(|g| crate::scene::Group { start: g.start - a as u32, end: g.end - a as u32, opacity: g.opacity }).collect();
            st.span = (0, 0);
        }
        self.stages.retain(|(k, _)| stage_seen.contains(k));
        if !self.plate_clear.is_empty() {
            // Serials are unique across every `Ui` so a renderer shared by two never skips one.
            static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            self.scene.plate_clear = std::mem::take(&mut self.plate_clear);
            self.scene.plate_serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        static PAINTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.scene.paint = PAINTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.scene.plate_now = self.pclock;
        self.scene.plate_ageing = self.plates_ageing();
        self.scene.own = own_cols;
        self.text_scratch = tx;
        // Text whose string changed: the changed glyphs recompose, the others keep their birth and slide.
        if !jobs.is_empty() || !self.recs.is_empty() {
            self.recompose_jobs(jobs, &mut births);
            self.recs.retain(|k, _| seen_recs.contains(k));
            latest = self.scene.instances.iter().map(|i| i.extra[3]).fold(f32::NEG_INFINITY, f32::max);
        }
        self.births = births;
        self.text_births = text_births;
        self.latest_birth = latest;
        self.static_loose = any_loose;
        // The caret blinks from where it last moved to: a new place starts it again, shown.
        self.caret = caret_found.map(|(i, r)| match self.caret {
            Some((_, reset, old)) if old == r => (i, reset, r),
            _ => (i, now, r),
        });
        if let Some((_, reset, _)) = self.caret { self.caret_on = ((now - reset).max(0.0) / BLINK).floor() as i64 % 2 == 0; }
        self.scene.holds = self.holds(true);
        self.scene.islands = self.islands(true);
        let bridges = self.bridges(&self.scene.holds);
        self.scene.islands.extend(bridges);
        self.dirty = true;
        self.page_len = self.scene.instances.len();
        self.base = self.scene.instances.clone();
        self.scene.mats = self.scene.instances.iter().chain(self.unseen.iter().map(|u| &u.1)).filter(|i| i.look[0] >= 127.5).fold(0, |m, i| m | 1u16 << (i.look[0].round() as u32 % 16));
        self.painted_phase = self.run.as_ref().map_or(0, |r| r.phase(now));
        // Ramps and flows apply from the first frame.
        self.animating = true;
        self.outline_pass();
        self.pens_pass();
        self.decode_pass();
        self.finish();
    }
    /// A stage drawn in this paint, from `start` (its first instance) to here: its image, kept for
    /// the paints it holds through, and how long it took.
    fn stage_close(&mut self, root: usize, start: usize, t0: f64, text_births: &HashMap<u64, f32>, recs: &HashSet<String>) {
        let ms = (crate::perf::now_ms() - t0) as f32;
        let frame_ms = if self.tick_gap > 0.0 { self.tick_gap * 1000.0 } else { 1000.0 / 60.0 };
        let end = self.scene.instances.len();
        let size = (root + 1..self.nodes.len()).take_while(|&j| within(&self.nodes, j, root)).count() + 1;
        let keys: Vec<(u64, &str)> = (root..root + size).map(|j| {
            use std::hash::{Hash, Hasher};
            let mut h = crate::fasthash::FastHasher::new();
            self.nodes[j].key.hash(&mut h);
            (h.finish(), self.nodes[j].key.as_str())
        }).collect();
        let now = self.clock;
        let n = &self.nodes[root];
        let (rect, clip) = (n.rect, n.clip);
        let Some((_, st)) = self.stages.iter_mut().find(|(k, _)| *k == n.key) else { return };
        st.snap = self.scene.instances[start..end].to_vec();
        st.owners = self.owners[start..end].iter().map(|&(j, id)| (j - root, id)).collect();
        st.tags = self.tags[start..end].to_vec();
        st.texts = keys.iter().filter_map(|(h, _)| text_births.get(h).map(|b| (*h, *b))).collect();
        st.recs = keys.iter().filter(|(_, k)| recs.contains(*k)).map(|(_, k)| (*k).to_owned()).collect();
        (st.rect, st.clip, st.nodes, st.span) = (rect, clip, size, (start, end));
        // A stage is held only when its scale is as low as it goes (or pinned, or nothing measures it).
        let may_hold = !self.scaler.fed || self.stage_pin.is_some() || self.scaler.lowest();
        st.drew(ms, now, frame_ms, may_hold);
    }
    /// The stages `keys` stepped (`Ui::on_step`) and nothing else changed: each is viewed alone
    /// (`Ui::stage_view`) and, when it lays out as before, put in place of its subtree, keeping the
    /// boxes, and only the stages are painted again. Anything else (a stage that lays out otherwise, one
    /// not on the page, a morph or a flow under way) views and lays out the whole page.
    fn restage(&mut self, keys: Vec<String>) {
        if self.defer > 0 {
            for k in keys { if !self.stale_stages.contains(&k) { self.stale_stages.push(k); } }
            self.dirty = true;
            return;
        }
        if self.stale_layout { self.relayout(); return; }
        let Some(view) = self.stage_of else { self.relayout(); return };
        // A slow stage that would hold now is not viewed: its step waits for the frame it draws in.
        let mut keys = keys;
        let mut held = std::mem::take(&mut self.held_steps);
        keys.retain(|k| {
            let holds = self.stage_holds(k);
            if holds && !held.contains(k) { held.push(k.clone()); }
            if !holds { held.retain(|h| h != k); }
            !holds
        });
        self.held_steps = held;
        if keys.is_empty() { return; }
        let sp = self.span_begin();
        let mut roots = vec![];
        let mut ok = self.run.is_none() && self.flows.is_empty() && self.ghosts.is_empty() && self.sent.is_empty();
        for k in &keys {
            if !ok { break; }
            let Some(i) = self.nodes.iter().position(|n| n.stage.is_some() && n.key == *k) else { ok = false; break };
            if roots.iter().any(|&r| within(&self.nodes, i, r)) { continue; }
            // A stage with parts that change alone (`El::steps` inside it): those parts, when each is
            // viewed and put in place; otherwise the stage whole. A stage that held (its image behind
            // what it shows) draws whole: drawing its parts alone would leave it behind for good.
            let behind = self.stages.iter().any(|(sk, st)| sk == k && st.behind);
            let parts = if behind { vec![] } else { self.parts(i) };
            let alone = !parts.is_empty() && parts.iter().all(|&j| { let pk = self.nodes[j].key.clone(); view(&self.state, &pk).is_some_and(|el| self.swap_stage(j, el)) });
            if alone {
                if let Some((_, st)) = self.stages.iter_mut().find(|(sk, _)| sk == k) { st.stale = true; }
                roots.extend(parts);
                continue;
            }
            ok = view(&self.state, k).is_some_and(|el| self.swap_stage(i, el));
            roots.retain(|&r| !within(&self.nodes, r, i));
            roots.push(i);
        }
        if !ok { self.span_end(sp); self.relayout(); return; }
        self.place();
        // A paint already owed draws the whole page; otherwise only the stages.
        if !self.stale_paint { self.restaging = Some(roots); }
        self.paint();
        self.restaging = None;
        self.span_end(sp);
        self.flush();
    }
    /// The parts of the stage at `i` that change alone when it steps (`El::steps` inside it), outermost
    /// first; none for an element that is not a held stage.
    fn parts(&self, i: usize) -> Vec<usize> {
        if !self.nodes[i].stage.as_ref().is_some_and(|c| c.held) { return vec![]; }
        let mut out: Vec<usize> = vec![];
        for j in (i + 1..self.nodes.len()).take_while(|&j| within(&self.nodes, j, i)) {
            if self.nodes[j].stage.as_ref().is_some_and(|c| c.part) && !out.iter().any(|&r| within(&self.nodes, j, r)) { out.push(j); }
        }
        out
    }
    /// What the last paint drew, per node, for a paint that draws only the stages at `roots` again
    /// (`Ui::restage`); `None` when it cannot be kept (each node's instances not one run, 3D elements,
    /// faces arriving). Text recomposing anywhere is kept: its tiles are drawn over the scene by id.
    fn kept(&mut self, roots: Vec<usize>) -> Option<Kept> {
        let n = self.page_len;
        if self.owners.len() != n || self.tags.len() != n || self.scene.instances.len() < n || !self.arriving.is_empty() { return None; }
        // 3D elements outside the stages keep their meshes (and their places in the list); one in a stage
        // drawn again would add its own.
        let mesh = |j: usize| matches!(self.nodes[j].content, Content::Scene3d(_));
        if roots.iter().any(|&r| (r..self.nodes.len()).take_while(|&j| within(&self.nodes, j, r)).any(mesh)) { return None; }
        let runs = runs_of(self.owners.iter().map(|o| o.0), self.nodes.len())?;
        let log_runs = runs_of(self.glyphs.log.iter().map(|g| g.node), self.nodes.len())?;
        // As painted, not as the last frame's motion left them (a glyph hidden under its tiles, one sliding).
        if self.base.len() != n { return None; }
        let insts = self.base.clone();
        Some(Kept { roots, insts, owners: std::mem::take(&mut self.owners), tags: std::mem::take(&mut self.tags), runs, log: std::mem::take(&mut self.glyphs.log), log_runs,
            laid: std::mem::take(&mut self.glyphs.laid), unseen: std::mem::take(&mut self.unseen), own: self.scene.own.clone(), meshes: std::mem::take(&mut self.scene.meshes) })
    }
    /// Put the subtree `el` (a stage viewed alone) in place of the stage at `i`, when it is laid out as
    /// the old one was: the same elements in the same places, with the same styles and the same measured
    /// content. Their boxes are kept; whether it was put.
    fn swap_stage(&mut self, i: usize, el: El<M>) -> bool {
        let key = self.nodes[i].key.clone();
        if !(el.props.stage || el.props.steps || el.props.local_messages) || (!el.props.key.is_empty() && el.props.key != key) { return false; }
        let Some(ctx) = self.nodes[i].stage.as_deref().cloned() else { return false };
        let len = (i + 1..self.nodes.len()).take_while(|&j| within(&self.nodes, j, i)).count() + 1;
        let mut nodes = std::mem::take(&mut self.nodes);
        let mut tail = nodes.split_off(i);
        let mut built = Built { base: i, ..Built::default() };
        let mut vx = VirtCtx { scroll: &self.scroll, views: &self.virt_views, window: self.size.1, made: HashMap::default() };
        flatten(el, ctx.parent, ctx.path, ctx.roles, ctx.off, ctx.flow, ctx.inh, &mut vx, &mut built, &mut nodes);
        self.stack_margins(&nodes, &mut built);
        let virt = !vx.made.is_empty();
        // An absolutely placed element whose offsets alone changed moves without a layout: nothing else
        // in a flexbox depends on where it is.
        let moves: Vec<Option<(f32, f32)>> = (0..len.min(nodes.len() - i)).map(|j| self.tree.style(tail[j].tid).ok().and_then(|st| moved_only(st, &built.styles[j]))).collect();
        let same = !virt && nodes.len() - i == len && (0..len).all(|j| {
            let (new, old) = (&nodes[i + j], &tail[j]);
            new.key == old.key && new.parent == old.parent && new.modal.is_none() && new.fit.is_none()
                && matches!(new.content, Content::None) == matches!(old.content, Content::None)
                && moves[j].is_some()
                && (self.mkeys.get(&old.tid) == Some(&measure_key(&new.content))
                    || new.clamp == 0 && same_measure(&mut self.text, &old.content, &new.content, old.local.size.width - old.local.padding.left - old.local.padding.right - old.local.border.left - old.local.border.right))
        });
        if !same {
            nodes.truncate(i);
            nodes.append(&mut tail);
            self.nodes = nodes;
            return false;
        }
        for (j, old) in tail.drain(..len).enumerate() {
            let n = &mut nodes[i + j];
            // Text that changed and measures as it did: the tree measures it by its new content from now on.
            if !matches!(n.content, Content::None) { self.mkeys.insert(old.tid, measure_key(&n.content)); }
            (n.local, n.exact, n.tid, n.rect, n.clip, n.inner, n.extent, n.shown, n.xf, n.written) = (old.local, old.exact, old.tid, old.rect, old.clip, old.inner, old.extent, old.shown, old.xf, old.written);
            if let Some((dx, dy)) = moves[j] && (dx != 0.0 || dy != 0.0) {
                // Where the layout would put it, on whole px; the tree has its new offsets for the next one.
                n.exact = (n.exact.0 + dx, n.exact.1 + dy);
                n.local.location = taffy::Point { x: n.exact.0.round(), y: n.exact.1.round() };
                self.moved.insert(n.tid, n.exact);
                let _ = self.tree.set_style(n.tid, built.styles[j].clone());
            }
        }
        nodes.append(&mut tail);
        self.nodes = nodes;
        true
    }
    /// A stack's children (`el::stack`) in the flow, as indices into `nodes`, by their styles.
    fn stack_kids(built: &Built, s: usize, placed: impl Fn(usize) -> bool) -> Vec<usize> {
        built.kids[s - built.base].iter().copied().filter(|&k| !placed(k)).collect()
    }
    /// Sets the margins of each stack's children from the widths they took in the last layout
    /// (`stack_extra`), when the stack has as many children as then.
    fn stack_margins(&self, nodes: &[Node<M>], built: &mut Built) {
        for si in 0..built.stacks.len() {
            let s = built.stacks[si];
            let Some(o) = self.stack_w.get(&nodes[s].key) else { continue };
            let kids = Self::stack_kids(built, s, |k| built.styles[k - built.base].position == taffy::Position::Absolute);
            if kids.len() != o.len() { continue; }
            for (j, k) in kids.into_iter().enumerate() {
                let (l, r) = stack_extra(o, j);
                let m = &mut built.styles[k - built.base].margin;
                (m.left, m.right) = (add_margin(m.left, l), add_margin(m.right, r));
            }
        }
    }
    /// After a layout: the widths each stack's children took, without what `stack_extra` added; where
    /// they changed the children's margins are set again in the tree. Whether any was.
    fn restack(&mut self, nodes: &[Node<M>], built: &Built) -> bool {
        let mut again = false;
        for &s in &built.stacks {
            let kids = Self::stack_kids(built, s, |k| self.tree.style(nodes[k].tid).map_or(true, |st| st.position == taffy::Position::Absolute));
            let old = self.stack_w.get(&nodes[s].key).filter(|o| o.len() == kids.len());
            let was = |j: usize| old.map_or((0.0, 0.0), |o| stack_extra(o, j));
            let o: Vec<f32> = kids.iter().enumerate().map(|(j, &k)| {
                let l = self.tree.layout(nodes[k].tid).map_or(Layout::new(), |l| *l);
                let (a, b) = was(j);
                l.size.width + l.margin.left + l.margin.right - a - b
            }).collect();
            let moved: Vec<(f32, f32)> = (0..kids.len()).map(|j| { let ((a, b), (c, d)) = (was(j), stack_extra(&o, j)); (c - a, d - b) }).collect();
            if moved.iter().any(|&(a, b)| a.abs() > 0.01 || b.abs() > 0.01) {
                for (&k, &(a, b)) in kids.iter().zip(&moved) {
                    let Ok(mut st) = self.tree.style(nodes[k].tid).cloned() else { continue };
                    (st.margin.left, st.margin.right) = (add_margin(st.margin.left, a), add_margin(st.margin.right, b));
                    let _ = self.tree.set_style(nodes[k].tid, st);
                }
                again = true;
            }
            self.stack_w.insert(nodes[s].key.clone(), o);
        }
        again
    }
    /// Where each stage's instances are in the scene and where it shows (`Scene::stages`), for the
    /// renderer to draw it at its own scale.
    fn stage_boxes(&mut self) {
        self.scene.stages.clear();
        if self.stages.is_empty() { return; }
        let mut of = vec![u32::MAX; self.nodes.len()];
        for i in 0..self.nodes.len() {
            if self.nodes[i].stage.is_none() || of[i] != u32::MAX { continue; }
            let k = self.scene.stages.len() as u32;
            for (j, o) in of.iter_mut().enumerate().skip(i) { if j > i && !within(&self.nodes, j, i) { break; } *o = k; }
            let r = self.nodes[i].rect.intersect(&self.nodes[i].clip);
            let s = self.scale;
            self.scene.stages.push(crate::scene::StageBox { start: u32::MAX, end: 0, rect: [r.x * s, r.y * s, r.right() * s, r.bottom() * s] });
        }
        for (k, &(n, _)) in self.owners.iter().enumerate() {
            let Some(&st) = of.get(n) else { continue };
            if let Some(b) = self.scene.stages.get_mut(st as usize) { b.start = b.start.min(k as u32); b.end = b.end.max(k as u32 + 1); }
        }
        self.scene.stages.retain(|b| b.end > b.start && b.rect[2] > b.rect[0] && b.rect[3] > b.rect[1]);
    }
    /// The stages' device scale for the next frame, as a share of the device's (1 at the device's own),
    /// and in `perf().stage_scale` (DESIGN.md §1.35). `pin`: the look's quality, when it is set (a
    /// stage at that share of the device's scale, the nearest step), and the governor rests; `govern`:
    /// the governor goes by the frames drawn (not for frames that are checks or stills).
    pub fn stage_scale(&mut self, pin: Option<f32>, govern: bool) -> f32 {
        self.governed = govern;
        let device = self.scale.max(0.01);
        let levels = crate::perf::StageScale::levels(device);
        self.stage_pin = pin.map(|q| levels.iter().copied().min_by(|a, b| (a - q * device).abs().total_cmp(&(b - q * device).abs())).unwrap_or(device));
        let s = self.stage_pin.unwrap_or_else(|| if govern && self.scaler.fed { self.scaler.scale() } else { device });
        self.perf.stage_scale = if s < device - 0.01 { s } else { 0.0 };
        s / device
    }
    /// The stages' scale steps from the device's own again (a new route; a new window size does this
    /// itself).
    pub fn reset_stage_scale(&mut self) { self.scaler.reset(); }
    /// The stages' governor (`perf::StageScale`), for checks that set its level.
    pub fn stage_governor(&mut self) -> &mut crate::perf::StageScale { &mut self.scaler }
    /// Whether a pointer is down (the stages' scale does not step up then).
    fn pointer_held(&self) -> bool { self.pressed.is_some() || self.capture.is_some() || self.touching }
    /// For checks: every stage's draw takes `ms` longer (`El::stage`; Y.5's slow piece).
    pub fn slow_stage(&mut self, ms: f32) { self.stage_add_ms = ms.max(0.0); }
    /// Whether the stage under `key` is slow, and how many times it has drawn (`El::stage`).
    pub fn stage_state(&self, key: &str) -> Option<(bool, usize)> { self.stages.iter().find(|(k, _)| k == key).map(|(_, s)| (s.slow, s.count)) }
    /// Whether the held stage under `key` would hold if it were painted now (slow, input since the last
    /// paint, within its hold, in the box it was drawn in).
    fn stage_holds(&self, key: &str) -> bool {
        let Some((_, st)) = self.stages.iter().find(|(k, _)| k == key) else { return false };
        let Some(i) = self.nodes.iter().position(|n| n.key == key && n.stage.as_ref().is_some_and(|c| c.held)) else { return false };
        let n = &self.nodes[i];
        let size = (i + 1..self.nodes.len()).take_while(|&j| within(&self.nodes, j, i)).count() + 1;
        !st.draws_now(n.rect, n.clip, size, self.clock, self.stage_input.0, self.stage_input.1)
    }
    /// The stages that held (through a paint, or a step that waits) and would draw if painted now.
    fn stages_due(&self) -> Vec<String> {
        let (input, released) = self.stage_input;
        self.stages.iter().filter(|(k, s)| (s.behind || self.held_steps.contains(k)) && s.draws_now(s.rect, s.clip, s.nodes, self.clock, input, released)).map(|(k, _)| k.clone()).collect()
    }
    fn stage_due(&self) -> bool {
        let (input, released) = self.stage_input;
        self.stages.iter().any(|(k, s)| (s.behind || self.held_steps.contains(k)) && s.draws_now(s.rect, s.clip, s.nodes, self.clock, input, released))
    }
    /// When the first stage that holds may draw though input goes on (its hold over), on the beat clock.
    fn stage_wake(&self) -> Option<f32> {
        self.stages.iter().filter(|(k, s)| s.behind || self.held_steps.contains(k)).map(|(_, s)| s.drawn_at + s.hold_s()).reduce(f32::min)
    }
    /// What text to read and controls hold (DESIGN.md §1.15): each of their instances that has some ink
    /// of something else (not legible) drawn before it within `r1` of it holds the looseness there to the
    /// legibility cap. `r0 = max(3, size / 4)` and `r1 = r0 + max(6, size / 2)` logical px, by the type
    /// size of the text, or for a control the label in it (a heading is a title: it holds nothing).
    /// (`near`: by the grid; false tests every earlier box, the same answer slower, for checks.)
    fn holds(&self, near: bool) -> Vec<(u32, [f32; 2])> {
        let s = self.scale;
        let insts = &self.scene.instances;
        let sp = crate::text::SPREAD as f32;
        let ink = |i: &Instance| if i.paint[3] == GLYPH { [i.rect[0] + sp, i.rect[1] + sp, i.rect[0] + i.rect[2] - sp, i.rect[1] + i.rect[3] - sp] } else { [i.rect[0], i.rect[1], i.rect[0] + i.rect[2], i.rect[1] + i.rect[3]] };
        let mut under: Vec<(usize, [f32; 4])> = vec![];
        let mut out = vec![];
        let mut size_of: HashMap<usize, f32> = HashMap::default();
        // The ink drawn before, by cell of a coarse grid over the window (each box in every cell it
        // touches, a cell index held to the grid's edge, which keeps two boxes that overlap in a cell they
        // share), so a legible instance tests only what is near it. A box that is not finite is in a
        // short list that is always tested.
        const CELL: f32 = 64.0;
        let (gw, gh) = (((self.size.0 * s / CELL).ceil() as i32).clamp(1, 1024), ((self.size.1 * s / CELL).ceil() as i32).clamp(1, 1024));
        let cells = |b: &[f32; 4]| -> Option<(i32, i32, i32, i32)> {
            if !b.iter().all(|v| v.is_finite() && v.abs() < 1e9) { return None; }
            let c = |v: f32, n: i32| ((v / CELL).floor() as i32).clamp(0, n - 1);
            Some((c(b[0], gw), c(b[1], gh), c(b[2], gw), c(b[3], gh)))
        };
        let mut grid: Vec<Vec<usize>> = vec![vec![]; (gw * gh) as usize];
        let at = |cx: i32, cy: i32| (cy * gw + cx) as usize;
        let mut wide: Vec<usize> = vec![];
        let mut bucketed = 0usize;
        for (k, inst) in insts.iter().enumerate() {
            let Some(&(ni, _)) = self.owners.get(k) else { break };
            let n = &self.nodes[ni];
            if inst.paint[2] <= 0.0 { continue; }
            if !n.legible {
                under.push((k, ink(inst)));
                continue;
            }
            if under.is_empty() || n.access.as_ref().is_some_and(|a| a.role == Some(crate::el::Role::Heading)) || !(inst.paint[3] == GLYPH || inst.paint[3] == SHAPE) { continue; }
            let size = *size_of.entry(ni).or_insert_with(|| match &n.content {
                Content::Text { size, .. } => *size,
                _ => self.nodes.iter().enumerate().skip(ni + 1).take_while(|(j, _)| within(&self.nodes, *j, ni))
                    .find_map(|(_, m)| if let Content::Text { size, .. } = m.content { Some(size) } else { None }).unwrap_or(14.0),
            });
            let r0 = (0.25 * size).max(3.0) * s;
            let mut r1 = r0 + (0.5 * size).max(6.0) * s;
            // A glyph's nearness reaches SPREAD device px from its outline: the hold fades out by there.
            if inst.paint[3] == GLYPH { r1 = r1.min(sp); }
            let r0 = r0.min(r1 - 1.0);
            let b = ink(inst);
            let g = [b[0] - r1, b[1] - r1, b[2] + r1, b[3] + r1];
            while bucketed < under.len() {
                match cells(&under[bucketed].1) {
                    Some((x0, y0, x1, y1)) => for cy in y0..=y1 { for cx in x0..=x1 { grid[at(cx, cy)].push(bucketed); } },
                    None => wide.push(bucketed),
                }
                bucketed += 1;
            }
            let hit = |u: &[f32; 4]| u[0] < g[2] && u[2] > g[0] && u[1] < g[3] && u[3] > g[1];
            // Two boxes that overlap share a cell.
            let found = match cells(&g).filter(|_| near) {
                Some((x0, y0, x1, y1)) => wide.iter().any(|&q| hit(&under[q].1)) || (y0..=y1).any(|cy| (x0..=x1).any(|cx| grid[at(cx, cy)].iter().any(|&q| hit(&under[q].1)))),
                None => under.iter().any(|(_, u)| hit(u)),
            };
            if found { out.push((k as u32, [r0, r1])); }
        }
        out
    }
    /// Text to read over others' ink keeps an island that follows its glyphs (`holds`); where two
    /// neighbouring glyphs of a word in a line are less than `2 × r1` apart, the gap between their ink
    /// boxes is held too, so a word is one island (a gap of a fifth of the text's size or more is a
    /// space between words). Each bridge is a box over the gap, the height the two glyphs share, held as
    /// a glyph is (`r0` fully, fading out by `r1`).
    fn bridges(&self, holds: &[(u32, [f32; 2])]) -> Vec<Instance> {
        let sp = crate::text::SPREAD as f32;
        let insts = &self.scene.instances;
        let mut out = vec![];
        for w in holds.windows(2) {
            let ((a, r), (b, _)) = (w[0], w[1]);
            let (ia, ib) = (&insts[a as usize], &insts[b as usize]);
            if ia.paint[3] != GLYPH || ib.paint[3] != GLYPH || self.owners[a as usize].0 != self.owners[b as usize].0 { continue; }
            let size = match &self.nodes[self.owners[a as usize].0].content { Content::Text { size, .. } => *size * self.scale, _ => continue };
            let ink = |i: &Instance| [i.rect[0] + sp, i.rect[1] + sp, i.rect[0] + i.rect[2] - sp, i.rect[1] + i.rect[3] - sp];
            let (ka, kb) = (ink(ia), ink(ib));
            let gap = kb[0] - ka[2];
            // The same line: their boxes overlap in height.
            let (top, bottom) = (ka[1].max(kb[1]), ka[3].min(kb[3]));
            if gap <= 0.0 || gap >= 2.0 * r[1] || gap >= 0.2 * size || bottom <= top { continue; }
            out.push(Instance { rect: [ka[2] - 0.5, top, gap + 1.0, bottom - top], uv: [0.0; 4], paint: [r[0], r[1], 1.0, SHAPE], clip: ia.clip, extra: [0.0; 4], look: [0.0; 4], resp: [0.0; 4], xf: [0.0; 4] });
        }
        out
    }
    /// The islands of filled controls whose own fill is loosened (`El::loose` on the control itself,
    /// DESIGN.md §1.15): the ink box of all its content, grown by `max(5, 0.36 × its text's size)` with
    /// that radius, within its outline at rest. One a control.
    /// (`walk`: up the parents, once a node; false searches every earlier node, the same answer slower,
    /// for checks.)
    fn islands(&self, walk: bool) -> Vec<Instance> {
        let s = self.scale;
        let sp = crate::text::SPREAD as f32;
        let mut boxes: HashMap<usize, [f32; 4]> = HashMap::default();
        let mut memo: HashMap<usize, Option<usize>> = HashMap::default();
        for (k, inst) in self.scene.instances.iter().enumerate() {
            let Some(&(ni, _)) = self.owners.get(k) else { break };
            if inst.paint[2] <= 0.0 { continue; }
            // The nearest filled, loosened, legible element above it (its own fill is not content): its
            // ancestors nearest first, once a node.
            let island = |j: usize| { let n = &self.nodes[j]; n.legible && n.fill.is_some() && n.loose > 0.0 };
            let c = match memo.get(&ni) { Some(c) => *c, None => {
                let a = if walk {
                    let mut a = self.nodes[ni].parent;
                    while let Some(j) = a { if island(j) { break; } a = self.nodes[j].parent; }
                    a
                } else { (0..ni).rev().find(|&j| within(&self.nodes, ni, j) && island(j)) };
                memo.insert(ni, a);
                a
            } };
            let Some(c) = c else { continue };
            let b = if inst.paint[3] == GLYPH { [inst.rect[0] + sp, inst.rect[1] + sp, inst.rect[0] + inst.rect[2] - sp, inst.rect[1] + inst.rect[3] - sp] } else { [inst.rect[0], inst.rect[1], inst.rect[0] + inst.rect[2], inst.rect[1] + inst.rect[3]] };
            let e = boxes.entry(c).or_insert(b);
            *e = [e[0].min(b[0]), e[1].min(b[1]), e[2].max(b[2]), e[3].max(b[3])];
        }
        let mut out: Vec<(usize, Instance)> = boxes.into_iter().map(|(c, b)| {
            let n = &self.nodes[c];
            let size = self.nodes.iter().enumerate().skip(c + 1).take_while(|(j, _)| within(&self.nodes, *j, c))
                .find_map(|(_, m)| if let Content::Text { size, .. } = m.content { Some(size) } else { None }).unwrap_or(14.0);
            let g = (0.36 * size).max(5.0) * s;
            let rect = [b[0] - g, b[1] - g, b[2] - b[0] + 2.0 * g, b[3] - b[1] + 2.0 * g];
            let outline = [n.rect.x * s, n.rect.y * s, n.rect.right() * s, n.rect.bottom() * s];
            (c, Instance { rect, uv: [g, 0.0, 0.0, 0.0], paint: [-3.0, 1.0, 1.0, 4.0], clip: outline, extra: [0.0; 4], look: [n.radius * s, 0.0, 0.0, 0.0], resp: [0.0; 4], xf: [0.0; 4] })
        }).collect();
        out.sort_by_key(|(c, _)| *c);
        out.into_iter().map(|(_, i)| i).collect()
    }
    /// Start what appears and what went (DESIGN.md §1.28), and keep each decoding element's instances.
    fn decode_pass(&mut self) {
        // Decodes turned off (DESIGN.md §1.34): the text is there as it ends.
        if self.timing.off.decode { self.decodes.clear(); self.decode_last.clear(); return; }
        let (now, s) = (self.clock, self.scale);
        let mut present: HashMap<String, Shown> = HashMap::default();
        for i in 0..self.nodes.len() {
            let Some(spec) = &self.nodes[i].decode else { continue };
            if self.nodes[i].hidden { continue; }
            let nodes = &self.nodes;
            let insts: Vec<Instance> = self.scene.instances[..self.page_len].iter().zip(&self.owners).filter(|(_, (k, _))| within(nodes, *k, i)).map(|(x, _)| *x).collect();
            let key = nodes[i].key.clone();
            match self.decodes.get_mut(&key) {
                Some(r) if !r.reverse => { r.spec.hold = spec.hold; }
                _ if !self.decode_last.contains_key(&key) => { self.decodes.insert(key.clone(), DecodeRun { start: now, reverse: false, spec: spec.clone(), plan: None, ghost: vec![], clip: None, drawn: None }); }
                _ => {}
            }
            if let Some(r) = self.decodes.get_mut(&key) && r.plan.is_none() && !insts.is_empty() {
                let fine = spec.tiers.last().copied().unwrap_or(4.0) * s;
                let cells: Vec<(f32, f32)> = morph::outline_cells(&insts, fine.round().max(1.0), &self.text.pixels).into_iter().filter(|c| c.cover > 0.02).map(|c| (c.x, c.y)).collect();
                r.plan = Some(spec.plan(&cells, s));
            }
            let c = nodes[i].clip;
            let clip = [c.x * s, c.y * s, c.right() * s, c.bottom() * s];
            present.insert(key, Shown { insts, spec: spec.clone(), clip });
        }
        // What went: its last instances play the transition backwards.
        let gone: Vec<String> = self.decode_last.keys().filter(|k| !present.contains_key(*k)).cloned().collect();
        for k in gone {
            // Gone with its page: nothing is played.
            if self.left.contains(&k) { self.decode_last.remove(&k); self.decodes.remove(&k); continue; }
            // Played backwards with its own spec (not the default one), inside its own clip.
            let Some(Shown { insts: ghost, spec, clip }) = self.decode_last.remove(&k) else { continue };
            let (fine, atlas) = (spec.tiers.last().copied().unwrap_or(4.0) * s, &self.text.pixels);
            let cells: Vec<(f32, f32)> = morph::outline_cells(&ghost, fine.round().max(1.0), atlas).into_iter().filter(|c| c.cover > 0.02).map(|c| (c.x, c.y)).collect();
            let plan = spec.plan(&cells, s);
            self.decodes.insert(k, DecodeRun { start: now, reverse: true, spec: Decode { hold: None, ..spec }, plan: Some(plan), ghost, clip: Some(clip), drawn: None });
        }
        self.decode_last = present;
    }
    /// Draw the decode transitions running now: hide what is not shown yet, the blocks over the rest.
    fn paint_decodes(&mut self) {
        let now = self.clock;
        self.decodes.retain(|_, r| r.spec.hold.is_some() || now - r.start < r.spec.duration_ms / 1000.0);
        if self.decodes.is_empty() { return; }
        let screen = [0.0, 0.0, self.size.0 * self.scale, self.size.1 * self.scale];
        let mut pushed = vec![];
        // A decode that runs needs the next frame; one held at a step needs one only when the step
        // changed since it was last drawn.
        let mut again = false;
        for r in self.decodes.values_mut() {
            again |= r.spec.hold.is_none() || r.drawn != r.spec.hold;
            r.drawn = r.spec.hold;
        }
        for (key, r) in &self.decodes {
            let Some(plan) = &r.plan else { continue };
            let p = ((now - r.start) / (r.spec.duration_ms / 1000.0).max(1e-3)).clamp(0.0, 1.0);
            let u = r.spec.hold.unwrap_or(if r.reverse { 1.0 - p } else { p });
            let at = self.nodes.iter().position(|n| &n.key == key);
            let clip = r.clip.unwrap_or_else(|| at.map_or(screen, |i| { let c = self.nodes[i].clip; let s = self.scale; [c.x * s, c.y * s, c.right() * s, c.bottom() * s] }));
            if let Some(i) = at && !r.reverse && !plan.shows_content(u) {
                for k in 0..self.page_len.min(self.owners.len()) { if within(&self.nodes, self.owners[k].0, i) { self.scene.instances[k].paint[2] = 0.0; } }
            }
            if r.reverse && plan.shows_content(u) { pushed.extend(r.ghost.iter().copied()); }
            for b in plan.at(u) {
                pushed.push(Instance { rect: [b.x, b.y, b.size, b.size], paint: [1.0, 1.0, 1.0, SHAPE], clip, extra: [0.0, 0.0, 0.0, -1e4], look: [0.0, b.hue as f32, 0.0, 0.0], resp: IGNORE, ..Default::default() });
            }
        }
        for i in pushed { self.scene.push(i); }
        if again { self.dirty = true; }
    }

    /// Whether motes are moving: shed marks in flight, or a flow field with speed.
    pub fn motes_moving(&self) -> bool { !self.motes.is_empty() || self.nodes.iter().any(|n| n.flow_field.is_some_and(|f| f.speed > 0.0) && !n.hidden) }
    /// Shed marks in flight now (for tests and the readout).
    pub fn motes(&self) -> &[crate::motes::Mote] { &self.motes }
    /// The box (logical px) a stir with `scope` reaches when it comes from node `i` (or from no node).
    fn stir_scope(&self, i: Option<usize>, scope: &Scope) -> Option<[f32; 4]> {
        let window = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let seen = |n: &Node<M>| n.rect.intersect(&n.clip);
        let r = match scope {
            Scope::Off => return None,
            Scope::Global => return Some(crate::fieldmap::EVERYWHERE),
            Scope::Own => i.map_or(window, |i| seen(&self.nodes[i])),
            Scope::Container(None) => i.and_then(|i| self.up(i, |n| n.ripple_scope)).map_or(window, |c| seen(&self.nodes[c])),
            Scope::Container(Some(k)) => self.nodes.iter().find(|n| &n.key == k).map_or(window, seen),
        };
        Some([r.x, r.y, r.right(), r.bottom()])
    }
    /// `Cmd::Stir` (DESIGN.md §1.29).
    fn stir_cmd(&mut self, at: crate::morph::StirAt, v: [f32; 2], energy: f32, radius: f32, scope: Scope) {
        use crate::morph::StirAt;
        self.fit_map();
        let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let flow = len * self.timing.map.flow;
        match at {
            StirAt::Point(p) => {
                let Some(sc) = self.stir_scope(None, &scope) else { return };
                let dir = if len > 0.0 { [v[0] / len, v[1] / len] } else { [0.0; 2] };
                self.fmap.stir(crate::fieldmap::Stir { at: p, dir, energy, flow, radius, scope: sc });
            }
            StirAt::Key(k) => {
                let Some(i) = self.nodes.iter().position(|n| n.key == k) else { return };
                let Some(sc) = self.stir_scope(Some(i), &scope) else { return };
                let (x, y) = self.nodes[i].rect.center();
                let dir = if len > 0.0 { [v[0] / len, v[1] / len] } else { [0.0; 2] };
                self.fmap.stir(crate::fieldmap::Stir { at: [x, y], dir, energy, flow, radius, scope: sc });
            }
            StirAt::Path { points, from, to } => {
                let Some(sc) = self.stir_scope(None, &scope) else { return };
                let total: f32 = points.windows(2).map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt()).sum();
                let (a, b) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
                if total <= 0.0 || b <= a { return; }
                // A stir every half cell along the part drawn, the flow along the path and across it.
                let n = ((total * (b - a)) / (self.fmap.cell * 0.5)).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let (p, d) = crate::fieldmap::along(&points, a + (b - a) * k as f32 / n as f32);
                    let f = [d[0] * v[0] - d[1] * v[1], d[1] * v[0] + d[0] * v[1]];
                    let fl = (f[0] * f[0] + f[1] * f[1]).sqrt();
                    let dir = if fl > 0.0 { [f[0] / fl, f[1] / fl] } else { [0.0; 2] };
                    self.fmap.stir(crate::fieldmap::Stir { at: p, dir, energy, flow: fl * self.timing.map.flow, radius, scope: sc });
                }
            }
        }
        self.dirty = true;
    }
    /// The map over the window at the look's cell size, the calm boxes and the viewer's preference.
    fn fit_map(&mut self) {
        let cell = self.timing.map.cell;
        if self.fmap.fit(self.size.0, self.size.1, cell) { self.stirr_at.clear(); self.fmap_ink = u64::MAX; self.fmap_inks.clear(); }
        let s = self.scale;
        self.fmap.calm = self.field.calm.iter().filter(|r| r[2] > r[0]).map(|r| [r[0] / s, r[1] / s, r[2] / s, r[3] / s]).collect();
        self.fmap.lives = self.nodes.iter().filter(|n| !n.hidden).filter_map(|n| n.map_life.map(|(e, f)| { let r = n.rect.intersect(&n.clip); ([r.x, r.y, r.right(), r.bottom()], e, f) })).collect();
    }
    /// Step the field map by `dt` s and stir it where `stirr` elements went since the last tick (the
    /// distance their centres moved, as shedding measures it); whether its bytes changed.
    fn step_map(&mut self, dt: f32) -> bool {
        self.fit_map();
        let fx = self.timing.map;
        // A new window size moves everything: that is not motion.
        if self.size != self.fmap_size { self.stirr_at.clear(); self.fmap_size = self.size; }
        // Where there is ink (for creep), taken again when the scene changed and the map is in use.
        if fx.creep > 0.0 && self.fmap.moving(&fx) && self.fmap_ink != self.perf.counters.scenes {
            self.fmap_ink = self.perf.counters.scenes;
            let ink = self.map_ink();
            self.fmap.set_ink(ink);
        }
        let before = self.fmap.snapshot().bytes.clone();
        self.fmap.advance(dt, &fx);
        let mut seen: Vec<String> = vec![];
        // (from, to, energy, flow, radius, scope) of each element that moved.
        type Trail = ([f32; 2], [f32; 2], f32, f32, f32, [f32; 4]);
        let mut stirs: Vec<Trail> = vec![];
        for (i, n) in self.nodes.iter().enumerate() {
            let Some(st) = n.stirr.as_ref() else { continue };
            if n.hidden { continue; }
            // Where it is in the content it scrolls with: scrolling carries it, which is not its motion.
            let mut c = n.rect.center();
            let mut a = n.parent;
            while let Some(k) = a {
                let p = &self.nodes[k];
                if p.scroll { let o = self.scroll.get(&p.key).copied().unwrap_or(0.0); if p.horizontal { c.0 += o; } else { c.1 += o; } }
                a = p.parent;
            }
            seen.push(n.key.clone());
            let Some(last) = self.stirr_at.insert(n.key.clone(), c) else { continue };
            let d = ((c.0 - last.0).powi(2) + (c.1 - last.1).powi(2)).sqrt();
            let energy = st.energy * fx.stirr;
            if d < 0.5 || energy <= 0.0 { continue; }
            let Some(scope) = self.stir_scope(Some(i), &st.scope) else { continue };
            let a = (d / 24.0).min(1.0);
            let radius = st.radius.unwrap_or(fx.stirr_radius + 0.5 * n.rect.w.min(n.rect.h));
            let at = n.rect.center();
            stirs.push(([last.0 - c.0 + at.0, last.1 - c.1 + at.1], [at.0, at.1], energy * a, fx.flow * a, radius, scope));
        }
        self.stirr_at.retain(|k, _| seen.contains(k));
        // A trace's head stirs the map where it goes (DESIGN.md §1.30).
        let mut heads: Vec<String> = vec![];
        for (i, n) in self.nodes.iter().enumerate() {
            let Some(tr) = n.trace.as_ref().filter(|t| t.stir > 0.0 && !n.hidden) else { continue };
            let o = n.origin();
            let h = crate::strand::at_length(&tr.points, crate::strand::length(&tr.points) * tr.progress.clamp(0.0, 1.0));
            let h = [o.0 + h[0], o.1 + h[1]];
            heads.push(n.key.clone());
            let Some(last) = self.trace_head.insert(n.key.clone(), h) else { continue };
            let d = ((h[0] - last[0]).powi(2) + (h[1] - last[1]).powi(2)).sqrt();
            let energy = tr.stir * fx.stirr;
            if d < 0.5 || energy <= 0.0 { continue; }
            let Some(scope) = self.stir_scope(Some(i), &Scope::Container(None)) else { continue };
            let a = (d / 24.0).min(1.0);
            stirs.push((last, h, energy * a, fx.flow * a, fx.stirr_radius, scope));
        }
        self.trace_head.retain(|k, _| heads.contains(k));
        for (a, b, e, f, r, sc) in stirs { self.fmap.stir_along(a, b, e, f, r, sc); }
        !std::rc::Rc::ptr_eq(&before, &self.fmap.snapshot().bytes) && *before != *self.fmap.snapshot().bytes
    }
    /// How much ink each node of the map has (0..1): the coverage of what reads the map, at the node and
    /// a quarter cell around it.
    fn map_ink(&mut self) -> Vec<f32> {
        use std::hash::{Hash, Hasher};
        let m = &self.fmap;
        let (s, c) = (self.scale, m.cell);
        let mut ink = vec![0.0f32; m.cols * m.rows];
        let mut kept: HashMap<u64, Vec<(u32, f32)>> = HashMap::with_capacity_and_hasher(self.fmap_inks.len(), Default::default());
        let atlas = self.text.pixels.len();
        for inst in self.scene.instances[..self.page_len.min(self.scene.instances.len())].iter().filter(|i| i.look[2] > 0.5 && i.paint[2] > 0.01) {
            let b = inst.bounds(0.0);
            // A blade is worked out on the GPU (its rect is only where it can reach); nothing creeps in
            // a box with lives of its own, so what lies all inside one is not read.
            if inst.blade() || m.lives.iter().any(|(l, _, _)| b[0] / s >= l[0] && b[1] / s >= l[1] && b[2] / s <= l[2] && b[3] / s <= l[3]) { continue; }
            let mut h = crate::fasthash::FastHasher::new();
            for v in inst.rect.iter().chain(&inst.uv).chain(&inst.paint).chain(&inst.clip).chain(&inst.xf).chain(&inst.extra[..1]) { v.to_bits().hash(&mut h); }
            atlas.hash(&mut h);
            let key = h.finish();
            let cells = match self.fmap_inks.remove(&key).or_else(|| kept.get(&key).cloned()) {
                Some(v) => v,
                None => {
                    self.perf.counters.inked += 1;
                    let (i0, i1) = (((b[0] / s) / c).floor().max(0.0) as usize, (((b[2] / s) / c).ceil().max(0.0) as usize).min(m.cols - 1));
                    let (j0, j1) = (((b[1] / s) / c).floor().max(0.0) as usize, (((b[3] / s) / c).ceil().max(0.0) as usize).min(m.rows - 1));
                    let mut v = vec![];
                    for j in j0..=j1 {
                        for i in i0..=i1 {
                            let (x, y) = (i as f32 * c * s, j as f32 * c * s);
                            let q = c * s * 0.25;
                            let a = [(0.0, 0.0), (-q, -q), (q, -q), (-q, q), (q, q)].iter().map(|(dx, dy)| crate::morph::ink_at(inst, x + dx, y + dy, &self.text.pixels)).fold(0.0, f32::max);
                            if a > 0.0 { v.push(((j * m.cols + i) as u32, a)); }
                        }
                    }
                    v
                }
            };
            for &(k, a) in &cells { let k = k as usize; ink[k] = ink[k].max(a); }
            kept.insert(key, cells);
        }
        self.fmap_inks = kept;
        ink
    }
    /// Move the motes by `dt` s (DESIGN.md §1.22); whether anything changed.
    fn step_motes(&mut self, dt: f32) -> bool {
        use crate::motes::{FlowField, Mote};
        use crate::field::hash;
        let now = self.clock;
        let mut changed = false;
        // Shedding: an element that moved since the last tick leaves marks along the way it went.
        let mut seen: Vec<String> = vec![];
        // The lines of text shed marks keep off (only worked out when some shedder asks to).
        let wants_clear = self.motes.iter().any(|m| m.clear > 0.0) || self.nodes.iter().any(|n| n.shed.is_some_and(|s| s.rate > 0.0 && s.clear > 0.0) && !n.hidden);
        let mut lines: Vec<[f32; 4]> = vec![];
        if wants_clear {
            let text = &mut self.text;
            for n in self.nodes.iter().filter(|n| !n.hidden) {
                let Some(p) = n.para() else { continue };
                for r in text.line_boxes(p, n.origin()) { if r.w > 0.0 { lines.push([r.x, r.y, r.right(), r.bottom()]); } }
            }
        }
        let near = |x: f32, y: f32| lines.iter().map(|b| crate::motes::distance(x, y, *b)).fold(f32::INFINITY, f32::min);
        for n in self.nodes.iter().filter(|n| n.shed.is_some_and(|s| s.rate > 0.0) && !n.hidden) {
            let sh = n.shed.expect("filtered");
            let c = n.rect.center();
            seen.push(n.key.clone());
            let Some(&last) = self.shed_at.get(&n.key) else { self.shed_at.insert(n.key.clone(), c); continue };
            let d = ((c.0 - last.0).powi(2) + (c.1 - last.1).powi(2)).sqrt();
            self.shed_at.insert(n.key.clone(), c);
            if d < 0.25 { continue; }
            let owed = self.shed_owed.get(&n.key).copied().unwrap_or(0.0) + sh.rate * d / 100.0;
            let k = owed.floor() as u32;
            self.shed_owed.insert(n.key.clone(), owed - k as f32);
            let paint = n.fill.unwrap_or(crate::el::Paint::INK);
            // New marks come out of the frame budget (DESIGN.md §1.27).
            for j in 0..crate::budget::grant(k as usize) as u32 {
                let m = self.motes_made.wrapping_add(j);
                // Somewhere on the element's edge, between where it was and where it is.
                let (u, v, f) = (hash(m * 3 + 1), hash(m * 3 + 2), hash(m * 3 + 3));
                let side = (u * 4.0) as u32;
                let (ex, ey) = match side { 0 => (n.rect.x + v * n.rect.w, n.rect.y), 1 => (n.rect.right(), n.rect.y + v * n.rect.h), 2 => (n.rect.x + v * n.rect.w, n.rect.bottom()), _ => (n.rect.x, n.rect.y + v * n.rect.h) };
                let back = (last.0 - c.0, last.1 - c.1);
                let (mx, my) = (ex + back.0 * f, ey + back.1 * f);
                if sh.clear > 0.0 && near(mx, my) < sh.clear { continue; }
                self.motes.push(Mote { x: mx, y: my, vx: (hash(m * 7 + 5) - 0.5) * 2.0 * sh.drift, vy: (hash(m * 7 + 6) - 0.5) * sh.drift, born: now, life: sh.life_ms / 1000.0, size: sh.size, gravity: sh.gravity, paint, clear: sh.clear, veil: 1.0 });
            }
            self.motes_made = self.motes_made.wrapping_add(k);
            changed |= k > 0;
        }
        self.shed_at.retain(|k, _| seen.contains(k));
        if !self.motes.is_empty() {
            for m in &mut self.motes {
                m.vy += m.gravity * dt; m.x += m.vx * dt; m.y += m.vy * dt;
                // Nearing text, it fades; it is gone a pixel before it would touch.
                if m.clear > 0.0 { let d = near(m.x, m.y) - m.size; m.veil = m.veil.min(((d - 1.0) / m.clear).clamp(0.0, 1.0)); }
            }
            self.motes.retain(|m| now - m.born < m.life && m.veil > 0.0);
            changed = true;
        }
        // Flow fields: their marks are made once per box size and move while the speed is above 0.
        let mut fields: Vec<String> = vec![];
        // Only fields that can be seen are stepped; each frame's marks come out of the budget.
        for n in self.nodes.iter().filter(|n| n.flow_field.is_some() && !n.hidden && { let r = n.rect.intersect(&n.clip); r.w > 0.0 && r.h > 0.0 }) {
            let f: FlowField = n.flow_field.expect("filtered");
            fields.push(n.key.clone());
            let (w, h) = (n.rect.w, n.rect.h);
            let want = crate::budget::grant(f.count(w, h));
            let marks = self.field_marks.entry(n.key.clone()).or_default();
            if marks.len() != want { *marks = (0..want).map(|i| FlowField::seed(i, w, h)).collect(); changed = true; }
            if f.speed > 0.0 && dt > 0.0 { for p in marks.iter_mut() { *p = f.advect(*p, w, h, dt); } changed = true; }
        }
        self.field_marks.retain(|k, _| fields.contains(k));
        changed
    }
    /// scribblr's threads and the traces, as round-capped bars over the page (DESIGN.md §1.30). Each loose
    /// text or shape element's fill fades by `strand::fill(k)` and one thread leaves its true outline;
    /// at looseness 0 nothing is added and nothing is faded.
    fn paint_strands(&mut self) {
        use crate::strand::{self, Kind, Seg};
        // Last frame's fades undone: looseness is measured on the element as it is.
        for (&i, &a) in &self.strand_faded { if let Some(inst) = self.scene.instances.get_mut(i) { inst.paint[2] = a; if inst.paint[3] == SHAPE { inst.uv[1] = 0.0; } } }
        self.strand_faded.clear();
        self.strand_drawn.clear();
        self.trace_drawn.clear();
        // No thread and no trace: nothing to measure (a field of blades is not walked every frame).
        if self.timing.material != crate::style::Material::Scribblr.id() && !self.nodes.iter().any(|n| n.trace.is_some() && !n.hidden) {
            self.strand_hold.clear();
            return;
        }
        let s = self.scale;
        let now = self.clock;
        let screen = [0.0, 0.0, self.size.0 * s, self.size.1 * s];
        let bar = |a: [f32; 2], b: [f32; 2], w: f32, paint: [f32; 4], look: [f32; 2], resp: [f32; 4], clip: [f32; 4]| -> Instance {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt();
            let (cx, cy) = ((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5);
            Instance { rect: [cx - (len + w) * 0.5, cy - w * 0.5, len + w, w], uv: [w * 0.5, 0.0, 0.0, 0.0], paint, clip, extra: [0.0, 0.0, 0.0, -1e4], look: [look[0], look[1], 0.0, 0.0], resp, xf: if len > 0.0 { let (c, sn) = (dx / len, dy / len); [c - 1.0, -sn, sn, c - 1.0] } else { [0.0; 4] } }
        };
        // Where pictures are drawn (device px): a line goes behind them, as the first pass would read a
        // line's ink over a picture as the picture's own colours.
        let pics: Vec<[f32; 4]> = self.scene.instances[..self.page_len.min(self.scene.instances.len())].iter()
            .filter(|i| i.paint[3] == PICTURE && i.paint[2] > 0.0)
            .map(|i| [i.rect[0].max(i.clip[0]), i.rect[1].max(i.clip[1]), (i.rect[0] + i.rect[2]).min(i.clip[2]), (i.rect[1] + i.rect[3]).min(i.clip[3])])
            .filter(|b| b[2] > b[0] && b[3] > b[1]).collect();
        let bar = |a: [f32; 2], b: [f32; 2], w: f32, paint: [f32; 4], look: [f32; 2], resp: [f32; 4], clip: [f32; 4], out: &mut Vec<Instance>| {
            let pad = w * 0.5 + 0.5;
            let grown: Vec<[f32; 4]> = pics.iter().map(|p| [p[0] - pad, p[1] - pad, p[2] + pad, p[3] + pad]).collect();
            for (a, b) in strand::outside(a, b, &grown) { out.push(bar(a, b, w, paint, look, resp, clip)); }
        };
        // Each element's own glyphs and shapes (not pictures, 3D views or drawings), in reading order.
        let mut own: Vec<Vec<usize>> = vec![vec![]; self.nodes.len()];
        for i in 0..self.page_len.min(self.scene.instances.len()) {
            let inst = &self.scene.instances[i];
            if (inst.paint[3] != GLYPH && inst.paint[3] != SHAPE) || inst.blade() { continue; }
            if inst.paint[2] <= 0.0 || inst.rect[2] <= 0.0 || inst.rect[3] <= 0.0 { continue; }
            own[self.owners[i].0].push(i);
        }
        // Morphs, recomposition and decoding hand over to the first pass's own ink pixel for pixel, so
        // their elements loosen as plain does (no thread).
        let running = self.run.is_some();
        let (grain_k, atlas) = (if crate::budget::usage().scale < 0.5 { 2.0 } else { 1.0 }, &self.text.pixels);
        // What the tails keep out of: calm boxes and the boxes of text to read and controls (not pictures).
        let mut calm: Vec<[f32; 4]> = self.field.calm.iter().filter(|r| r[2] > r[0]).copied().collect();
        let ink_from = calm.len();
        // Each one's ink: its instances' boxes (a glyph's without the spread around it), within its clip.
        let spread = crate::text::SPREAD as f32;
        for (ni, idx) in own.iter().enumerate() {
            let n = &self.nodes[ni];
            // A line whose face is changing comes apart itself as the sweep passes: nothing keeps out of it.
            // Nor does a control's own fill held loose (it threads; its label keeps its box).
            if idx.is_empty() || !n.legible || n.hidden || self.swept_nodes.contains(&ni) || self.fill_threads(ni, idx) { continue; }
            let b = idx.iter().map(|&i| { let r = self.scene.instances[i].rect; let p = if self.scene.instances[i].paint[3] == GLYPH { spread } else { 0.0 }; [r[0] + p, r[1] + p, r[0] + r[2] - p, r[1] + r[3] - p] })
                .fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |a, r| [a[0].min(r[0]), a[1].min(r[1]), a[2].max(r[2]), a[3].max(r[3])]);
            let c = [n.clip.x * s, n.clip.y * s, n.clip.right() * s, n.clip.bottom() * s];
            let b = [b[0].max(c[0]), b[1].max(c[1]), b[2].min(c[2]), b[3].min(c[3])];
            if b[2] > b[0] && b[3] > b[1] { calm.push(b); }
        }
        // Traces, under any material: the drawn part, pushed by the map's flow.
        let drift = self.timing.map.drift;
        let mut out: Vec<Instance> = vec![];
        // Under a modal layer the page holds its lines back: they would be drawn in the layer, over it.
        // Only a dialog's layer: a floating window's or a lifted element's layer starts one too.
        let modal = self.modal_at.and(self.scene.layer).map(|l| l as usize);
        // What a drawn path keeps clear of: pictures (it goes round them, 12 px out) and text to read (its
        // tips stop 24 px short of it).
        let pics: Vec<[f32; 4]> = self.nodes.iter().filter(|n| n.kind() == "image" && !n.hidden).map(|n| { let r = n.rect.intersect(&n.clip); [r.x * s, r.y * s, r.right() * s, r.bottom() * s] }).filter(|b| b[2] > b[0] && b[3] > b[1]).collect();
        let read: Vec<[f32; 4]> = calm[ink_from..].to_vec();
        for n in self.nodes.iter().filter(|n| n.trace.is_some() && !n.hidden && modal.is_none()) {
            let tr = n.trace.as_ref().expect("filtered");
            let o = n.origin();
            let pts: Vec<[f32; 2]> = tr.points.iter().map(|p| {
                let (x, y) = (o.0 + p[0], o.1 + p[1]);
                let f = self.fmap.flow_at(x, y);
                [(x + f[0] * drift * tr.pull) * s, (y + f[1] * drift * tr.pull) * s]
            }).collect();
            let pts = strand::clear_path(pts, &pics, 12.0 * s, &read, 24.0 * s);
            self.trace_drawn.push((n.key.clone(), pts.clone()));
            let pieces = strand::dashes(&pts, tr.progress, tr.dash * s, tr.gap * s);
            let want: usize = pieces.iter().map(|p| p.len().saturating_sub(1)).sum();
            let got = crate::budget::grant(want);
            if got == 0 { continue; }
            // Fewer bars than asked: each piece simplified, a quarter pixel off it and then twice as far
            // each time until it fits, so it stays a curve with fewer, longer bars (and still unbroken).
            let mut pieces = pieces;
            let mut tol = 0.25 * s;
            while pieces.iter().map(|p| p.len().saturating_sub(1)).sum::<usize>() > got && tol < 64.0 * s {
                pieces = pieces.iter().map(|p| strand::simplify(p, tol)).collect();
                tol *= 2.0;
            }
            let have: usize = pieces.iter().map(|p| p.len().saturating_sub(1)).sum();
            let every = have.div_ceil(got).max(1);
            let clip = [n.clip.x * s, n.clip.y * s, n.clip.right() * s, n.clip.bottom() * s];
            let p = tr.paint;
            let w = line_px(tr.weight * s, self.timing.binary);
            for piece in pieces {
                let mut keep: Vec<[f32; 2]> = piece.iter().step_by(every).copied().collect();
                if keep.last() != piece.last() { keep.push(*piece.last().expect("a point")); }
                for ab in keep.windows(2) { bar(ab[0], ab[1], w, [p.tone, p.accent, p.alpha, SHAPE], [0.0, p.theme_hue()], IGNORE, clip, &mut out); }
            }
        }
        if self.timing.material != crate::style::Material::Scribblr.id() {
            self.strand_hold.clear();
            for i in out { self.scene.push(i); }
            return;
        }
        let fx = self.timing.strand;
        let hold = self.timing.beat.step_s() * 1000.0;
        // The thread's pose follows the material clock, which runs only while something moves: an element
        // held loose at rest keeps its pose and asks for no frames.
        let clock = self.mclock;
        let beat = if hold > 0.0 { (clock * 1000.0 / hold).floor() as i64 } else { (clock * 1000.0).round() as i64 };
        let t = if hold > 0.0 { beat as f32 * hold / 1000.0 } else { clock };
        // A transition's Loosen steps loosen everything, on the CPU as the material pass does (render.rs).
        let tr_k = match &self.split {
            Some(sp) if self.timing.transition > 0.0 && self.split_alive(sp, now) =>
                self.look().transition.loosen(((now - sp.t) / (self.timing.transition * sp.span(self.size.0 * self.scale, self.size.1 * self.scale))).clamp(0.0, 1.0)).min(self.timing.loose_max),
            _ => 0.0,
        };
        let mut keep_hold: Vec<String> = vec![];
        let mut fades: Vec<(usize, f32, f32)> = vec![];
        for (ni, idx) in own.iter().enumerate() {
            if idx.is_empty() { continue; }
            let n = &self.nodes[ni];
            // Text to read and controls hold: no thread, no fade, not carried by the map (DESIGN.md §1.30),
            // except as their face changes (DESIGN.md §1.6.5).
            if (n.legible && !self.swept_nodes.contains(&ni) && !self.fill_threads(ni, idx)) || n.hidden || modal.is_some_and(|l| idx[0] < l) || matches!(n.content, Content::Canvas(_)) || self.recomps.contains_key(&n.key) || self.decodes.contains_key(&n.key) || (running && self.roles[ni] != 0) || idx.iter().any(|&i| self.flows.contains_key(&self.owners[i].1)) { continue; }
            // As the thread shows it: through 0.15–0.4 quickly (`strand::shown`).
            // A line whose face is changing is one thread: as loose as its glyphs are on average, so it
            // comes apart a little as the sweep passes and is whole again after it.
            let k = if self.swept_nodes.contains(&ni) { idx.iter().map(|&i| self.inst_loose(i, now)).sum::<f32>() / idx.len() as f32 } else { idx.iter().map(|&i| self.inst_loose(i, now)).fold(0.0, f32::max) };
            let k = strand::shown(k.max(tr_k));
            if k <= 0.0 { continue; }
            let f = strand::fill(k);
            for &i in idx { fades.push((i, self.scene.instances[i].paint[2], f)); }
            // The thread: held for a step of the beat (the pose; its opacity follows `k` every frame).
            let segs: Vec<Seg> = match self.strand_hold.get(&n.key) {
                Some((b, _, segs)) if hold > 0.0 && *b == beat => segs.clone(),
                _ => {
                    let g = fx.grain * s * grain_k;
                    let mut loops: Vec<Vec<[f32; 2]>> = vec![];
                    // Where the first pass draws an instance the map carries (vertex shader: its centre's
                    // flow × drift), so the thread starts on the outline as drawn.
                    let carried = |inst: &Instance| -> [f32; 2] {
                        let c = if inst.resp[1] > 1.5 { [inst.resp[2], inst.resp[3]] } else { [inst.rect[0] + inst.rect[2] * 0.5, inst.rect[1] + inst.rect[3] * 0.5] };
                        let calm_here = calm.iter().any(|b| c[0] >= b[0] && c[1] >= b[1] && c[0] < b[2] && c[1] < b[3]);
                        if inst.look[2] <= 0.5 || inst.look[3] % 2.0 >= 0.5 || inst.resp[1] < 0.5 || calm_here { return [0.0; 2]; }
                        let f = self.fmap.flow_at(c[0] / s, c[1] / s);
                        [f[0] * drift * s, f[1] * drift * s]
                    };
                    for &i in idx {
                        let mut inst = self.scene.instances[i];
                        let off = carried(&inst);
                        if inst.paint[3] == SHAPE {
                            inst.rect[0] += off[0];
                            inst.rect[1] += off[1];
                            loops.push(strand::box_outline(inst.rect, inst.uv[0], inst.xf, g));
                            continue;
                        }
                        inst.rect[0] += off[0];
                        inst.rect[1] += off[1];
                        let key = { use std::hash::{Hash, Hasher}; let mut h = crate::fasthash::FastHasher::new(); for v in inst.uv.iter().chain(&inst.rect[2..]).chain(&inst.xf) { v.to_bits().hash(&mut h); } g.to_bits().hash(&mut h); h.finish() };
                        let rel = self.strand_cache.entry(key).or_insert_with(|| {
                            let mut at = inst;
                            at.rect[0] = 0.0; at.rect[1] = 0.0;
                            at.clip = EVERYWHERE;
                            strand::outline(&at, atlas, g)
                        });
                        for l in rel.iter() { loops.push(l.iter().map(|p| [p[0] + inst.rect[0], p[1] + inst.rect[1]]).collect()); }
                    }
                    let rest = strand::join(&loops);
                    let scope = self.stir_scope(Some(ni), &Scope::Container(None)).map_or(screen, |b| [b[0] * s, b[1] * s, b[2] * s, b[3] * s]);
                    // The line height the line is simplified by: a text's line, a shape's short side.
                    let lh = match &n.content { Content::Text { line, .. } => *line, _ => n.rect.w.min(n.rect.h) } * s;
                    // A held pose is never one of the in-between looks (`strand::held`).
                    let kp = if hold > 0.0 { strand::held(k) } else { k };
                    let c = strand::Ctx { k: kp, t, seed: (ni as u32).wrapping_mul(7919), scale: s, drift, scope, calm: &calm, pics: &pics, lh };
                    let map = &self.fmap;
                    let field = |x: f32, y: f32| (map.energy_at(x / s, y / s), map.flow_at(x / s, y / s));
                    // Knots and tails belong to text: an element of shapes only (a rule, a box, a ground)
                    // comes apart into a line that wavers gently and reaches half as far.
                    let shapes = idx.iter().all(|&i| self.scene.instances[i].paint[3] == SHAPE);
                    let fx = if shapes { strand::StrandFx { knot: 0.0, tail: 0.0, reach: fx.reach * 0.5, ..fx } } else { fx };
                    let want = ((rest.pts.len() + 6 * strand::knots(fx.knot, 1.0, k) + 64) * 3).min(2400);
                    let got = crate::budget::grant(want);
                    strand::thread(&rest, &fx, &c, &field, got)
                }
            };
            if hold > 0.0 { self.strand_hold.insert(n.key.clone(), (beat, k, segs.clone())); keep_hold.push(n.key.clone()); }
            let first = self.scene.instances[idx[0]];
            let alpha = idx.iter().map(|&i| self.scene.instances[i].paint[2]).fold(0.0, f32::max);
            let (line_a, bridge_a) = strand::opacity(k);
            let resp = [0.0, first.resp[1], first.resp[2], first.resp[3]];
            let w = line_px(fx.weight * s, self.timing.binary);
            for g in &segs {
                // The knots and the line bright, the tails quiet.
                let a = alpha * match g.kind { Kind::Line => line_a, Kind::Tail => bridge_a * fx.tail_tone, _ => bridge_a };
                if a <= 0.0 { continue; }
                bar(g.a, g.b, w, [first.paint[0], first.paint[1], a, SHAPE], [first.look[0], first.look[1]], resp, first.clip, &mut out);
            }
            self.strand_drawn.push((n.key.clone(), k, segs));
        }
        self.strand_hold.retain(|k, _| keep_hold.contains(k));
        for (i, a, f) in fades {
            self.strand_faded.insert(i, a);
            let inst = &mut self.scene.instances[i];
            inst.paint[2] = a * f;
            // Its alpha before the fade, for where text to read holds it (a shape's uv.y is free).
            if inst.paint[3] == SHAPE && f < 1.0 { inst.uv[1] = a; }
        }
        self.scene.instances.reserve(out.len());
        for i in out { self.scene.push(i); }
    }
    /// Whether scribblr threads a control's own fill (DESIGN.md §1.30): one held loose (`El::loose`) whose
    /// own instances are all shapes. Its label is text to read and holds; the fill comes apart round it, and
    /// within `r0` of the label it is whole (its island, as in every material).
    fn fill_threads(&self, ni: usize, idx: &[usize]) -> bool {
        self.nodes[ni].loose > 0.0 && idx.iter().all(|&i| self.scene.instances[i].paint[3] == SHAPE)
    }
    /// The threads scribblr drew in the last frame: each element's key, its looseness and its pieces
    /// (device px; DESIGN.md §1.30).
    pub fn strands(&self) -> &[(String, f32, Vec<crate::strand::Seg>)] { &self.strand_drawn }
    /// The INK leaders of the last frame (`Gaze` with INK): each annotated element's key and its leader's
    /// whole path, device px, from its free end to the point on the element's true outline.
    pub fn leaders(&self) -> &[(String, Vec<[f32; 2]>)] { &self.leaders }
    /// When the echoes of element `key`'s INK box (`Gaze::echo`) were taken, on the beat clock, oldest
    /// first (the last is the box as it is now).
    pub fn echo_times(&self, key: &str) -> Vec<f32> { self.echoes.get(key).map(|h| h.iter().map(|e| e.0).collect()).unwrap_or_default() }
    /// The drawn paths (`El::trace`) of the last frame: each element's key and its whole path as kept
    /// clear of pictures and text to read (device px), before progress and dashes cut it.
    pub fn traces(&self) -> &[(String, Vec<[f32; 2]>)] { &self.trace_drawn }

    /// The motes as instances, over the page (shapes in the ink, under the annotation layer).
    fn paint_motes(&mut self) {
        if self.motes.is_empty() && self.field_marks.is_empty() { return; }
        let s = self.scale;
        let screen = [0.0, 0.0, self.size.0 * s, self.size.1 * s];
        let now = self.clock;
        let mote = |x: f32, y: f32, size: f32, p: crate::el::Paint, alpha: f32, clip: [f32; 4]| Instance {
            rect: [((x - size * 0.5) * s).round(), ((y - size * 0.5) * s).round(), (size * s).round().max(1.0), (size * s).round().max(1.0)],
            uv: [size * s * 0.5, 0.0, 0.0, 0.0], paint: [p.tone, p.accent, alpha, SHAPE], clip, extra: [0.0, 0.0, 0.0, -1e4], look: [0.0, p.theme_hue(), 0.0, 0.0], resp: IGNORE, xf: [0.0; 4],
        };
        for n in self.nodes.iter().filter(|n| n.flow_field.is_some() && !n.hidden) {
            let (f, Some(marks)) = (n.flow_field.expect("filtered"), self.field_marks.get(&n.key)) else { continue };
            let clip = [n.clip.x.max(n.rect.x) * s, n.clip.y.max(n.rect.y) * s, n.clip.right().min(n.rect.right()) * s, n.clip.bottom().min(n.rect.bottom()) * s];
            for &(x, y) in marks { self.scene.push(mote(n.rect.x + x, n.rect.y + y, f.size, crate::el::Paint::INK, f.alpha, clip)); }
        }
        for m in &self.motes {
            let fade = (1.0 - (now - m.born) / m.life.max(1e-3)).clamp(0.0, 1.0);
            self.scene.push(mote(m.x, m.y, m.size, m.paint, m.paint.alpha * fade * m.veil, screen));
        }
    }
    /// Apply what moves without a new layout (morph ramps, flows, the old page dissolving), then the marks.
    fn finish(&mut self) {
        let t0 = crate::perf::now_ms();
        self.stage_boxes();
        self.animate();
        self.paint_decodes();
        self.paint_strands();
        self.paint_motes();
        // The page as animated, with its motes: what the hover marks are drawn over.
        self.body_len = self.scene.instances.len();
        self.paint_gaze();
        // Over the annotation layer the gaze makes afresh (`paint_gaze` clears it).
        self.paint_marks();
        self.paint_overlay();
        self.grain_at = self.marked_region(|n| n.grain);
        self.bloom_at = self.marked_region(|n| n.bloom);
        if !self.painting { self.perf.pending.scene += (crate::perf::now_ms() - t0) as f32; }
    }

    /// Which labels marked `El::show_clear` show, and how much: by view, by rank, each shown that is inside
    /// the view by its inset and clear of those shown before it, with 4 px more to spare to come back;
    /// each opacity goes to its target linearly over 150 ms from where it is.
    fn show_clear(&mut self) {
        const FADE: f32 = 0.15;
        const MOST: usize = 64;
        let now = self.clock;
        self.clearing = false;
        self.clear_many.clear();
        if self.nodes.iter().all(|n| n.show_clear.is_none()) { self.clears.clear(); return; }
        // Each label's view: its nearest view scale above it, or the root.
        let mut views: Vec<(usize, Vec<usize>)> = vec![];
        for i in 0..self.nodes.len() {
            if self.nodes[i].show_clear.is_none() { continue; }
            let mut v = self.nodes[i].parent;
            while let Some(p) = v { if self.nodes[p].view_scale.is_some() { break; } v = self.nodes[p].parent; }
            let v = v.unwrap_or(0);
            match views.iter_mut().find(|x| x.0 == v) { Some(x) => x.1.push(i), None => views.push((v, vec![i])) }
        }
        let mut seen = HashSet::default();
        let mut ops: Vec<(usize, f32)> = vec![];
        for (v, mut labels) in views {
            if labels.len() > MOST { self.clear_many.push((self.nodes[v].key.clone(), labels.len())); }
            // By rank, then in the order of the view (a few, so in place).
            let rank = |i: usize| self.nodes[i].show_clear.map_or(0, |c| c.1);
            for a in 1..labels.len() { let mut b = a; while b > 0 && rank(labels[b - 1]) > rank(labels[b]) { labels.swap(b - 1, b); b -= 1; } }
            let a = self.nodes[v].rect.intersect(&self.nodes[v].clip);
            let mut shown: Vec<Rect> = vec![];
            for &i in labels.iter().take(MOST) {
                let (n, (inset, _)) = (&self.nodes[i], self.nodes[i].show_clear.expect("a label"));
                let prev = self.clears.get(&n.key).map(|&(op, at)| if at >= 0.0 { (op, true, at) } else { (op, false, -1.0 - at) });
                let (ins, gap) = if prev.is_none_or(|p| p.1) { (inset, 2.0) } else { (inset + 4.0, 6.0) };
                let r = n.rect;
                let inside = !n.hidden && r.x >= a.x + ins && r.y >= a.y + ins && r.right() <= a.right() - ins && r.bottom() <= a.bottom() - ins;
                let apart = |s: &Rect| r.x >= s.right() + gap || s.x >= r.right() + gap || r.y >= s.bottom() + gap || s.y >= r.bottom() + gap;
                let show = inside && shown.iter().all(apart);
                if show { shown.push(r); }
                let to = if show { 1.0 } else { 0.0 };
                let op = match prev {
                    None => to,
                    // From when it last moved towards this target (a target that just changed starts now).
                    Some((op, was, t)) => { let step = if was == show { (now - t).max(0.0) / FADE } else { 0.0 }; if op < to { (op + step).min(to) } else { (op - step).max(to) } }
                };
                if op != to { self.clearing = true; }
                self.clears.insert(n.key.clone(), (op, if show { now } else { -1.0 - now }));
                seen.insert(n.key.clone());
                ops.push((i, op));
            }
            // Past the most a view checks: shown as they are.
            for &i in labels.iter().skip(MOST) { ops.push((i, 1.0)); }
        }
        self.clears.retain(|k, _| seen.contains(k));
        for (i, op) in ops {
            self.nodes[i].opacity = self.nodes[i].op0.0 * op;
            let mut j = i;
            while j < self.nodes.len() && (j == i || self.within(j, i)) { self.nodes[j].opacity_all = self.nodes[j].op0.1 * op; j += 1; }
        }
        if self.clearing { self.dirty = true; }
    }
    /// How much a label marked `El::show_clear` is shown now (0–1), or `None` for one that is not.
    pub fn clear_opacity(&self, key: &str) -> Option<f32> { self.clears.get(key).map(|c| c.0) }

    /// The box round what the elements `mark` picks (`El::grain`, `El::bloom`) draw now (device px, each
    /// instance within its clip; a flowing picture where it is this frame), or `None` when none is marked.
    fn marked_region(&self, mark: fn(&Node<M>) -> bool) -> Option<[f32; 4]> {
        let marked: Vec<usize> = (0..self.nodes.len()).filter(|&i| mark(&self.nodes[i]) && !self.nodes[i].hidden).collect();
        if marked.is_empty() { return None; }
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut add = |r: [f32; 4]| if r[2] > r[0] && r[3] > r[1] { b = [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]; };
        let s = self.scale;
        for &i in &marked {
            let mut any = false;
            for j in 0..self.page_len.min(self.scene.instances.len()) {
                if !self.within(self.owners[j].0, i) { continue; }
                let q = &self.scene.instances[j];
                any = true;
                add([q.rect[0].max(q.clip[0]), q.rect[1].max(q.clip[1]), (q.rect[0] + q.rect[2]).min(q.clip[2]), (q.rect[1] + q.rect[3]).min(q.clip[3])]);
            }
            if !any { let n = &self.nodes[i]; let r = n.rect.intersect(&n.clip); add([r.x * s, r.y * s, r.right() * s, r.bottom() * s]); }
        }
        Some(if b[2] > b[0] { b } else { [0.0; 4] })
    }

    /// The performance readout (DESIGN.md §1.12), drawn by the annotation layer in Departure Mono at the
    /// window's bottom right, in the slate's manner: a hairline plate, small capitals, one accent. Mode 1
    /// is one line; mode 2 every number, with the last 120 frames as a sparkline split by part. It costs
    /// a few hundred glyph instances, changes only when a frame is drawn anyway, and once more a second
    /// after the last frame to say the page is idle; it never keeps frames coming.
    fn paint_overlay(&mut self) {
        self.overlay = self.timing.overlay;
        self.overlay_box = None;
        if self.overlay == 0 { return; }
        self.overlay_at = self.clock;
        let p = &self.perf;
        let now_ms = crate::perf::now_ms();
        let sm = p.summary(now_ms);
        let idle = p.frames.back().is_none_or(|(t, _)| now_ms - t > 1000.0);
        self.hud_idle = idle;
        let inst_fps = p.interval().map_or(0.0, |d| 1000.0 / d.max(0.1));
        // Every field keeps its width whatever its number (idle is as wide as the rates), so the plate,
        // anchored at its corner, does not move as the page goes idle and wakes.
        let fps = if idle { format!("{:<14}", "idle") } else { format!("{:>3.0} fps  {:>3}/s", inst_fps.min(999.0), sm.fps.min(999)) };
        let parts = self.run.as_ref().map_or(0, |r| r.grid.map_or(r.particles.len() as u32, |g| g.count()));
        let path = if parts == 0 { "-" } else if p.particle_path.is_empty() { "cpu" } else { p.particle_path };
        // Instances by kind.
        let mut kinds = [0usize; 4];
        for i in &self.scene.instances { let k = i.paint[3]; let k = if k < 0.5 { 0 } else if k < 1.5 { 1 } else if k < 2.5 { 2 } else { 3 }; kinds[k] += 1; }
        let fronts = self.field.fronts.iter().filter(|f| f[3] > 0.0 && self.clock - f[2] < self.timing.wave_life).count();
        let mat = crate::style::Material::from_id(self.timing.material).name();
        let backend = if p.backend.is_empty() { "?" } else { &p.backend };
        let last = p.frames.back().map(|f| f.1).unwrap_or_default();
        let lines: Vec<String> = if self.overlay == 1 {
            vec![format!("{fps}  {:>5.1} ms  dirty {:>3.0}%  {:>5} inst  budget {:>3.0}%  {mat:<8}  {backend}", sm.total[0], p.dirty * 100.0, self.scene.instances.len(), crate::budget::usage().scale * 100.0)]
        } else {
            let row = |name: &str, v: [f32; 3]| format!("{name:<13}{:>7.2}{:>7.2}{:>7.2}", v[0], v[1], v[2]);
            vec![
                format!("PERFORMANCE   {fps}"),
                format!("frames        drawn {}  idle-skipped {}  dropped {}", p.drawn, p.skipped, p.dropped),
                format!("{:<13}{:>7}{:>7}{:>7}", "ms", "last", "avg", "max"),
                row("update+layout", sm.update_layout),
                format!("{:<13}{:>7.2}", " of it text", last.text),
                row("scene", sm.scene),
                row("gpu submit", sm.submit),
                row("total", sm.total),
                format!("dirty         {:>5.1}% of {}x{}  passes {}", p.dirty * 100.0, (self.size.0 * self.scale) as u32, (self.size.1 * self.scale) as u32, p.passes),
                format!("instances     glyph {}  shape {}  picture {}  mesh {}", kinds[0], kinds[1], kinds[2], kinds[3]),
                format!("particles     {parts}  {path}"),
                { let b = crate::budget::usage(); format!("budget        {} / {} marks  asked {}  scale {:.2}", b.used, b.max, b.asked, b.scale) },
                format!("tree          {} nodes  changed {}  layouts {}", self.nodes.len(), self.rebuilt, p.counters.layouts),
                format!("text          {} cached  {} shaped  atlas {:.0}%", self.text.cached(), self.text.shaped(), self.text.atlas_use() * 100.0),
                format!("textures      ~{:.0} MB  fronts {fronts}", p.tex_mb),
                format!("field map     {} of {}x{} nodes not zero  level {:.3}", self.fmap.nonzero(), self.fmap.cols, self.fmap.rows, self.fmap.level()),
                format!("path          {}", p.path.line()),
                format!("look          {mat}  quality {}{}{}", p.quality, if p.auto { " auto" } else { "" }, p.stage_label().map_or(String::new(), |l| format!("  {l}"))),
                format!("device        {backend}  {}", if p.adapter.is_empty() { "-" } else { &p.adapter }),
                format!("surface       {:.0}x{:.0} @{}  {}", self.size.0, self.size.1, self.scale, crate::i18n::current().tag()),
            ]
        };
        let look = self.look_of.map(|f| f(&self.state)).unwrap_or_default();
        let ink = [look.theme.ink[0], look.theme.ink[1], look.theme.ink[2], 0.0];
        const SIZE: f32 = 11.0;
        const LINE: f32 = 14.0;
        let s = self.scale;
        let [ix, iy] = look.overlay_inset;
        // In a narrow window the lines wrap where their fields part (two spaces), the rest indented,
        // and a field wider than the window still breaks between characters: the box never overflows.
        // Its room is from the inset to 12 px from the window's other side (the inset is one side's).
        let room = (self.size.0 - ix - 12.0 - 16.0).max(40.0);
        let lines = {
            let text = &mut self.text;
            let mut fit = |l: &str| text.measure(l, MONO, SIZE, LINE, None).0 <= room;
            let mut out = vec![];
            for l in lines {
                if fit(&l) { out.push(l); continue; }
                let mut cur = String::new();
                for part in l.split("  ").map(str::trim).filter(|p| !p.is_empty()) {
                    let next = if cur.is_empty() { part.to_owned() } else { format!("{cur}  {part}") };
                    if fit(&next) || cur.is_empty() { cur = next; continue; }
                    out.push(std::mem::take(&mut cur));
                    cur = format!("  {part}");
                }
                // A field alone wider than the window.
                while !fit(&cur) && cur.chars().count() > 1 {
                    let mut n = cur.chars().count() - 1;
                    while n > 1 && !fit(&cur.chars().take(n).collect::<String>()) { n -= 1; }
                    let head: String = cur.chars().take(n).collect();
                    out.push(head);
                    cur = format!("  {}", cur.chars().skip(n).collect::<String>());
                }
                if !cur.trim().is_empty() { out.push(cur); }
            }
            out
        };
        let spark_h = if self.overlay == 2 { 34.0 } else { 0.0 };
        let width = lines.iter().map(|l| self.text.measure(l, MONO, SIZE, LINE, None).0).fold(0.0, f32::max).max(if self.overlay == 2 { 240.0f32.min(room) } else { 0.0 });
        let kept = self.overlay_w;
        let width = if kept.0 == self.overlay && kept.1 == self.size { width.max(kept.2).min(room) } else { width };
        self.overlay_w = (self.overlay, self.size, width);
        let h = LINE * lines.len() as f32 + spark_h + if spark_h > 0.0 { 8.0 } else { 0.0 };
        // The box in the look's corner, `overlay_inset` from the edges; where it would cover text to read
        // it moves on clockwise to the next corner that is clear (the asked one when none is, or when
        // `overlay_keep` says to stay).
        let (pw, ph) = (width + 16.0, h + 12.0);
        let at = |c: crate::style::Corner| {
            use crate::style::Corner::*;
            let x = match c { TopLeft | BottomLeft => ix, TopRight | BottomRight => self.size.0 - ix - pw };
            let y = match c { TopLeft | TopRight => iy, BottomLeft | BottomRight => self.size.1 - iy - ph };
            Rect::new(x.round(), y.round(), pw, ph)
        };
        let read = self.legible_ink();
        let clear = |r: Rect| !read.iter().any(|b| b[0] < (r.right()) * s && b[2] > r.x * s && b[1] < r.bottom() * s && b[3] > r.y * s);
        let mut corner = look.overlay_at;
        for _ in 0..4 { if look.overlay_keep || clear(at(corner)) { break; } corner = corner.clockwise(); }
        if !clear(at(corner)) { corner = look.overlay_at; }
        let plate = at(corner);
        self.overlay_box = Some(plate);
        let (x, y) = (plate.x + 8.0, plate.y + 6.0);
        let clip = [0.0, 0.0, self.size.0 * s, self.size.1 * s];
        let r = |r: Rect| [(r.x * s).round(), (r.y * s).round(), (r.w * s).round().max(1.0), (r.h * s).round().max(1.0)];
        // The plate: the ground nearly opaque, a hairline round it.
        self.scene.annot.push(Instance { rect: r(plate), paint: [0.0, 0.0, 0.94, SHAPE], clip, look: [look.theme.ground[0], look.theme.ground[1], look.theme.ground[2], 0.0], ..Default::default() });
        self.scene.annot.push(Instance { rect: r(plate), uv: [0.0; 4], paint: [1.0, 0.0, 0.35, SHAPE], clip, look: ink, extra: [1.0 * s, 0.0, 0.0, -1e4], ..Default::default() });
        for (k, l) in lines.iter().enumerate() {
            // The first line in the accent, as the slate's title is.
            let proto = Instance { paint: [0.0, if k == 0 { 1.0 } else { 0.0 }, if k == 0 { 1.0 } else { 0.85 }, 0.0], look: ink, ..Default::default() };
            let mut out = vec![];
            self.text.emit(Para { s: l, face: MONO, size: SIZE, line: LINE, width: 1e5, print: 0.0, around: None, track: 0.0 }, (x, y + k as f32 * LINE), proto, s, |mut i, _| { i.clip = clip; out.push(i); });
            self.scene.annot.extend(out);
        }
        if spark_h > 0.0 {
            // The last 120 frames, one bar each, its height the frame's cost (16.7 ms is the full height,
            // marked by a hairline), stacked: update and layout, scene, submit.
            let base = y + LINE * lines.len() as f32 + 8.0 + spark_h;
            let n = 120usize;
            let bw = width / n as f32;
            let frames: Vec<crate::perf::FrameCost> = self.perf.frames.iter().rev().take(n).map(|f| f.1).collect();
            let k = spark_h / crate::perf::PERIOD_MS as f32;
            self.scene.annot.push(Instance { rect: r(Rect::new(x, base - spark_h, width, 1.0)), paint: [1.0, 0.0, 0.25, SHAPE], clip, look: ink, ..Default::default() });
            for (j, f) in frames.iter().enumerate() {
                let bx = x + width - (j + 1) as f32 * bw;
                let mut top = base;
                for (v, accent, alpha) in [(f.update_layout, 0.0, 0.9), (f.scene, 0.0, 0.5), (f.submit, 1.0, 0.9)] {
                    let hh = (v * k).min(top - (base - spark_h * 1.5)).max(0.0);
                    if hh < 0.25 { continue; }
                    top -= hh;
                    self.scene.annot.push(Instance { rect: [((bx) * s).round(), (top * s).round(), (bw * s * 0.7).max(1.0).round(), (hh * s).max(1.0).round()], paint: [1.0, accent, alpha, SHAPE], clip, look: ink, ..Default::default() });
                }
            }
        }
    }

    /// The ink of each element of text to read and each control on the page (its instances' boxes, a
    /// glyph's without the spread round it, within its clip), device px `[x0, y0, x1, y1]`.
    fn legible_ink(&self) -> Vec<[f32; 4]> {
        let (s, spread) = (self.scale, crate::text::SPREAD as f32);
        let mut ink: HashMap<usize, [f32; 4]> = HashMap::default();
        for (i, inst) in self.scene.instances[..self.page_len.min(self.owners.len())].iter().enumerate() {
            let ni = self.owners[i].0;
            let n = &self.nodes[ni];
            if !n.legible || n.hidden || inst.paint[2] <= 0.0 { continue; }
            let r = inst.rect;
            let p = if inst.paint[3] == GLYPH { spread } else { 0.0 };
            let c = n.clip;
            let b = [(r[0] + p).max(c.x * s), (r[1] + p).max(c.y * s), (r[0] + r[2] - p).min(c.right() * s), (r[1] + r[3] - p).min(c.bottom() * s)];
            if b[2] <= b[0] || b[3] <= b[1] { continue; }
            let e = ink.entry(ni).or_insert(b);
            *e = [e[0].min(b[0]), e[1].min(b[1]), e[2].max(b[2]), e[3].max(b[3])];
        }
        ink.into_values().collect()
    }
    /// Where the performance readout (`Look::overlay`) was drawn in the last frame, logical px.
    pub fn overlay_box(&self) -> Option<Rect> { self.overlay_box }

    fn animate(&mut self) {
        self.scene.truncate(self.page_len);
        let now = self.clock;
        let active = self.run.is_some() || !self.flows.is_empty() || !self.recomps.is_empty() || !self.slides.is_empty() || self.decodes.values().any(|d| d.spec.hold.is_none());
        let hidden: HashSet<u64> = self.recomps.values().flat_map(|r| r.hide.iter().copied()).collect();
        // One more pass after the last animation ends puts every instance back as painted.
        // Parts in flight are drawn over the page, so they do not pass behind what is already there.
        let mut lifted = vec![];
        // Growing pictures (`Grow`): drawn as the page's last pictures, over what dissolves.
        let mut growing = vec![];
        // What is keyed the same on both pages of a growing subtree, over the picture.
        let mut keyed = vec![];
        let plain = self.flows.values().any(|f| f.src.is_some()) && self.look().material == crate::style::Material::Plain;
        if active || self.animating {
            for i in 0..self.page_len {
                let mut inst = self.base[i];
                let (node, id) = self.owners[i];
                if let Some(r) = &self.run {
                    match self.roles[node] {
                        1 => inst.extra[2] = inst.extra[2].max(ramp(r.src_ramp, now)),
                        2 => inst.extra[2] = inst.extra[2].max(ramp(r.dst_ramp, now)),
                        _ => {}
                    }
                }
                if let Some(f) = self.flows.get(&id) && let Some(kd) = &f.keyed && let Some(g) = self.grows.get(kd.grow) {
                    // Keyed the same on both pages: carried by the picture onto its new place.
                    let t = ((now - f.start) / f.dur.max(1e-4)).clamp(0.0, 1.0);
                    let e = g.ease(now);
                    let (o, n, pin) = (kd.old.rect, inst.rect, kd.pin);
                    let (po, pn) = ([o[0] + pin[0] * o[2], o[1] + pin[1] * o[3]], [n[0] + pin[0] * n[2], n[1] + pin[1] * n[3]]);
                    let (c, c1) = (g.ride([po[0], po[1], 0.0, 0.0], e), g.ride([po[0], po[1], 0.0, 0.0], 1.0));
                    let p = [c[0] + (pn[0] - c1[0]) * e, c[1] + (pn[1] - c1[1]) * e];
                    let (w, h) = (o[2] + (n[2] - o[2]) * e, if kd.keep_h { n[3] } else { o[3] + (n[3] - o[3]) * e });
                    inst.rect = [p[0] - pin[0] * w, p[1] - pin[1] * h, w, h];
                    for k in 0..3 { inst.paint[k] = kd.old.paint[k] + (inst.paint[k] - kd.old.paint[k]) * t; }
                    if t < 1.0 {
                        inst.clip = g.shows(e);
                        if self.scene.layer.is_none() { keyed.push((i, inst)); inst.rect = [0.0; 4]; }
                    }
                } else if let Some(f) = self.flows.get(&id) && let Some(g) = f.grow.and_then(|g| self.grows.get(g)) {
                    // A growing picture: in its box on whole pixels, only the part that shows.
                    let e = g.ease(now);
                    inst.rect = g.picture(e);
                    inst.clip = g.shows(e);
                    if now < g.start + g.dur && self.scene.layer.is_none() { growing.push((i, inst)); inst.rect = [0.0; 4]; }
                } else if let Some(f) = self.flows.get(&id) {
                    let p = ((now - f.start) / f.dur.max(1e-4)).clamp(0.0, 1.0);
                    // A glyph flying from the last frame eases in and out on a sine, which leaves its
                    // ends sooner than the cubic.
                    let e = if f.src.is_some() { 0.5 - 0.5 * (std::f32::consts::PI * p).cos() } else { Ease::InOutCubic.at(p) };
                    // Something that only fades in stays where it is laid out now (a new window size
                    // during the fade moves it with the page).
                    let still = f.fade_in.is_some() && f.src.is_none();
                    if !still && inst.paint[3] > 2.5 && (f.from[2] * inst.rect[3] - f.from[3] * inst.rect[2]).abs() > 0.5 {
                        // A 3D view between boxes of other proportions keeps its own (it is drawn into its
                        // layout box): it moves centre to centre and scales uniformly, never stretched
                        // (DESIGN.md §1.10).
                        let (w1, h1) = (inst.rect[2], inst.rect[3]);
                        let s0 = ((f.from[2] * f.from[3]) / (w1 * h1).max(1.0)).sqrt();
                        let sc = s0 + (1.0 - s0) * e;
                        let c0 = (f.from[0] + f.from[2] * 0.5, f.from[1] + f.from[3] * 0.5);
                        let c1 = (inst.rect[0] + w1 * 0.5, inst.rect[1] + h1 * 0.5);
                        let c = (c0.0 + (c1.0 - c0.0) * e, c0.1 + (c1.1 - c0.1) * e);
                        inst.rect = [c.0 - w1 * sc * 0.5, c.1 - h1 * sc * 0.5, w1 * sc, h1 * sc];
                    } else if !still {
                        for k in 0..4 { inst.rect[k] = f.from[k] + (inst.rect[k] - f.from[k]) * e; }
                    }
                    if let Some(o) = &f.src {
                        // The glyph's own box (without the spread) goes from the old one's to the new
                        // one's; drawn as the old glyph, then as the new, scaled to it.
                        let sp = crate::text::SPREAD as f32;
                        let own = |r: [f32; 4]| [r[0] + sp, r[1] + sp, (r[2] - 2.0 * sp).max(1e-3), (r[3] - 2.0 * sp).max(1e-3)];
                        let (gs, gd) = (own(o.rect), own(self.base[i].rect));
                        let g: [f32; 4] = std::array::from_fn(|k| gs[k] + (gd[k] - gs[k]) * e);
                        let (shape, at) = if e < 0.5 { (o, gs) } else { (&self.base[i], gd) };
                        let (fx, fy) = (g[2] / at[2], g[3] / at[3]);
                        inst.uv = shape.uv;
                        inst.extra[1] = shape.extra[1];
                        inst.rect = [g[0] - sp * fx, g[1] - sp * fy, g[2] + 2.0 * sp * fx, g[3] + 2.0 * sp * fy];
                        for k in 0..3 { inst.paint[k] = o.paint[k] + (self.base[i].paint[k] - o.paint[k]) * e; }
                        // In a material it comes apart into the material's marks on the way, most at
                        // halfway; plain, it stays a whole glyph.
                        if !plain { inst.extra[2] = inst.extra[2].max(0.6 * (std::f32::consts::PI * e).sin()); }
                    }
                    if let Some([a, b]) = f.fade_in { inst.paint[2] *= ((now - a) / (b - a).max(1e-4)).clamp(0.0, 1.0); }
                    // Its clip goes from the old one to the new: what was inside both is inside all the way.
                    if p < 1.0 && !still { for k in 0..4 { inst.clip[k] = f.clip[k] + (inst.clip[k] - f.clip[k]) * e; } }
                    inst.extra[2] = inst.extra[2].max(f.loose * (std::f32::consts::PI * e).sin());
                    if p < 1.0 && self.scene.layer.is_none() { lifted.push((i, inst)); inst.rect = [0.0; 4]; }
                }
                // Recomposition: the new glyphs wait under their tiles; unchanged ones slide.
                if hidden.contains(&id) { inst.paint[2] = 0.0; }
                if let Some(sl) = self.slides.get(&id) {
                    let e = Ease::InOutCubic.at(((now - sl.start) / sl.dur.max(1e-4)).clamp(0.0, 1.0));
                    for k in 0..4 { inst.rect[k] = sl.from[k] + (inst.rect[k] - sl.from[k]) * e; }
                    if e < 1.0 { inst.clip = [sl.clip[0].min(inst.clip[0]), sl.clip[1].min(inst.clip[1]), sl.clip[2].max(inst.clip[2]), sl.clip[3].max(inst.clip[3])]; }
                }
                self.scene.instances[i] = inst;
            }
        }
        if active || self.animating { self.dirty = true; }
        self.animating = active;
        if let Some((i, _, _)) = self.caret && i < self.page_len { self.scene.instances[i].paint[2] = if self.caret_on { self.base[i].paint[2] } else { 0.0 }; }
        self.ghosts.retain(|g| now < g.start + g.dur - 1e-4);
        // An old page's 3D elements are not kept: their meshes went with that page's scene.
        let mut riding = vec![];
        for g in self.ghosts.iter().filter(|g| g.inst.paint[3] != MESH) {
            let p = ((now - g.start) / g.dur.max(1e-4)).clamp(0.0, 1.0);
            let mut inst = g.inst;
            // Less motion: it only fades.
            if g.loosen && !self.fmap.reduced { inst.extra[2] = inst.extra[2].max(p); }
            inst.paint[2] *= 1.0 - p;
            // Riding with a growing picture, over it, inside the part of it that shows (drawn after it).
            if let Some(gr) = g.ride.and_then(|r| self.grows.get(r)) {
                let e = gr.ease(now);
                if g.scaled {
                    // The view's geometry: with the picture's transform, its pen kept.
                    let r = inst.rect;
                    let line = inst.paint[3] == SHAPE && r[2] > r[3] + 0.5 && (r[3] - 2.0 * inst.uv[0]).abs() < 0.01;
                    inst.rect = gr.carry(r, e, line);
                } else {
                    let p = [g.anchor[0] + g.pin[0] * g.anchor[2], g.anchor[1] + g.pin[1] * g.anchor[3]];
                    let a = gr.ride([p[0], p[1], 0.0, 0.0], e);
                    inst.rect[0] += a[0] - p[0];
                    inst.rect[1] += a[1] - p[1];
                }
                inst.clip = gr.shows(e);
                if self.scene.layer.is_none() { riding.push(inst); continue; }
            }
            self.scene.push(inst);
        }
        // The growing pictures and what rides on them are the page's, drawn last: over what dissolves,
        // and pictures as any other, so what rides on them is ink over a picture.
        self.lifted.clear();
        for (i, inst) in growing { self.lifted.insert(i, self.scene.instances.len()); self.scene.push(inst); }
        for inst in riding { self.scene.push(inst); }
        for (i, inst) in keyed { self.lifted.insert(i, self.scene.instances.len()); self.scene.push(inst); }
        for r in self.recomps.values() {
            for t in &r.tiles { for inst in tile_pieces(t, now - r.start, false) { self.scene.push(inst); } }
            if now < r.keep_until { for inst in &r.keep { self.scene.push(*inst); } }
        }
        // Text over pictures and shapes, as it is on any page.
        lifted.sort_by_key(|(_, inst)| std::cmp::Reverse(inst.paint[3] as u32));
        // In a layer of their own (only when there is no modal layer): where they have ink they replace
        // what is beneath in the near image, so a title flying over a picture is drawn as text.
        if !lifted.is_empty() { self.scene.begin_layer(); }
        for (i, inst) in lifted { self.lifted.insert(i, self.scene.instances.len()); self.scene.push(inst); }
        self.body_len = self.scene.instances.len();
    }

    // ── Morph and flow ───────────────────────────────────────────────────────────────────────────

    /// Morph element `from` into element `to` (keys; see DESIGN.md §1.10). Asking for the reverse of
    /// the morph in progress turns it around: in flight, every tile goes back from where it is; while
    /// the source is still loosening, it tightens again. Any other morph in progress is cut short.
    pub fn morph(&mut self, from: &str, to: &str, spec: MorphSpec) {
        let drawn = self.staged_drawn(from);
        self.morph_drawn(from, to, spec, drawn);
    }
    /// What the last frame drew of `key`'s subtree inside stages (`El::stage`), in order, with each
    /// instance's element: a stage's glyphs may be posed anew by every step (and the new view may hide
    /// them), and a morph from them starts where they were seen.
    fn staged_drawn(&self, key: &str) -> Vec<(String, Instance)> {
        let Some(root) = self.nodes.iter().position(|n| n.key == key) else { return vec![] };
        let staged = |mut j: usize| loop {
            if self.nodes[j].stage.is_some() { return true; }
            match self.nodes[j].parent { Some(p) => j = p, None => return false }
        };
        let n = self.page_len.min(self.owners.len()).min(self.scene.instances.len());
        (0..n).filter(|&i| within(&self.nodes, self.owners[i].0, root) && staged(self.owners[i].0)).map(|i| (self.nodes[self.owners[i].0].key.clone(), self.scene.instances[i])).collect()
    }
    /// `morph`, with what stages drew of `from` in the last frame (`staged_drawn`).
    fn morph_drawn(&mut self, from: &str, to: &str, spec: MorphSpec, drawn: Vec<(String, Instance)>) {
        if self.timing.off.morphs { return; }
        let now = self.clock;
        let tighten = spec.tighten_ms / 1000.0;
        let mut k0 = 0.0;
        if let Some(r) = &mut self.run {
            let reverse = r.from == to && r.to == from;
            // In flight, on either path, any number of times: every tile goes back from where it is.
            if reverse && let Some(clock) = r.clock(now) {
                let dur = spec.duration_ms / 1000.0 * (1.0 - spec.stagger.clamp(0.0, 0.9));
                morph::reverse(&mut r.particles, clock, r.spec.ease, dur);
                if let Some(g) = &mut r.grid {
                    g.turned = Some(match g.turned {
                        None => morph::Turn { first: clock, alpha: 1.0, beta: 0.0, to: 0.0, dur },
                        Some(t) => t.again(clock, r.spec.ease, dur),
                    });
                }
                std::mem::swap(&mut r.from, &mut r.to);
                r.spec.ease = spec.ease;
                r.flight = Some([now, now + dur]);
                r.src_until = now;
                r.dst_from = now + dur;
                r.dst_ramp = [1.0, now + dur, 0.0, now + dur + tighten];
                r.end = now + dur + tighten;
                r.upload = next_run();
                self.paint();
                return;
            }
            if reverse && now < r.src_until {
                let k = ramp(r.src_ramp, now);
                r.src_until = NEVER;
                r.src_ramp = [k, now, 0.0, now + tighten * k];
                r.flight = None;
                r.dst_from = NEVER;
                r.end = now + tighten * k;
                self.paint();
                return;
            }
            // Starting from where the morph in progress left the element.
            k0 = if r.to == from { if now >= r.dst_from { ramp(r.dst_ramp, now) } else { 1.0 } }
                 else if r.from == from && now < r.src_until { ramp(r.src_ramp, now) } else { 0.0 };
        }
        self.run = None;
        self.paint();
        let (src, dst) = (self.collect(from, &drawn), self.collect(to, &[]));
        self.morph_with(from, to, src, dst, spec, k0);
    }

    /// Morph into `to` from what the frame drawn last showed for `spec.from_last` (or `to`'s own key):
    /// call it, then change the view (a route that replaces the page). The source is not looked up in
    /// the tree: it is the instances that last frame drew under that key, kept until the morph is done,
    /// so a title on the old page flies, glyph by glyph, to the title of the new one (`morph_last`).
    /// With nothing drawn under that key, the new element just comes in as usual. `Cmd::Morph` with `from_last` set does the same.
    pub fn morph_from_last(&mut self, to: &str, spec: MorphSpec) {
        if self.timing.off.morphs { return; }
        let from = spec.from_last.clone().unwrap_or_else(|| to.to_owned());
        let src = self.instances_of(&from).into_iter().filter(|i| i.paint[2] > 0.0 && i.rect[2] > 0.0).collect();
        self.last_morph = Some((src, to.to_owned(), spec));
    }
    /// Start a morph kept by `morph_from_last`, once the new view is laid out.
    /// The old title's glyphs fly to the new one's, glyph by glyph in order, each whole (DESIGN.md
    /// §1.10): straight, eased in and out on a sine, its size and colour going over, in a material loosened
    /// by `0.6 × sin(π × p)` on the way; each 12 ms after the last (in all at most 15 % of the duration), in
    /// the word's order from the end that faces the way it goes (the way projected onto the text's direction;
    /// under 10 % of the way, as going straight down, there is no stagger and all go together), so the last lands at the duration. Glyphs one side has
    /// more of fade where they are: the old over the first 30 %, the new over the last 30 %. With less
    /// motion nothing flies: the old fades out where it was and the new in where it is, 150 ms each.
    fn morph_last(&mut self) {
        let Some((src, to, spec)) = self.last_morph.take() else { return };
        if src.is_empty() { return; }
        let Some(root) = self.nodes.iter().position(|n| n.key == to) else { return };
        let now = self.clock;
        let dst: Vec<usize> = (0..self.page_len).filter(|&i| within(&self.nodes, self.owners[i].0, root)).collect();
        let reduced = self.fmap.reduced;
        let dur = if reduced { self.fade_s } else { (spec.duration_ms / 1000.0).max(1e-3) };
        let glyph = |i: &Instance| i.paint[3] == GLYPH;
        let (sg, so): (Vec<Instance>, Vec<Instance>) = src.into_iter().partition(glyph);
        let (dg, d_other): (Vec<usize>, Vec<usize>) = dst.into_iter().partition(|&i| glyph(&self.base[i]));
        let n = if reduced { 0 } else { sg.len().min(dg.len()) };
        // In the word's order, from the end that faces the way it goes: the way projected onto the
        // word's own direction (first glyph to last). Going on along it, the last glyph leaves first;
        // going back, the first does; so the glyphs open out a little on the way and never run into
        // the ones ahead. Where that projection is under 10 % of the way (near square to the text,
        // as straight down a page), no end is in front: all glyphs go together.
        let centre = |r: [f32; 4]| [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5];
        let mean = |c: &mut dyn Iterator<Item = [f32; 2]>| { let (mut s, mut m) = ([0.0f32; 2], 0.0f32); for p in c { s[0] += p[0]; s[1] += p[1]; m += 1.0; } [s[0] / m.max(1.0), s[1] / m.max(1.0)] };
        let (a, b) = (mean(&mut sg[..n].iter().map(|i| centre(i.rect))), mean(&mut dg[..n].iter().map(|&i| centre(self.base[i].rect))));
        let way = [b[0] - a[0], b[1] - a[1]];
        let along = if n > 1 {
            let (f, l) = (centre(sg[0].rect), centre(sg[n - 1].rect));
            let td = [l[0] - f[0], l[1] - f[1]];
            (way[0] * td[0] + way[1] * td[1]) / td[0].hypot(td[1]).max(1e-6)
        } else { 0.0 };
        let staggered = n > 1 && along.abs() >= 0.1 * way[0].hypot(way[1]);
        let front_first = along > 0.0;
        let per = if staggered { 0.012f32.min(0.15 * dur / (n - 1) as f32) } else { 0.0 };
        let travel = dur - per * n.saturating_sub(1) as f32;
        for k in 0..n {
            let i = dg[k];
            let id = self.owners[i].1;
            let order = if front_first { n - 1 - k } else { k };
            self.flows.insert(id, FlowRec { from: sg[k].rect, clip: sg[k].clip, start: now + per * order as f32, dur: travel, loose: 0.0, src: Some(sg[k]), fade_in: None, grow: None, keyed: None });
            // It is the old glyph come over: born when that was.
            self.births.insert(id, sg[k].extra[3]);
            self.base[i].extra[3] = sg[k].extra[3];
        }
        let (out, inn) = if reduced { (dur, [now, now + dur]) } else { (0.3 * dur, [now + 0.7 * dur, now + dur]) };
        for inst in sg[n..].iter().chain(&so) { self.ghosts.push(Ghost { inst: *inst, start: now, dur: out, loosen: false, ride: None, anchor: inst.rect, pin: [0.5, 0.5], scaled: false }); }
        for &i in dg[n..].iter().chain(&d_other) {
            let id = self.owners[i].1;
            self.flows.insert(id, FlowRec { from: self.base[i].rect, clip: self.base[i].clip, start: now, dur, loose: 0.0, src: None, fade_in: Some(inn), grow: None, keyed: None });
            // It fades in, and does not come in as a new element too.
            self.births.insert(id, -1e4);
            self.base[i].extra[3] = -1e4;
        }
        // The rest of the new page is in place from the first frame: it does not come in.
        for i in 0..self.page_len {
            if within(&self.nodes, self.owners[i].0, root) { continue; }
            self.births.insert(self.owners[i].1, -1e4);
            self.base[i].extra[3] = -1e4;
        }
        self.animating = true;
        self.finish();
    }
    fn morph_with(&mut self, from: &str, to: &str, src: Vec<Instance>, dst: Vec<Instance>, spec: MorphSpec, k0: f32) {
        let now = self.clock;
        let tighten = spec.tighten_ms / 1000.0;
        if src.is_empty() || dst.is_empty() { return; }
        let pitch = (spec.lattice * self.scale).max(1.5);
        // The tiles come out of the frame budget: its scale, and on the CPU path at most its count
        // (DESIGN.md §1.27). A smaller count only makes the tiles larger.
        let b = crate::budget::usage();
        let spec = MorphSpec { count_max: (spec.count_max * b.scale).max(1.0), ..spec.clone() };
        let cpu_cap = b.max as f32;
        // 3D elements emit from their surface (on the CPU path; the GPU's grid samples what the screen
        // shows); the rest of each side from the screen.
        let (es, ed) = (self.emitters(from), self.emitters(to));
        let meshy = !es.is_empty() || !ed.is_empty();
        // Many tiles: the GPU simulates a grid (when the renderer can), the CPU builds nothing.
        let above = self.look().particles.gpu_above;
        let grid = match (morph::ink_bounds(&src), morph::ink_bounds(&dst)) {
            (Some(a), Some(b)) if self.particle_gpu && morph::Grid::estimate(a, b, pitch) > above => Some(morph::Grid::new(a, b, (spec.lattice * self.scale).max(1.0), spec.count_max.max(above))),
            _ => None,
        };
        let mut emit = vec![];
        let tiles = if let Some(g) = grid { morph::Tiles { particles: vec![], tile: g.a[2], tags: vec![] } }
        else if meshy {
            let (lib, atlas, cap) = (&self.meshes, &self.text.pixels, spec.count_max.min(cpu_cap));
            let t = morph::tiles_with(mesh_side(&src, &es, 0, lib, atlas, cap), mesh_side(&dst, &ed, es.len() as u32, lib, atlas, cap), morph::ink_bounds(&src), morph::ink_bounds(&dst), pitch, &spec);
            for (key, sc, r) in es.iter().chain(&ed) { emit.push(Emitter { key: key.clone(), points: sc.surface(lib, surface_count(r, t.tile, cap), 0x51ab) }); }
            t
        } else { morph::tiles(&src, &dst, pitch, &MorphSpec { count_max: spec.count_max.min(cpu_cap), ..spec.clone() }, &self.text.pixels) };
        let (loosen, flight) = (spec.loosen_ms / 1000.0 * (1.0 - k0), spec.duration_ms / 1000.0);
        let (t1, t2) = (now + loosen, now + loosen + flight);
        let id = next_run();
        self.run = Some(MorphRun {
            id, upload: id, from: from.into(), to: to.into(), spec,
            src_until: t1, src_ramp: [k0, now, 1.0, t1], flight: Some([t1, t2]),
            dst_from: t2, dst_ramp: [1.0, t2, 0.0, t2 + tighten], end: t2 + tighten,
            particles: tiles.particles, grid, tile: tiles.tile, src, dst, emit, tags: tiles.tags, scrub: None,
        });
        self.reproject();
        self.paint();
    }

    /// The `scene3d` elements in the subtree `key`: their keys, scenes and device rects.
    fn emitters(&self, key: &str) -> Vec<(String, Rc<crate::mesh::Scene3d>, [f32; 4])> {
        let Some(root) = self.nodes.iter().position(|n| n.key == key) else { return vec![] };
        let s = self.scale;
        (0..self.nodes.len()).filter(|&i| within(&self.nodes, i, root)).filter_map(|i| match &self.nodes[i].content {
            Content::Scene3d(sc) => { let r = self.nodes[i].inner; Some((self.nodes[i].key.clone(), sc.clone(), [r.x * s, r.y * s, r.w * s, r.h * s])) }
            _ => None,
        }).collect()
    }

    /// A morph with 3D elements: every tile that starts or lands on a mesh surface point goes where that
    /// point is now (the object may be turning), and is hidden while the point is behind the mesh.
    fn reproject(&mut self) {
        let s = self.scale;
        let (nodes, lib) = (&self.nodes, &self.meshes);
        let Some(r) = self.run.as_mut() else { return };
        if r.emit.is_empty() { return; }
        let scenes: Vec<Option<(Rc<crate::mesh::Scene3d>, [f32; 4])>> = r.emit.iter().map(|e| nodes.iter().find(|n| n.key == e.key).and_then(|n| match &n.content {
            Content::Scene3d(sc) => Some((sc.clone(), [n.inner.x * s, n.inner.y * s, n.inner.w * s, n.inner.h * s])),
            _ => None,
        })).collect();
        let at = |tag: u32| -> Option<(f32, f32, bool)> {
            if tag == morph::NO_TAG { return None; }
            let (e, k) = ((tag >> 24) as usize, (tag & 0x00ff_ffff) as usize);
            let (sc, rect) = scenes.get(e)?.as_ref()?;
            let sp = r.emit[e].points.get(k)?;
            let (fx, fy, seen) = sc.project(lib, sp, rect[2] / rect[3].max(1.0))?;
            let t = r.tile;
            Some((rect[0] + fx * rect[2] - t * 0.5, rect[1] + fy * rect[3] - t * 0.5, seen))
        };
        let mut changed = false;
        for (p, tag) in r.particles.iter_mut().zip(&r.tags) {
            let mut code = morph::kind::base(p.misc[0]);
            if let Some((x, y, seen)) = at(tag[0]) {
                changed |= p.ab[0] != x || p.ab[1] != y;
                p.ab[0] = x; p.ab[1] = y;
                if !seen { code += morph::kind::SRC_HIDDEN; }
                // A tile that stays where it is stays on its point.
                if tag[1] == morph::NO_TAG && code as u32 & 7 >= 3 { p.ab[2] = x; p.ab[3] = y; }
            }
            if let Some((x, y, seen)) = at(tag[1]) {
                changed |= p.ab[2] != x || p.ab[3] != y;
                p.ab[2] = x; p.ab[3] = y;
                if !seen { code += morph::kind::DST_HIDDEN; }
                if tag[0] == morph::NO_TAG && code as u32 & 7 >= 3 { p.ab[0] = x; p.ab[1] = y; }
            }
            changed |= p.misc[0] != code;
            p.misc[0] = code;
        }
        if changed { r.upload = next_run(); }
    }

    /// Everything a node and its descendants draw, drawn or hidden, at looseness 1 and fully born. An
    /// element `drawn` has (a stage's content as the last frame drew it) is taken as it was drawn, in
    /// place of what this paint has of it.
    fn collect(&self, key: &str, drawn: &[(String, Instance)]) -> Vec<Instance> {
        let Some(root) = self.nodes.iter().position(|n| n.key == key) else { return vec![] };
        let mut left: Vec<&str> = vec![];
        for (k, _) in drawn { if !left.contains(&k.as_str()) { left.push(k); } }
        let mut out = vec![];
        // An element of `drawn`: all it drew, at its first instance here.
        let mut take = |out: &mut Vec<Instance>, node: usize, inst: Instance| {
            let k = self.nodes[node].key.as_str();
            match left.iter().position(|l| *l == k) {
                Some(at) => { left.remove(at); out.extend(drawn.iter().filter(|(d, _)| d == k).map(|(_, i)| *i)); }
                None => if !drawn.iter().any(|(d, _)| d == k) { out.push(inst) },
            }
        };
        for i in (0..self.page_len).filter(|&i| within(&self.nodes, self.owners[i].0, root)) { take(&mut out, self.owners[i].0, self.base[i]); }
        for &(n, inst) in self.unseen.iter().filter(|(n, _)| within(&self.nodes, *n, root)) { take(&mut out, n, inst); }
        // What is no longer here at all.
        for k in left { out.extend(drawn.iter().filter(|(d, _)| d == k).map(|(_, i)| *i)); }
        out.into_iter().map(|mut i| { i.extra[2] = 1.0; i.extra[3] = -1e4; i }).collect()
    }

    /// The page as it is drawn now, for a flow.
    fn record(&self) -> Vec<Old> {
        let slots = self.slots();
        // The nearest view scale above a node, own-size elements on the way included.
        let view = |mut v: usize| -> Option<usize> {
            let mut own = false;
            loop {
                own |= self.nodes[v].own_size.is_some();
                let p = self.nodes[v].parent?;
                if self.nodes[p].view_scale.is_some() { return (!own).then_some(p); }
                v = p;
            }
        };
        (0..self.page_len).map(|i| {
            let n = &self.nodes[self.owners[i].0];
            let own = n.own_size.is_some();
            // A drawing's marks are its geometry wherever it is; under a view scale, everything is.
            let scaled = !own && (matches!(n.content, Content::Canvas(_)) || view(self.owners[i].0).is_some());
            Old { id: self.owners[i].1, node: self.owners[i].0, pin: n.own_size.map_or([0.5, 0.5], |o| o.0), tag: self.tags[i].clone(), inst: self.scene.instances[i], key: n.keyed.then(|| Rc::from(n.key.as_str())), slot: slots[i], own, scaled }
        }).collect()
    }
    /// Which of its element's instances each page instance is: glyph or not, how many of that kind came
    /// before it in the element, and the glyph.
    fn slots(&self) -> Vec<(bool, u32, u32)> {
        // By node: how many glyphs and how many other instances so far.
        let mut count = vec![[0u32; 2]; self.nodes.len()];
        (0..self.page_len).map(|i| {
            let glyph = self.base[i].paint[3] == GLYPH;
            let c = &mut count[self.owners[i].0][glyph as usize];
            *c += 1;
            (glyph, *c - 1, self.tags[i].as_ref().map_or(0, |t| t.gid))
        }).collect()
    }

    /// After a relayout for a new page: instances of a flow subtree that left one place and appeared in
    /// another fly between them and keep their birth time; what is gone dissolves. A subtree with a
    /// picture on both pages grows (`Grow`): its first picture goes from the old box to the new one, shown
    /// only within the part that shows going from the old one's to the new one's; the rest of the old
    /// subtree rides with it at its own size and fades, the rest of the new one and everything new
    /// outside the flows comes in once it has landed, and the old page's ground stays around it.
    fn flow_from(&mut self, old: Vec<Old>, spec: FlowSpec) {
        let now = self.clock;
        let flight = spec.flow_ms / 1000.0;
        let new_ids: HashSet<u64> = self.owners.iter().map(|o| o.1).collect();
        let old_ids: HashSet<u64> = old.iter().map(|o| o.id).collect();
        let mut used = vec![false; old.len()];
        let shows = |r: [f32; 4], c: [f32; 4]| [r[0].max(c[0]), r[1].max(c[1]), (r[0] + r[2]).min(c[2]), (r[1] + r[3]).min(c[3])];
        // The growing subtrees: by flow, the first picture of each on the old page and on the new.
        let mut grow_of: HashMap<Rc<str>, usize> = HashMap::default();
        if flight > 0.0 {
            let mut first_old: HashMap<Rc<str>, usize> = HashMap::default();
            for (k, o) in old.iter().enumerate() {
                // A picture, not the cells that read one (`el::cells`, a picture instance with its ramp in extra.x).
                // (One keyed the same on both pages is the same picture, and grows when its box changes.)
                if let Some(t) = &o.tag && o.inst.paint[3] == PICTURE && o.inst.extra[0] <= 0.5 { first_old.entry(t.flow.clone()).or_insert(k); }
            }
            for i in 0..self.page_len {
                let id = self.owners[i].1;
                let Some(t) = self.tags[i].clone() else { continue };
                if self.base[i].paint[3] != PICTURE || self.base[i].extra[0] > 0.5 || grow_of.contains_key(&t.flow) { continue; }
                let Some(&k) = first_old.get(&t.flow) else { continue };
                if old[k].id == id && (0..4).all(|j| (old[k].inst.rect[j] - self.base[i].rect[j]).abs() < 0.5) { continue; }
                let (o, n) = (old[k].inst, self.base[i]);
                let g = self.grows.len();
                self.grows.push(Grow { p0: o.rect, p1: n.rect, r0: shows(o.rect, o.clip), r1: shows(n.rect, n.clip), start: now, dur: flight });
                grow_of.insert(t.flow.clone(), g);
                used[k] = true;
                self.flows.insert(id, FlowRec { from: o.rect, clip: o.clip, start: now, dur: flight, loose: 0.0, src: None, fade_in: None, grow: Some(g), keyed: None });
                self.births.insert(id, o.extra[3]);
                self.base[i].extra[3] = o.extra[3];
            }
        }
        let grows = |t: &Option<Tag>| t.as_ref().is_some_and(|t| grow_of.contains_key(&t.flow));
        // Elements keyed the same on both pages of a subtree that grows: each instance goes from where it
        // was to where it is with the picture (`Keyed`). Own-size type of another size on the new page is
        // two layouts, not one moving: it is told (`ViewProblem::FlowOwnSize`) and the two cross-fade
        // once the picture lands.
        let out = if self.fmap.reduced { self.fade_s } else { spec.out_ms / 1000.0 };
        let mut xfade = vec![false; old.len()];
        self.flow_own.clear();
        if !grow_of.is_empty() {
            // The old page's keyed instances in growing subtrees, by key.
            let mut by_key: HashMap<String, Vec<usize>> = HashMap::default();
            for (k, o) in old.iter().enumerate() {
                if let Some(key) = o.key.as_deref() && !used[k] && grows(&o.tag) { by_key.entry(key.to_owned()).or_default().push(k); }
            }
            let slots = self.slots();
            for (i, &slot) in slots.iter().enumerate() {
                let (node, id) = self.owners[i];
                // (Kept on both pages, its identity is the same: it is found by its key all the same.)
                if self.flows.contains_key(&id) || !self.nodes[node].keyed { continue; }
                // In a growing subtree on the new page, it is carried by that picture; outside every
                // flow (a layer over the page's frame), by the one it rode on the old page.
                let new_g = match &self.tags[i] { Some(t) => match grow_of.get(&t.flow) { Some(&g) => Some(g), None => continue }, None => None };
                let old_g = |k: usize| old[k].tag.as_ref().and_then(|t| grow_of.get(&t.flow)).copied();
                let Some(&k) = by_key.get(self.nodes[node].key.as_str()).and_then(|ks| ks.iter().find(|&&k| match new_g {
                    Some(g) => old[k].slot == slot && old_g(k) == Some(g),
                    // (Outside the flows a glyph has no glyph id in its slot: by its place in the text.)
                    None => old[k].slot.0 == slot.0 && old[k].slot.1 == slot.1,
                })) else { continue };
                let Some(g) = new_g.or_else(|| old_g(k)) else { continue };
                let (o, n) = (old[k].inst, self.base[i]);
                let own = old[k].own || self.nodes[node].own_size.is_some();
                if own && n.paint[3] == GLYPH && ((o.rect[2] - n.rect[2]).abs() > 0.5 || (o.rect[3] - n.rect[3]).abs() > 0.5) {
                    if !self.flow_own.contains(&self.nodes[node].key) { self.flow_own.push(self.nodes[node].key.clone()); }
                    xfade[k] = true;
                    self.flows.insert(id, FlowRec { from: n.rect, clip: n.clip, start: now, dur: flight + out.max(1e-3), loose: 0.0, src: None, fade_in: Some([now + flight, now + flight + out.max(1e-3)]), grow: None, keyed: None });
                    continue;
                }
                used[k] = true;
                let pin = self.nodes[node].own_size.map_or(old[k].pin, |a| a.0);
                let keep_h = n.paint[3] == SHAPE && n.rect[2] > n.rect[3] + 0.5 && (n.rect[3] - 2.0 * n.uv[0]).abs() < 0.01;
                self.flows.insert(id, FlowRec { from: o.rect, clip: o.clip, start: now, dur: flight, loose: 0.0, src: None, fade_in: None, grow: None, keyed: Some(Keyed { grow: g, old: o, pin, keep_h }) });
                self.births.insert(id, o.extra[3]);
                self.base[i].extra[3] = o.extra[3];
            }
        }
        // With no flight (`flow_ms` 0) nothing is shared: the old page fades out and the new in.
        let by_tag: HashMap<&Tag, usize> = if flight <= 0.0 { HashMap::default() } else { old.iter().enumerate().filter(|(k, o)| !used[*k] && !new_ids.contains(&o.id) && !grows(&o.tag)).filter_map(|(k, o)| o.tag.as_ref().map(|t| (t, k))).collect() };
        for i in 0..self.page_len {
            let id = self.owners[i].1;
            if old_ids.contains(&id) || self.flows.contains_key(&id) || grows(&self.tags[i]) { continue; }
            let Some(&k) = self.tags[i].as_ref().and_then(|t| by_tag.get(t)) else { continue };
            if used[k] { continue; }
            used[k] = true;
            let o = &old[k];
            self.flows.insert(id, FlowRec { from: o.inst.rect, clip: o.inst.clip, start: now, dur: flight, loose: spec.flow_loose, src: None, fade_in: None, grow: None, keyed: None });
            self.births.insert(id, o.inst.extra[3]);
            self.base[i].extra[3] = o.inst.extra[3];
        }
        // What is new on the page fades in over `fade_ms` as the old fades out, with or without less
        // motion: it is never drawn whole over the old page from the first frame. While a picture grows,
        // it comes in once the picture has landed; what is new over the picture itself is there at once
        // then, as what rode on it goes (each note drawn once, every frame).
        let fade = (spec.fade_ms / 1000.0).max(1e-3);
        let wait = if grow_of.is_empty() { 0.0 } else { flight };
        for i in 0..self.page_len {
            let id = self.owners[i].1;
            if old_ids.contains(&id) || self.flows.contains_key(&id) { continue; }
            let fade = if grows(&self.tags[i]) { 0.0 } else { fade };
            self.flows.insert(id, FlowRec { from: self.base[i].rect, clip: self.base[i].clip, start: now, dur: wait + fade.max(1e-3), loose: 0.0, src: None, fade_in: Some([now + wait, now + wait + fade]), grow: None, keyed: None });
        }
        // Around a picture that grows is the page it grows out of; one that shrinks takes the page it
        // leaves with it: what else of that page lies in the part that showed rides with it too, and
        // what lies outside goes at once, so around it is only the new page's ground.
        let area = |r: [f32; 4]| (r[2] - r[0]) * (r[3] - r[1]);
        let grows_out = grow_of.values().any(|&g| area(self.grows[g].r0) < area(self.grows[g].r1));
        let shrinks = grow_of.values().copied().find(|&g| area(self.grows[g].r0) > area(self.grows[g].r1));
        let mut going = vec![];
        for (k, o) in old.iter().enumerate() {
            if used[k] || new_ids.contains(&o.id) || spec.out_ms <= 0.0 { continue; }
            let mut ride = o.tag.as_ref().and_then(|t| grow_of.get(&t.flow)).copied();
            // The rest of the picture's own subtree rides with it, whole, until it lands.
            let on_it = ride.is_some();
            if ride.is_none() && let Some(g) = shrinks {
                let (r, b) = (self.grows[g].r0, o.inst.rect);
                if b[0] < r[0] || b[1] < r[1] || b[0] + b[2] > r[2] || b[1] + b[3] > r[3] { continue; }
                ride = Some(g);
            }
            let mut loosen = ride.is_none();
            // What lies along the edge of the part that showed (a frame round the window) is the
            // window's, not the picture's: it fades where it was, under the picture that grows over it.
            if let Some(g) = ride {
                let (r, b) = (self.grows[g].r0, o.inst.rect);
                let (cx, cy) = (b[0] + b[2] * 0.5, b[1] + b[3] * 0.5);
                if cx < r[0] + 3.0 || cy < r[1] + 3.0 || cx > r[2] - 3.0 || cy > r[3] - 3.0 { (ride, loosen) = (None, false); }
            }
            let (start, dur) = if on_it && ride.is_some() { if xfade[k] { (now + flight, out) } else { (now + flight - 1e-4, 1e-4) } } else { (now, out) };
            going.push((k, Ghost { inst: o.inst, start, dur, loosen, ride, anchor: o.inst.rect, pin: o.pin, scaled: o.scaled }));
        }
        // What rides moves by parts of its element, each part whole: an element's glyphs together (a
        // word or label), and lines or shapes that touch (one box of four lines) together, so a part
        // keeps its shape while only the parts' places close up with the picture.
        let mut part: Vec<usize> = (0..going.len()).collect();
        fn root(p: &mut [usize], mut i: usize) -> usize { while p[i] != i { p[i] = p[p[i]]; i = p[i]; } i }
        // By what each covers, turned (a drawing's strokes are), as (x0, y0, x1, y1).
        let touch = |a: [f32; 4], b: [f32; 4]| a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3];
        let mut by_node: HashMap<(usize, usize), Vec<usize>> = HashMap::default();
        // (What is scaled with the view goes with the picture's transform instance by instance.)
        for (j, (k, g)) in going.iter().enumerate() { if let Some(r) = g.ride && !g.scaled { by_node.entry((old[*k].node, r)).or_default().push(j); } }
        for js in by_node.values() {
            for (a, &x) in js.iter().enumerate() { for &y in &js[a + 1..] {
                let (gx, gy) = (&going[x].1.inst, &going[y].1.inst);
                if (gx.paint[3] == GLYPH && gy.paint[3] == GLYPH) || touch(gx.bounds(0.5), gy.bounds(0.5)) { let (rx, ry) = (root(&mut part, x), root(&mut part, y)); part[rx] = ry; }
            } }
        }
        let mut boxes: HashMap<usize, [f32; 4]> = HashMap::default();
        for (j, (_, g)) in going.iter().enumerate() {
            let (r, b) = (root(&mut part, j), g.inst.bounds(0.0));
            let e = boxes.entry(r).or_insert(b);
            *e = [e[0].min(b[0]), e[1].min(b[1]), e[2].max(b[2]), e[3].max(b[3])];
        }
        for (j, (_, mut g)) in going.into_iter().enumerate() {
            let b = boxes[&root(&mut part, j)];
            g.anchor = [b[0], b[1], b[2] - b[0], b[3] - b[1]];
            self.ghosts.push(g);
        }
        if grows_out && let Some(g) = self.ground_seen { self.ground_hold = Some((g, now + flight)); }
        self.flow_join = Some(FlowJoin { start: now, wait, fade, grows: grow_of.keys().cloned().collect(), seen: new_ids, end: now + wait + fade });
        self.animating = true;
        self.finish();
    }
    /// A view laid out while a flow is on its way: what it draws that the page had not drawn since the
    /// flow began comes in as what was new then does (`flow_from`): over a picture that grows, once it
    /// has landed; elsewhere fading in after it. Not drawn at once where it is, outside the part of
    /// the picture that shows.
    fn flow_late(&mut self) {
        let Some(j) = &mut self.flow_join else { return };
        if self.clock >= j.end { self.flow_join = None; return; }
        let mut any = false;
        for i in 0..self.page_len {
            let id = self.owners[i].1;
            if !j.seen.insert(id) || self.flows.contains_key(&id) { continue; }
            let fade = if self.tags[i].as_ref().is_some_and(|t| j.grows.contains(&t.flow)) { 0.0 } else { j.fade };
            let (a, b) = (j.start + j.wait, j.start + j.wait + fade);
            self.flows.insert(id, FlowRec { from: self.base[i].rect, clip: self.base[i].clip, start: j.start, dur: j.wait + fade.max(1e-3), loose: 0.0, src: None, fade_in: Some([a, b]), grow: None, keyed: None });
            any = true;
        }
        if any { self.animating = true; self.finish(); }
    }

    /// Work out what changed in each recomposing text since it was last painted (DESIGN.md §1.16). The
    /// strings are compared by grapheme: the common prefix and suffix are unchanged (their glyphs keep
    /// their birth and slide to where they now are), the rest changed. The changed old glyphs (or, while
    /// they are still recomposing, the tiles where they are now) become tiles that fly to the changed new
    /// glyphs, which stay hidden until the tiles land.
    fn recompose_jobs(&mut self, jobs: Vec<RecJob>, births: &mut HashMap<u64, f32>) {
        use unicode_segmentation::UnicodeSegmentation;
        let now = self.clock;
        let spec = self.timing.recompose;
        let done: Vec<(usize, String)> = jobs.iter().map(|j| (j.node, self.nodes[j.node].key.clone())).collect();
        // Each recording text's glyphs as they were put in the scene (clip, looseness, response, birth), by
        // identity: one look over the scene for all of them.
        let mut recording = vec![false; self.nodes.len()];
        for j in &jobs { recording[j.node] = true; }
        let index: HashMap<u64, usize> = (0..self.scene.instances.len()).filter(|&k| self.owners.get(k).is_some_and(|o| recording.get(o.0).copied().unwrap_or(false))).map(|k| (self.owners[k].1, k)).collect();
        for job in jobs {
            let mut new = job.rec;
            for (id, inst) in new.glyphs.iter_mut() { if let Some(&k) = index.get(id) { *inst = self.scene.instances[k]; } }
            // The same text (a paint without a new view: a scroll step): only where it is and its glyphs are new.
            if let Some(o) = self.recs.get_mut(&self.nodes[job.node].key) && o.s == new.s { *o = TextRec { changed: o.changed, ..new }; continue; }
            let key = self.nodes[job.node].key.clone();
            let old = self.recs.remove(&key);
            let changed = match &old { None => f32::NEG_INFINITY, Some(o) if o.s == new.s => o.changed, Some(_) => now };
            self.recs.insert(key.clone(), TextRec { glyphs: new.glyphs.clone(), changed, ..new.clone_head() });
            let Some(old) = old else { continue };
            if old.s == new.s { continue; }
            // A text that changes faster than the minimum interval, or a change that would put more
            // tiles in the window than allowed, is replaced at once: no tiles, no slides, no condensing.
            let live: usize = self.recomps.iter().filter(|(k, _)| **k != key).map(|(_, r)| r.tiles.len()).sum();
            if now - old.changed < spec.min_interval_ms / 1000.0 - 1e-4 || live as f32 >= spec.max_tiles {
                self.recomps.remove(&key);
                for (id, _) in &new.glyphs {
                    births.insert(*id, -1e4);
                    self.slides.remove(id);
                    if let Some(&k) = index.get(id) { self.scene.instances[k].extra[3] = -1e4; }
                }
                continue;
            }
            // Common prefix and suffix, in whole graphemes.
            let og: Vec<(usize, &str)> = old.s.grapheme_indices(true).collect();
            let ng: Vec<(usize, &str)> = new.s.grapheme_indices(true).collect();
            let pre = og.iter().zip(&ng).take_while(|(a, b)| a.1 == b.1).count();
            let suf = og[pre..].iter().rev().zip(ng[pre..].iter().rev()).take_while(|(a, b)| a.1 == b.1).count();
            let cut = |g: &[(usize, &str)], len: usize, k: usize| if k < g.len() { g[k].0 } else { len };
            let (op, os) = (cut(&og, old.s.len(), pre), cut(&og, old.s.len(), og.len() - suf));
            let (np, ns) = (cut(&ng, new.s.len(), pre), cut(&ng, new.s.len(), ng.len() - suf));
            // Which glyph is in which part: its centre in the boxes of that byte range (as a selection).
            let s = self.scale;
            let mut parts = |rec: &TextRec, a: usize, b: usize| -> Vec<u8> {
                let para = Para { s: &rec.s, face: rec.face, size: rec.size, line: rec.line, width: rec.width, print: rec.print, around: None, track: rec.track };
                let boxes = [(0, a), (a, b), (b, rec.s.len())].map(|(x, y)| if x < y { self.text.boxes(para, rec.origin, x, y) } else { vec![] });
                rec.glyphs.iter().map(|(_, g)| {
                    let (cx, cy) = ((g.rect[0] + g.rect[2] * 0.5) / s, (g.rect[1] + g.rect[3] * 0.5) / s);
                    let inside = |k: usize| boxes[k].iter().any(|r| r.inset(-1.0, -1.0, -1.0, -1.0).contains(cx, cy));
                    if inside(1) { 1 } else if inside(0) { 0 } else if inside(2) { 2 } else { 1 }
                }).collect()
            };
            let (opart, npart) = (parts(&old, op, os), parts(&new, np, ns));
            // A clamped text letting go of its clamp: its ellipsis stays where it is, not cut into tiles,
            // until the first glyph that comes after it has more than half its ink (DESIGN.md §1.2), so
            // the end of the line always shows one or the other.
            let dots = ["\u{2026}", "..."].into_iter().find(|d| old.s.ends_with(d) && new.s.starts_with(old.s[..old.s.len() - d.len()].trim_end()) && new.s.len() > old.s.len() - d.len());
            let ellipsis: Vec<bool> = match dots {
                Some(d) => {
                    let para = Para { s: &old.s, face: old.face, size: old.size, line: old.line, width: old.width, print: old.print, around: None, track: old.track };
                    let b = self.text.boxes(para, old.origin, old.s.len() - d.len(), old.s.len());
                    old.glyphs.iter().map(|(_, g)| { let (cx, cy) = ((g.rect[0] + g.rect[2] * 0.5) / s, (g.rect[1] + g.rect[3] * 0.5) / s); b.iter().any(|r| r.inset(-1.0, -1.0, -1.0, -1.0).contains(cx, cy)) }).collect()
                }
                None => vec![false; old.glyphs.len()],
            };
            let mut keep: Vec<Instance> = vec![];
            let run = self.recomps.remove(&key);
            let hidden_before: HashSet<u64> = run.as_ref().map(|r| r.hide.clone()).unwrap_or_default();
            let shown = |id: u64, inst: &Instance, slides: &HashMap<u64, Slide>| -> Instance {
                let mut i = *inst;
                if let Some(sl) = slides.get(&id) {
                    let e = Ease::InOutCubic.at(((now - sl.start) / sl.dur.max(1e-4)).clamp(0.0, 1.0));
                    for k in 0..4 { i.rect[k] = sl.from[k] + (i.rect[k] - sl.from[k]) * e; }
                }
                i
            };
            // A retarget reuses the tiles in flight: each goes on from where it is, carrying what it shows
            // now (pieces cut once from whole glyphs, never cut again, so nothing multiplies). Tiles that
            // were fading out (merges, dust) are let go.
            let prev: Vec<Unit> = run.as_ref().map(|r| r.tiles.iter().filter(|t| t.p.misc[0] != morph::kind::MERGE && t.p.misc[0] != morph::kind::SRC_DUST)
                .filter_map(|t| tile_now(t, now - r.start)).collect()).unwrap_or_default();
            let mut src: Vec<Instance> = vec![];
            let mut dst: Vec<Instance> = vec![];
            let mut new_slides = vec![];
            let mut new_changed: Vec<u64> = vec![];
            for part in [0u8, 2] {
                let o: Vec<&(u64, Instance)> = old.glyphs.iter().zip(&opart).filter(|(_, p)| **p == part).map(|(g, _)| g).collect();
                let n: Vec<&(u64, Instance)> = new.glyphs.iter().zip(&npart).filter(|(_, p)| **p == part).map(|(g, _)| g).collect();
                for (k, (nid, ninst)) in n.iter().enumerate() {
                    match o.get(k) {
                        // A glyph that was still recomposing is part of the tiles: it recomposes on.
                        Some((oid, _)) if hidden_before.contains(oid) => { new_changed.push(*nid); dst.push(Instance { extra: [ninst.extra[0], ninst.extra[1], ninst.extra[2], -1e4], ..*ninst }); }
                        Some((oid, oinst)) => {
                            let born = births.get(oid).copied().unwrap_or(oinst.extra[3]);
                            births.insert(*nid, born);
                            if let Some(&k) = index.get(nid) { self.scene.instances[k].extra[3] = born; }
                            let from = shown(*oid, oinst, &self.slides);
                            let moved = (0..4).any(|c| (from.rect[c] - ninst.rect[c]).abs() > 0.25);
                            if moved && spec.slide_ms > 0.0 { new_slides.push((*nid, Slide { from: from.rect, clip: from.clip, start: now, dur: spec.slide_ms / 1000.0 })); }
                        }
                        None => { new_changed.push(*nid); dst.push(Instance { extra: [ninst.extra[0], ninst.extra[1], ninst.extra[2], -1e4], ..*ninst }); }
                    }
                }
                for (oid, oinst) in o.iter().skip(n.len()) { if !hidden_before.contains(oid) { src.push(shown(*oid, oinst, &self.slides)); } }
            }
            for (((oid, oinst), p), e) in old.glyphs.iter().zip(&opart).zip(&ellipsis) {
                if *p != 1 || hidden_before.contains(oid) { continue; }
                if *e { keep.push(shown(*oid, oinst, &self.slides)); } else { src.push(shown(*oid, oinst, &self.slides)); }
            }
            // The first glyph after the ellipsis: the first new glyph in the changed part.
            let first: Option<Instance> = if keep.is_empty() { None } else { new.glyphs.iter().zip(&npart).find(|(_, p)| **p == 1).map(|(g, _)| g.1) };
            for ((nid, ninst), p) in new.glyphs.iter().zip(&npart) { if *p == 1 { new_changed.push(*nid); dst.push(Instance { extra: [ninst.extra[0], ninst.extra[1], ninst.extra[2], -1e4], ..*ninst }); } }
            for (id, sl) in new_slides { self.slides.insert(id, sl); }
            // The new glyphs are there all along (under their tiles): they do not condense.
            for id in &new_changed {
                births.insert(*id, -1e4);
                if let Some(&k) = index.get(id) { self.scene.instances[k].extra[3] = -1e4; }
            }
            if src.is_empty() && dst.is_empty() && prev.is_empty() { continue; }
            // Whole glyphs only (the tiles in flight are not counted again).
            let glyphs = src.len().max(dst.len()).max(1) as f32;
            // A retarget lands when the run it replaces would have (but takes at least a third of a
            // flight), so a quick run of changes keeps arriving instead of starting over each time.
            let dur_ms = match &run { Some(r) => ((r.end - now) * 1000.0).max(spec.duration_ms * 0.35).min(spec.duration_ms), None => spec.duration_ms };
            let mspec = MorphSpec { duration_ms: dur_ms, stagger: spec.stagger, count_max: spec.count_max * glyphs, curl: 0.0, threshold: 0.5, ease: Ease::InOutCubic, lattice: (spec.tile * self.scale).round().max(1.0), ..MorphSpec::default() };
            let mut rtiles = recompose_tiles(prev, &src, &dst, &mspec, &self.text.pixels, !keep.is_empty());
            // Never more than the window's budget (what is left of it after the other texts).
            let budget = (spec.max_tiles - live as f32).max(0.0) as usize;
            if rtiles.len() > budget { rtiles.truncate(budget); }
            let dur = dur_ms / 1000.0;
            // When more than half of the cells that land in that glyph show it.
            let keep_until = match first {
                None => f32::NEG_INFINITY,
                Some(g) => {
                    // Where a tile lands: its cell (the pieces are placed relative to it).
                    let inside = |t: &RTile| { let (cx, cy) = (t.p.ab[2] + t.p.misc[1] * 0.5, t.p.ab[3] + t.p.misc[1] * 0.5); cx >= g.rect[0] && cy >= g.rect[1] && cx < g.rect[0] + g.rect[2] && cy < g.rect[1] + g.rect[3] };
                    let mine: Vec<&RTile> = rtiles.iter().filter(|t| !t.dst.is_empty() && inside(t)).collect();
                    let mut at = dur;
                    let mut t = 0.0;
                    while t < dur {
                        let landed = mine.iter().filter(|r| morph::progress(&r.p, t, Ease::InOutCubic) >= r.p.misc[3]).count();
                        if !mine.is_empty() && landed * 2 > mine.len() { at = t; break; }
                        t += 1.0 / 240.0;
                    }
                    now + at
                }
            };
            self.recomps.insert(key, Recomp { tiles: rtiles, start: now, end: now + dur, hide: new_changed.into_iter().collect(), keep, keep_until });
            self.animating = true;
        }
        // Each text's record holds its glyphs as this paint leaves them (births given above included), as
        // the next paint would find them: a paint that copies the text (`Ui::restage`) records nothing.
        for (node, key) in done {
            let Some(rec) = self.recs.get_mut(&key) else { continue };
            let index: HashMap<u64, usize> = (0..self.scene.instances.len()).filter(|&k| self.owners.get(k).is_some_and(|o| o.0 == node)).map(|k| (self.owners[k].1, k)).collect();
            for (id, inst) in rec.glyphs.iter_mut() { if let Some(&k) = index.get(id) { *inst = self.scene.instances[k]; } }
        }
    }

    /// Put the marks on what they frame now: the target the pointer or the keyboard gives, in this
    /// frame. A new target restarts the short settle; the same target moved (a scroll, a layout) is
    /// followed exactly. Without a target they fade out where they were within `MARKS_SETTLE`.
    fn aim_marks(&mut self) {
        // While something is dragged the marks stand aside; on hover they skip what is `unframed`.
        let target = if self.capture.is_some() { None } else if self.keyboard { self.focus.clone() } else { self.hover.clone().or_else(|| self.focus.clone()) };
        let target = target.and_then(|k| self.node(&k).filter(|n| self.keyboard || !n.unframed).map(|n| (k.clone(), n.rect, n.clip, self.framing(n))));
        let now = self.clock;
        let m = &mut self.marks;
        match target {
            Some((k, r, clip, within)) => {
                if m.key.as_deref() != Some(k.as_str()) { m.key = Some(k); m.since = now; }
                m.at = Some(r);
                m.clip = clip;
                m.within = within;
                // Seen at once, full within the settle.
                m.alpha = (0.5 + 0.5 * (now - m.since) / MARKS_SETTLE).clamp(0.5, 1.0);
            }
            None => {
                if m.key.take().is_some() { m.since = now; m.fade = m.alpha; }
                m.alpha = (m.fade * (1.0 - (now - m.since) / MARKS_SETTLE)).max(0.0);
                if m.alpha < 0.02 { m.alpha = 0.0; }
            }
        }
    }
    /// Where the marks of a part of a compound control (a segment, a tab: a control inside another one
    /// that draws a border round it) stay: inside that border, so they never sit on it.
    fn framing(&self, n: &Node<M>) -> Option<Rect> {
        let mut p = n.parent;
        while let Some(i) = p {
            let c = &self.nodes[i];
            if c.active() && let Some((w, _)) = c.border { let k = w + 1.0; return Some(c.rect.inset(k, k, k, k)); }
            p = c.parent;
        }
        None
    }
    /// What the hover / focus marks frame as drawn now (logical px, before their 4 px margin and the
    /// settle's opening) and how opaque they are; `None` when they are not drawn.
    pub fn marks_frame(&self) -> Option<(Rect, f32)> { self.marks.at.filter(|_| self.marks.alpha >= 0.02).map(|r| (r, self.marks.alpha)) }
    /// The marks are still closing onto their target or fading.
    fn marks_settling(&self) -> bool { self.marks.alpha > 0.0 && (self.clock - self.marks.since) < MARKS_SETTLE }

    fn paint_marks(&mut self) {
        self.scene.truncate(self.body_len);
        if self.marks.alpha < 0.02 { return; }
        let Some(r) = self.marks.at else { return };
        let s = self.scale;
        // The cosmetic close: a few pixels out at the change, on the target `MARKS_SETTLE` later.
        let e = if self.marks.key.is_some() { 1.0 - ((self.clock - self.marks.since) / MARKS_SETTLE).clamp(0.0, 1.0) } else { 0.0 };
        let open = 4.0 + MARKS_OPEN * e * e;
        // A front (a press's) pushes the corners as it pushes the control they frame: as one piece about
        // its centre, so they stay on it.
        let (mx, my) = r.center();
        let resp = [0.0, 2.0, mx * s, my * s];
        let mut r = r.inset(-open, -open, -open, -open);
        if let Some(b) = self.marks.within { r = r.intersect(&b); }
        let (len, th) = (8.0f32.min(r.w * 0.4).min(r.h * 0.4), 1.5);
        let c = self.marks.clip.inset(-6.0, -6.0, -6.0, -6.0);
        let clip = [c.x * s, c.y * s, c.right() * s, c.bottom() * s];
        // Over the material, exact, in the ink (the annotation layer), or toward the accent by
        // `Fx::marks_accent`: a material would draw the bars as its own shapes (near's bands round each
        // one, with the control's border running through).
        let look = self.look();
        let k = look.fx.marks_accent.clamp(0.0, 1.0);
        let accent: [f32; 3] = std::array::from_fn(|i| look.theme.accent[i] * k + look.theme.ink[i] * (1.0 - k));
        let bar = |x: f32, y: f32, w: f32, h: f32| Instance {
            rect: [(x * s).round(), (y * s).round(), (w * s).round().max(1.0), (h * s).round().max(1.0)],
            paint: [0.0, 0.0, self.marks.alpha, SHAPE], clip, look: [accent[0], accent[1], accent[2], 0.0], extra: [0.0, 0.0, 0.0, -1e4], resp, ..Default::default()
        };
        for (cx, cy, sx, sy) in [(r.x, r.y, 1.0, 1.0), (r.right(), r.y, -1.0, 1.0), (r.x, r.bottom(), 1.0, -1.0), (r.right(), r.bottom(), -1.0, -1.0)] {
            let x0 = if sx > 0.0 { cx } else { cx - len };
            let y0 = if sy > 0.0 { cy } else { cy - len };
            self.scene.annot.push(bar(x0, if sy > 0.0 { cy } else { cy - th }, len, th));
            self.scene.annot.push(bar(if sx > 0.0 { cx } else { cx - th }, y0, th, len));
        }
    }

    /// Stage 3: the annotation records of this frame and their instances, exact, in Departure Mono at
    /// whole multiples of its size.
    fn paint_gaze(&mut self) {
        self.paint_gaze_marks();
        self.paint_guides();
    }
    /// Guides (`El::guide`): each a closed outline drawn as dots a device px or two across, every 4
    /// logical px, in the annotation colour, over everything, whatever the annotation mode.
    fn paint_guides(&mut self) {
        let s = self.scale;
        let ground = self.look().theme.ground;
        let col = self.gaze_of.map_or(Gaze::default(), |f| f(&self.state)).colour_on(ground);
        let t = s.round().max(1.0) * 1.5;
        for n in self.nodes.iter().filter(|n| !n.hidden) {
            let Some(g) = &n.guide else { continue };
            let clip = [n.clip.x * s, n.clip.y * s, n.clip.right() * s, n.clip.bottom() * s];
            let (ox, oy) = (n.rect.x, n.rect.y);
            for k in 0..g.len() {
                let (a, b) = (g[k], g[(k + 1) % g.len()]);
                let len = (b.0 - a.0).hypot(b.1 - a.1);
                let steps = (len / 4.0).ceil().max(1.0) as usize;
                for j in 0..steps {
                    let f = j as f32 / steps as f32;
                    let (x, y) = ((ox + a.0 + (b.0 - a.0) * f) * s, (oy + a.1 + (b.1 - a.1) * f) * s);
                    self.scene.annot.push(Instance { rect: [(x - t * 0.5).round(), (y - t * 0.5).round(), t.round(), t.round()], paint: [0.0, 0.0, 0.95, SHAPE], clip, look: [col[0], col[1], col[2], 0.0], ..Default::default() });
                }
            }
        }
    }
    fn paint_gaze_marks(&mut self) {
        self.scene.annot.clear();
        self.leaders.clear();
        self.readout = gaze::Readout::default();
        let g = match self.gaze_of { Some(f) => f(&self.state), None => { self.annots.clear(); return; } };
        if g.mode == GazeMode::Off { self.annots.clear(); self.gaze_seen.clear(); self.gaze_until = f32::NEG_INFINITY; return; }
        let s = self.scale;
        let now = self.clock;
        let window = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let pointer = (self.field.pointer[2] > 0.0).then(|| (self.field.pointer[0] / s, self.field.pointer[1] / s));
        let focus = self.focus.as_deref().and_then(|f| self.nodes.iter().position(|n| n.key == f));
        let annots = gaze::select(&self.nodes, &g, pointer, focus, window);
        self.gaze_seen = annots.iter().map(|a| (a.key.clone(), self.gaze_seen.get(&a.key).copied().unwrap_or(now))).collect();
        self.gaze_until = self.gaze_seen.values().fold(f32::NEG_INFINITY, |m, &t| m.max(t + g.draw_ms / 1000.0));
        let (scope, scopes) = gaze::scope(&self.nodes, window);
        let view = scopes.first().map_or(window, |&i| self.nodes[i].clip);
        let clip = [view.x * s, view.y * s, view.right() * s, view.bottom() * s];
        let gc = g.colour_on(self.look().theme.ground);
        let col = [gc[0], gc[1], gc[2], 0.0];
        let t = s.round().max(1.0);
        // The scope-wide readouts show with the pointer in the scope (Hover, Focus) or always (All).
        let in_scope = pointer.filter(|&(x, y)| scope.contains(x, y));
        let readouts = (g.mode == GazeMode::All || in_scope.is_some()) && g.kinds & (kinds::EDGE | kinds::OSC | kinds::LEVEL) != 0;
        let notes: Vec<String> = if readouts && g.has(kinds::EDGE) {
            let look = self.look();
            let theme = crate::style::THEME_IDS.get(crate::style::theme_index(look.theme.name)).copied().unwrap_or("custom");
            let loosest = (0..self.page_len.min(self.scene.instances.len())).map(|i| self.inst_loose(i, now)).fold(0.0, f32::max);
            vec![
                format!("{:.0} × {:.0}", scope.w, scope.h),
                in_scope.map_or_else(|| "x — y —".to_owned(), |(x, y)| format!("x {:.0} y {:.0}", x - scope.x, y - scope.y)),
                format!("{} · {theme}", look.material.name()),
                format!("loose {loosest:.2}"),
                format!("{} nodes", self.fmap.nonzero()),
            ]
        } else { vec![] };
        let mut out: Vec<Instance> = Vec::new();
        let dev = |v: f32| (v * s).round();
        // A line or a block in device px: the gaze colour, or the page ground (`plate`) under labels.
        let bar = |out: &mut Vec<Instance>, x: f32, y: f32, w: f32, h: f32, alpha: f32, plate: bool| {
            if w <= 0.0 || h <= 0.0 { return; }
            out.push(Instance { rect: [x, y, w, h], paint: [0.0, if plate { 1.0 } else { 0.0 }, alpha, SHAPE], clip, look: col, ..Default::default() });
        };
        const SIZE: f32 = 11.0;
        const LINE: f32 = 14.0;
        // The true ink of each annotated element (device px) and the point of its outline nearest its
        // layout box's corner, measured as the audit measures it.
        let inks: Vec<crate::hit::Ink> = if g.has(kinds::INK) {
            annots.iter().map(|a| crate::hit::ink_of(&self.scene.instances, &self.owners[..self.page_len.min(self.owners.len())], |n| within(&self.nodes, n, a.node), (a.rect.x * s, a.rect.y * s), &self.text.pixels)).collect()
        } else { vec![] };
        // Echoes (`Gaze::echo`): every `echo_ms` (whole steps on a beat; the beat clock, so a hold
        // takes none and fades none) each element's INK box is kept, the last `echo` of them drawn
        // fainter and fainter until they are gone.
        let tg = self.beat_grid();
        let e = { let e = g.echo_ms.max(16.0) / 1000.0; if self.timing.beat.on() { let st = self.timing.beat.step_s(); (e / st).round().max(1.0) * st } else { e } };
        let n_echo = g.echo.min(8) as usize;
        let life = n_echo as f32 * e;
        if g.has(kinds::INK) && n_echo > 0 {
            if tg >= self.echo_at + e - 1e-4 || tg < self.echo_at {
                self.echo_at = tg;
                for (a, ink) in annots.iter().zip(&inks) {
                    let Some((b, _)) = ink else { continue };
                    let h = self.echoes.entry(a.key.clone()).or_default();
                    if h.last().is_none_or(|(_, l)| l != b) { h.push((tg, *b)); } else if let Some(l) = h.last_mut() { l.0 = tg; }
                }
            }
            for h in self.echoes.values_mut() {
                let keep_last = h.len().saturating_sub(n_echo);
                h.drain(..keep_last);
                let latest = h.len().saturating_sub(1);
                let mut k = 0;
                h.retain(|(t0, _)| { let alive = k == latest || tg - t0 < life; k += 1; alive });
            }
            self.echoes.retain(|key, h| !h.is_empty() && annots.iter().any(|a| &a.key == key));
            // Frames go on until the last echo has faded.
            let last = self.echoes.values().flat_map(|h| h[..h.len() - 1].iter()).fold(f32::NEG_INFINITY, |m, (t0, _)| m.max(t0 + life));
            self.gaze_until = self.gaze_until.max(last);
        } else { self.echoes.clear(); }
        let echoes = &self.echoes;
        let text = &mut self.text;
        // A label at logical (x, y), on a plate; returns its width.
        let label = |text: &mut Text, out: &mut Vec<Instance>, str: &str, x: f32, y: f32| -> f32 {
            let (w, _) = text.measure(str, MONO, SIZE, LINE, None);
            bar(out, dev(x) - t * 2.0, dev(y), dev(w) + t * 4.0, dev(LINE), 1.0, true);
            let proto = Instance { paint: [0.0, 0.0, 1.0, 0.0], look: col, ..Default::default() };
            text.emit(Para { s: str, face: MONO, size: SIZE, line: LINE, width: 1e5, print: 0.0, around: None, track: 0.0 }, (x, y), proto, s, |mut i, _| { i.clip = clip; out.push(i); });
            w
        };
        // Labels keep off the ink of text to read (each element's, logical px) and off each other.
        let avoid: Vec<Rect> = {
            let mut by: std::collections::HashMap<usize, [f32; 4]> = Default::default();
            for gl in &self.glyphs.log {
                let n = &self.nodes[gl.node];
                let Some(inst) = self.scene.instances.get(gl.inst) else { continue };
                if !n.legible || n.hidden || inst.paint[2] < 0.05 { continue; }
                let b = crate::form::ink_box(inst);
                if b[2] <= b[0] || b[3] <= b[1] { continue; }
                let e = by.entry(gl.node).or_insert([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY]);
                *e = [e[0].min(b[0]), e[1].min(b[1]), e[2].max(b[2]), e[3].max(b[3])];
            }
            by.values().map(|b| Rect::new(b[0] / s, b[1] / s, (b[2] - b[0]) / s, (b[3] - b[1]) / s)).collect()
        };
        let mut tags: Vec<Rect> = vec![];
        for a in &annots {
            let p = gaze::drawn(now - self.gaze_seen[&a.key], g.draw_ms);
            let (x0, y0, x1, y1) = (dev(a.rect.x), dev(a.rect.y), dev(a.rect.right()), dev(a.rect.bottom()));
            let (w, h) = (x1 - x0, y1 - y0);
            if g.has(kinds::BOX) {
                // Drawn on round the rect: top, right, bottom, left.
                bar(&mut out, x0, y0, (w * p).round(), t, 0.9, false);
                bar(&mut out, x1 - t, y0, t, (h * p).round(), 0.9, false);
                bar(&mut out, x1 - (w * p).round(), y1 - t, (w * p).round(), t, 0.9, false);
                bar(&mut out, x0, y1 - (h * p).round(), t, (h * p).round(), 0.9, false);
            }
            if g.has(kinds::FRAME) {
                let len = (10.0f32 * s).min(w * 0.3).min(h * 0.3).max(t * 2.0) * p;
                let len = len.round();
                // Set 6 px out from the rect, so a corner never lies on the element's own ink.
                let g = dev(6.0);
                for (cx, cy, sx, sy) in [(x0 - g, y0 - g, 1.0, 1.0), (x1 + g, y0 - g, -1.0, 1.0), (x0 - g, y1 + g, 1.0, -1.0), (x1 + g, y1 + g, -1.0, -1.0)] {
                    let bx = if sx > 0.0 { cx } else { cx - len };
                    let by = if sy > 0.0 { cy } else { cy - t };
                    bar(&mut out, bx, by, len, t, 1.0, false);
                    let vx = if sx > 0.0 { cx } else { cx - t };
                    let vy = if sy > 0.0 { cy } else { cy - len };
                    bar(&mut out, vx, vy, t, len, 1.0, false);
                }
            }
            if let Some(Some((b, end))) = inks.get(annots.iter().position(|x| x.node == a.node).unwrap_or(usize::MAX)) {
                // Its echoes: the boxes it had before, newest first, the k-th (from 1) of n at
                // 0.6 × (1 − (k − 1) ÷ n) of the live box's opacity; thin lines, no label, no fill.
                if let Some(h) = echoes.get(&a.key) {
                    // Under the live element: nothing of an echo is drawn inside its ink box.
                    let live = [b[0].floor(), b[1].floor(), b[2].ceil(), b[3].ceil()];
                    let under = |out: &mut Vec<Instance>, x: f32, y: f32, w: f32, h: f32, a: f32| {
                        let (x1, y1) = (x + w, y + h);
                        let ix = (x.max(live[0]), x1.min(live[2]));
                        let iy = (y.max(live[1]), y1.min(live[3]));
                        if ix.0 >= ix.1 || iy.0 >= iy.1 { bar(out, x, y, w, h, a, false); return; }
                        // What is left of the bar round the box: above, below, left, right.
                        if iy.0 > y { bar(out, x, y, w, iy.0 - y, a, false); }
                        if y1 > iy.1 { bar(out, x, iy.1, w, y1 - iy.1, a, false); }
                        if ix.0 > x { bar(out, x, iy.0, ix.0 - x, iy.1 - iy.0, a, false); }
                        if x1 > ix.1 { bar(out, ix.1, iy.0, x1 - ix.1, iy.1 - iy.0, a, false); }
                    };
                    for (k, (_, eb)) in h.iter().rev().filter(|(_, eb)| eb != b).take(n_echo).enumerate() {
                        let fade = 0.9 * 0.6 * (1.0 - k as f32 / n_echo.max(1) as f32);
                        let (x0, y0, x1, y1) = (eb[0].floor(), eb[1].floor(), eb[2].ceil(), eb[3].ceil());
                        under(&mut out, x0, y0, x1 - x0, t, fade);
                        under(&mut out, x0, y1 - t, x1 - x0, t, fade);
                        under(&mut out, x0, y0, t, y1 - y0, fade);
                        under(&mut out, x1 - t, y0, t, y1 - y0, fade);
                    }
                }
                // Dashed: 4 device px on, 3 off, along each side, drawn on with the frame.
                let (x0, y0, x1, y1) = (b[0].floor(), b[1].floor(), b[2].ceil(), b[3].ceil());
                let (dash, gap) = (4.0 * t, 3.0 * t);
                let run = |out: &mut Vec<Instance>, ax: f32, ay: f32, bx: f32, by: f32| {
                    let len = (bx - ax).hypot(by - ay) * p;
                    let (ux, uy) = if len > 0.0 { ((bx - ax) / (bx - ax).hypot(by - ay), (by - ay) / (bx - ax).hypot(by - ay)) } else { (0.0, 0.0) };
                    let mut d = 0.0;
                    while d < len {
                        let e = (d + dash).min(len);
                        let (sx, sy, ex, ey) = (ax + ux * d, ay + uy * d, ax + ux * e, ay + uy * e);
                        bar(out, sx.min(ex).round(), sy.min(ey).round(), ((ex - sx).abs()).max(t).round(), ((ey - sy).abs()).max(t).round(), 0.9, false);
                        d = e + gap;
                    }
                };
                run(&mut out, x0, y0, x1, y0);
                run(&mut out, x1 - t, y0, x1 - t, y1);
                run(&mut out, x0, y1 - t, x1, y1 - t);
                run(&mut out, x0, y0, x0, y1);
                // The leader (DESIGN.md §1.7): dashed, 4 on and 4 off, from a corner of the ink box to the
                // point of the true outline nearest it, and never outside the box. Of the corners 12 px
                // clear of every other element's text to read, the one farthest from the outline; one
                // shorter than 16 px (two dashes) is not drawn: a box that hugs its ink needs none.
                if end.is_some() {
                    let own = Rect::new(b[0] / s, b[1] / s, (b[2] - b[0]) / s, (b[3] - b[1]) / s);
                    let read: Vec<[f32; 4]> = avoid.iter().filter(|r| r.intersect(&own).w <= 0.0 || r.intersect(&own).h <= 0.0)
                        .map(|r| [r.x * s, r.y * s, r.right() * s, r.bottom() * s]).collect();
                    let clear = |q: [f32; 2]| read.iter().all(|r| q[0] < r[0] - 12.0 * s || q[0] > r[2] + 12.0 * s || q[1] < r[1] - 12.0 * s || q[1] > r[3] + 12.0 * s);
                    let mut best: Option<(f32, [f32; 2], (f32, f32))> = None;
                    for c in [[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]].into_iter().filter(|q| clear(*q)) {
                        let near = crate::hit::ink_of(&self.scene.instances, &self.owners[..self.page_len.min(self.owners.len())], |n| within(&self.nodes, n, a.node), (c[0], c[1]), &text.pixels).and_then(|(_, e)| e);
                        if let Some(e) = near { let d = (e.0 - c[0]).hypot(e.1 - c[1]); if best.is_none_or(|(bd, _, _)| d > bd) { best = Some((d, c, e)); } }
                    }
                    if let Some((d, f, (ex, ey))) = best && d >= 16.0 * s {
                        let n = ((d / (4.0 * s)).ceil() as usize).max(2);
                        let line: Vec<[f32; 2]> = (0..=n).map(|k| { let u = k as f32 / n as f32; [f[0] + (ex - f[0]) * u, f[1] + (ey - f[1]) * u] }).collect();
                        let mut path = crate::strand::clear_path(line, &[], 0.0, &read, 12.0 * s);
                        // Inside the box, from its corner to the outline.
                        for q in path.iter_mut() { q[0] = q[0].clamp(b[0], b[2]); q[1] = q[1].clamp(b[1], b[3]); }
                        if let Some(q) = path.first_mut() { *q = f; }
                        if let Some(e) = path.last_mut() { *e = [ex, ey]; }
                        self.leaders.push((a.key.clone(), path.clone()));
                        for piece in crate::strand::dashes(&path, p, 4.0 * s, 4.0 * s) {
                            for ab in piece.windows(2) {
                                let (dx, dy) = (ab[1][0] - ab[0][0], ab[1][1] - ab[0][1]);
                                let len = dx.hypot(dy);
                                if len <= 0.0 { continue; }
                                let (cx, cy) = ((ab[0][0] + ab[1][0]) * 0.5, (ab[0][1] + ab[1][1]) * 0.5);
                                let (c, sn) = (dx / len, dy / len);
                                out.push(Instance { rect: [cx - len * 0.5, cy - t * 0.5, len, t], paint: [0.0, 0.0, 1.0, SHAPE], clip, look: col, xf: [c - 1.0, -sn, sn, c - 1.0], ..Default::default() });
                            }
                        }
                    }
                }
                if p >= 1.0 {
                    // Beside the ink box where it covers no text to read and no other label; else left out.
                    let str = format!("INK {}×{}", ((x1 - x0) / s).round(), ((y1 - y0) / s).round());
                    let (tw, _) = text.measure(&str, MONO, SIZE, LINE, None);
                    let ink = Rect::new(x0 / s, y0 / s, (x1 - x0) / s, (y1 - y0) / s);
                    if let Some(r) = gaze::place_tag(ink, (tw + 4.0, LINE), &avoid, &tags, view) {
                        tags.push(r);
                        label(text, &mut out, &str, r.x + 2.0, r.y);
                    }
                }
            }
            if g.has(kinds::LEADER) { continue; }
            let mut tag = String::new();
            if g.has(kinds::INDEX) { tag = format!("{:02}", gaze::count(a.index as f32, p) as u32); }
            if g.has(kinds::KIND) {
                if !tag.is_empty() { tag.push(' '); }
                // A callout may ask for where the element is: `{xy}` becomes its centre in the scope.
                let (cx, cy) = a.rect.center();
                let xy = format!("x {:.0}  y {:.0}", cx - scope.x, cy - scope.y);
                tag.push_str(&a.label.as_deref().unwrap_or(a.kind).replace("{xy}", &xy));
            }
            if !tag.is_empty() {
                // Above the element's top left, else below it, else to its right: the first place that
                // covers no text to read and no other label; at none of them, no tag.
                let (tw, _) = text.measure(&tag, MONO, SIZE, LINE, None);
                if let Some(r) = gaze::place_tag(a.rect, (tw + 4.0, LINE), &avoid, &tags, view) {
                    tags.push(r);
                    label(text, &mut out, &tag, r.x + 2.0, r.y);
                }
            }
            if g.has(kinds::SIZE) && a.rect.h >= LINE + 6.0 {
                // Inside the bottom right corner, where nothing else is labelled.
                let str = format!("{}×{}", gaze::count(a.rect.w.round(), p), gaze::count(a.rect.h.round(), p));
                let (tw, _) = text.measure(&str, MONO, SIZE, LINE, None);
                label(text, &mut out, &str, a.rect.right() - tw - 4.0, a.rect.bottom() - LINE - 3.0);
            }
        }
        if g.has(kinds::LEADER) {
            let names: Vec<String> = annots.iter().map(|a| {
                let name = a.label.clone().unwrap_or_else(|| a.kind.to_owned());
                if g.has(kinds::INDEX) { format!("{:02}  {name}", a.index) } else { name }
            }).collect();
            let sizes: Vec<(f32, f32)> = names.iter().map(|n| { let (w, _) = text.measure(n, MONO, SIZE, LINE, None); (w + 4.0, LINE) }).collect();
            let lscope = if g.has(kinds::RULER) { scope.inset(22.0, 22.0, 0.0, 0.0) } else { scope };
            let boxes = gaze::place_labels(&annots, lscope, |a| sizes[annots.iter().position(|b| b.node == a.node).unwrap_or(0)]);
            for (k, a) in annots.iter().enumerate() {
                let p = gaze::drawn(now - self.gaze_seen[&a.key], g.draw_ms);
                let (side, b) = boxes[k];
                let (ax, ay) = match side { gaze::Side::Left => (a.rect.x, a.rect.center().1), gaze::Side::Right => (a.rect.right(), a.rect.center().1) };
                let (lx, ly) = match side { gaze::Side::Left => (b.right() + 4.0, b.center().1), gaze::Side::Right => (b.x - 4.0, b.center().1) };
                let knee = match side { gaze::Side::Left => lx + 14.0, gaze::Side::Right => lx - 14.0 };
                // Elbow: along from the element to the knee, up or down to the label's line, into the label.
                let (ax, ay, lx, ly, knee) = (dev(ax), dev(ay), dev(lx), dev(ly), dev(knee));
                let seg1 = ((ax - knee).abs() * p).round();
                if ax > knee { bar(&mut out, ax - seg1, ay, seg1, t, 1.0, false); } else { bar(&mut out, ax, ay, seg1, t, 1.0, false); }
                if p >= 1.0 {
                    bar(&mut out, knee.min(ax).min(knee), ay.min(ly), t, (ay - ly).abs() + t, 1.0, false);
                    bar(&mut out, lx.min(knee), ly, (knee - lx).abs(), t, 1.0, false);
                }
                bar(&mut out, ax - t, ay - t, t * 3.0, t * 3.0, 1.0, false);
                if p >= 1.0 { label(text, &mut out, &names[k], b.x + 2.0, b.y); }
            }
        }
        let pt = pointer.filter(|&(x, y)| scope.contains(x, y));
        if g.has(kinds::GRID) {
            let (x0, y0, x1, y1) = (dev(scope.x), dev(scope.y), dev(scope.right()), dev(scope.bottom()));
            let mut y = y0;
            while y < y1 { bar(&mut out, x0, y, x1 - x0, t, 0.10, false); y += dev(8.0); }
            let cols = 12.0;
            let gutter = 16.0;
            let cw = (scope.w - gutter * (cols - 1.0)) / cols;
            for c in 0..12 {
                let x = scope.x + c as f32 * (cw + gutter);
                bar(&mut out, dev(x), y0, cw.mul_add(s, 0.0).round(), y1 - y0, 0.05, false);
                bar(&mut out, dev(x), y0, t, y1 - y0, 0.22, false);
                bar(&mut out, dev(x + cw) - t, y0, t, y1 - y0, 0.22, false);
            }
        }
        if g.has(kinds::RULER) {
            let (x0, y0, x1, y1) = (dev(scope.x), dev(scope.y), dev(scope.right()), dev(scope.bottom()));
            bar(&mut out, x0, y0, x1 - x0, dev(16.0), 0.85, true);
            bar(&mut out, x0, y0, dev(16.0), y1 - y0, 0.85, true);
            let mut v = 0.0;
            while scope.x + v < scope.right() {
                let len = if v % 100.0 == 0.0 { 9.0 } else if v % 50.0 == 0.0 { 6.0 } else { 3.0 };
                bar(&mut out, dev(scope.x + v), y0 + dev(16.0) - dev(len), t, dev(len), 0.9, false);
                if v % 100.0 == 0.0 && v > 0.0 { label(text, &mut out, &format!("{v}"), scope.x + v + 3.0, scope.y); }
                v += 10.0;
            }
            let mut v = 0.0;
            while scope.y + v < scope.bottom() {
                let len = if v % 100.0 == 0.0 { 9.0 } else if v % 50.0 == 0.0 { 6.0 } else { 3.0 };
                bar(&mut out, x0 + dev(16.0) - dev(len), dev(scope.y + v), dev(len), t, 0.9, false);
                v += 10.0;
            }
            if let Some((px, py)) = pt {
                bar(&mut out, dev(px), y0, t, dev(16.0), 1.0, false);
                bar(&mut out, x0, dev(py), dev(16.0), t, 1.0, false);
            }
        }
        if g.has(kinds::CROSS) && let Some((px, py)) = pt {
            bar(&mut out, dev(scope.x), dev(py), dev(scope.w), t, 0.45, false);
            bar(&mut out, dev(px), dev(scope.y), t, dev(scope.h), 0.45, false);
            label(text, &mut out, &format!("x {:.0}  y {:.0}", px - scope.x, py - scope.y), px + 10.0, py + 8.0);
        }
        if readouts {
            self.readout.scope = scope;
            let (x0, y0, x1, y1) = (dev(scope.x), dev(scope.y), dev(scope.right()), dev(scope.bottom()));
            if g.has(kinds::LEVEL) {
                // Up the left edge, 6 px wide, from the bottom.
                let level = self.fmap.level();
                let h = gaze::level_height(scope.h, level, g.level_full);
                (self.readout.level, self.readout.level_h) = (level, h);
                let hd = dev(h);
                bar(&mut out, x0, y1 - hd, dev(6.0), hd, 1.0, false);
                // How ready the page is, while it is not (DESIGN.md §1.23): a reading beside the bar.
                let ready = self.prep.readiness();
                if ready < 1.0 { label(text, &mut out, &format!("READY {ready:.2}"), scope.x + 12.0, scope.bottom() - LINE - 4.0); }
            }
            if g.has(kinds::OSC) {
                // One point per column of nodes, along the pointer's row (or the strongest), joined by a
                // line one device pixel thick: runs of whole pixels, no antialiasing, nothing mixed in.
                let m = &self.fmap;
                let row = in_scope.map_or_else(|| m.strongest_row().0, |(_, y)| (y / m.cell).round().max(0.0) as usize);
                let base = scope.y + scope.h * g.osc_y;
                // Short of the edge notes' column when they are on.
                let end = if g.has(kinds::EDGE) { scope.right() - 40.0 } else { scope.right() };
                let pts = gaze::osc_points(|i| m.byte_e(i, row), m.cols, m.cell, scope.x, end, base, g.osc_amp);
                let dp: Vec<[f32; 2]> = pts.iter().map(|p| [dev(p[0]), dev(p[1]).clamp(y0, y1 - t)]).collect();
                for ab in dp.windows(2) {
                    let ([ax, ay], [bx, by]) = (ab[0], ab[1]);
                    let mut x = ax;
                    while x < bx {
                        // The pixels this column covers between where the line enters and leaves it.
                        let (fa, fb) = ((x - ax) / (bx - ax), ((x + t) - ax) / (bx - ax));
                        let (ya, yb) = (ay + (by - ay) * fa, ay + (by - ay) * fb.min(1.0));
                        let (top, bot) = (ya.min(yb).round(), ya.max(yb).round());
                        bar(&mut out, x, top, t, (bot - top).max(t), 1.0, false);
                        x += t;
                    }
                }
                self.readout.osc_row = row;
                self.readout.osc = pts;
            }
            if g.has(kinds::EDGE) {
                // A hairline on the right edge, with ticks every 10 / 50 / 100 px and the pointer marked.
                let lx = x1 - t;
                bar(&mut out, lx, y0, t, y1 - y0, 1.0, false);
                let mut v = 0.0;
                while scope.y + v < scope.bottom() {
                    let len = if v % 100.0 == 0.0 { 9.0 } else if v % 50.0 == 0.0 { 6.0 } else { 3.0 };
                    bar(&mut out, lx - dev(len), dev(scope.y + v), dev(len), t, 1.0, false);
                    v += 10.0;
                }
                if let Some((_, py)) = in_scope {
                    bar(&mut out, lx - dev(14.0), dev(py) - t, dev(14.0), t * 2.0, 1.0, false);
                    self.readout.mark_y = Some(py);
                }
                self.readout.edge_x = Some(lx);
                // The notes in the serif, turned a quarter to read upwards, from the bottom; a note that
                // does not fit starts a new column to the left.
                const NOTE: f32 = 12.0;
                const NOTE_LINE: f32 = 15.0;
                const GAP: f32 = 18.0;
                let set = crate::text::Setting { around: None, track: 0.12 };
                let mut col_x = scope.right() - 12.0 - 4.0 - NOTE_LINE;
                let mut y = scope.bottom() - 8.0;
                for note in &notes {
                    let (w, _) = text.measure_around(note, crate::text::SERIF, NOTE, NOTE_LINE, None, set);
                    if y - w < scope.y + 8.0 && y < scope.bottom() - 8.0 { col_x -= NOTE_LINE + 4.0; y = scope.bottom() - 8.0; }
                    if y - w < scope.y + 8.0 || col_x < scope.x + 8.0 { break; }
                    // Laid out flat at the origin, then each glyph turned about its own centre and moved to
                    // where the turned line puts it: (dx, dy) → (dy, −dx) from the pivot.
                    let pivot = (dev(col_x), dev(y));
                    let proto = Instance { paint: [0.0, 0.0, 1.0, 0.0], look: col, ..Default::default() };
                    text.emit(Para { s: note, face: crate::text::SERIF, size: NOTE, line: NOTE_LINE, width: 1e5, print: 0.0, around: None, track: set.track }, (0.0, 0.0), proto, s, |mut i, _| {
                        let (cx, cy) = (i.rect[0] + i.rect[2] * 0.5, i.rect[1] + i.rect[3] * 0.5);
                        let (nx, ny) = (pivot.0 + cy, pivot.1 - cx);
                        i.rect[0] = nx - i.rect[2] * 0.5;
                        i.rect[1] = ny - i.rect[3] * 0.5;
                        i.xf = [-1.0, 1.0, -1.0, -1.0];
                        i.clip = clip;
                        out.push(i);
                    });
                    self.readout.notes.push((note.clone(), Rect::new(col_x, y - w, NOTE_LINE, w)));
                    y -= w + GAP;
                }
            }
        }
        self.scene.annot = out;
        self.annots = annots;
    }
}


/// Where `paint` puts instances: the scene (with owners and flow tags) or, for hidden nodes, aside.
struct Sink<'a> {
    scene: &'a mut Scene,
    owners: &'a mut Vec<(usize, u64)>,
    tags: &'a mut Vec<Option<Tag>>,
    unseen: &'a mut Vec<(usize, Instance)>,
    counters: &'a mut HashMap<usize, [u32; 2]>,
    latest: &'a mut f32,
    loose: &'a mut bool,
}

/// What a subtree passes down besides fonts: its material override (nesting level and id) and the
/// looseness it holds.
#[derive(Clone)]
struct Inherit {
    level: u8, mat: Option<u8>, loosen: f32, select: bool,
    /// Ripple settings written above (DESIGN.md §1.15), and whether this is inside a control or a text field.
    respond: Option<Response>, legible: Option<bool>, scope: Option<Scope>, recompose: Option<bool>, control: bool, field: bool,
    /// What text and controls, and what everything else, do when nothing above says (the look's roles).
    roles: [Response; 2],
    /// Right to left (`El::rtl`).
    rtl: bool,
    /// Drawn as a skeleton (`El::skeleton`).
    skeleton: bool,
    /// Inside a preview (`El::preview`).
    preview: bool,
    /// Inside a cached subtree (`El::cached`): its text's faces carry `text::FINE`.
    fine: bool,
}
impl Default for Inherit {
    fn default() -> Self {
        Self { level: 0, mat: None, loosen: 0.0, select: false, respond: None, legible: None, scope: None, recompose: None, control: false, field: false, roles: [Response::Nudge, Response::Loosen], rtl: false, skeleton: false, preview: false, fine: false }
    }
}
impl Inherit {
    fn with(self, mat: Option<u8>, loosen: f32) -> Self {
        let (level, mat) = match mat { Some(m) => (self.level + 1, Some(m)), None => (self.level, self.mat) };
        Self { level, mat, loosen: self.loosen.max(loosen), ..self }
    }
    fn code(&self) -> f32 { self.mat.map_or(0.0, |m| crate::style::override_code(self.level, m)) }
}

/// An instance's `resp` that fronts pass by (see `Instance::resp`; the negative push tells `paint` to
/// keep it).
const IGNORE: [f32; 4] = [0.0, -1.0, 0.0, 0.0];
/// A decode transition running over one element (DESIGN.md §1.28): forward when it appeared, backwards
/// over its last instances when it went.
/// How long what comes and goes fades when the viewer asked for less motion, s, until a flow says
/// otherwise (`FlowSpec::fade_ms`).
pub const REDUCED_FADE: f32 = 0.15;
/// How long a face arriving takes to sweep along a line, s (DESIGN.md §1.6.5).
pub const SWEEP: f32 = 0.24;
/// How long before and after its change a glyph loosens and draws back in, s.
pub const SWEEP_LOOSE: f32 = 0.06;
/// The least time between two frames that only a plate's ageing asks for, s (30 a second).
pub const PLATE_FRAME: f32 = 1.0 / 30.0;


struct DecodeRun { start: f32, reverse: bool, spec: crate::decode::Decode, plan: Option<crate::decode::Plan>, ghost: Vec<Instance>, clip: Option<[f32; 4]>, drawn: Option<f32> }
/// What a decoded element showed in the last frame: its instances, its own spec and its clip (device
/// px), so it goes as it came, inside the box it was seen in.
struct Shown { insts: Vec<Instance>, spec: crate::decode::Decode, clip: [f32; 4] }
use crate::decode::Decode;

/// What the field may do to a node (DESIGN.md §1.15): how much looseness fronts may add (0 when it is
/// ignored or only nudged; `legible_cap` for legible elements that loosen; else 1), how loose a press held
/// or a pull may make it (0 when ignored, `legible_cap` when legible, else 1), and its instances' `resp`.
/// Whether the pointer operates node `i` or something around it (presses, drags, picks or hears moves):
/// the lens stays out of the way of what is being used.
fn operated<M>(nodes: &[Node<M>], i: usize) -> bool {
    let mut k = Some(i);
    while let Some(j) = k {
        let n = &nodes[j];
        if n.active() || n.on_pick.is_some() || n.on_move.is_some() { return true; }
        k = n.parent;
    }
    false
}

fn response_caps<M>(n: &Node<M>, legible_cap: f32) -> (f32, f32, [f32; 4]) {
    let cap = if n.legible { legible_cap } else { 1.0 };
    let (front, own) = match n.respond { Response::Ignore => (0.0, 0.0), Response::Nudge => (0.0, cap), Response::Loosen => (cap, cap) };
    let push = if n.respond == Response::Ignore { 0.0 } else { 1.0 };
    (front, own, [front, push, 0.0, 0.0])
}

impl TextRec {
    /// The same text without its glyphs.
    fn clone_head(&self) -> TextRec { TextRec { s: self.s.clone(), face: self.face, size: self.size, line: self.line, width: self.width, print: self.print, track: self.track, origin: self.origin, glyphs: vec![], changed: self.changed } }
}

/// The part of a glyph instance inside box `b` (x0, y0, x1, y1, device px), as an instance of its own:
/// the same glyph, its atlas coordinates cut to match. `None` when they do not overlap.
fn piece(i: &Instance, b: [f32; 4]) -> Option<Instance> {
    let r = i.rect;
    let (x0, y0, x1, y1) = (r[0].max(b[0]), r[1].max(b[1]), (r[0] + r[2]).min(b[2]), (r[1] + r[3]).min(b[3]));
    if x1 - x0 < 1e-3 || y1 - y0 < 1e-3 || r[2] <= 0.0 || r[3] <= 0.0 { return None; }
    let mut p = *i;
    let (du, dv) = ((i.uv[2] - i.uv[0]) / r[2], (i.uv[3] - i.uv[1]) / r[3]);
    p.rect = [x0, y0, x1 - x0, y1 - y0];
    p.uv = [i.uv[0] + (x0 - r[0]) * du, i.uv[1] + (y0 - r[1]) * dv, i.uv[0] + (x1 - r[0]) * du, i.uv[1] + (y1 - r[1]) * dv];
    Some(p)
}

/// A tile that will fly in a recomposition: where its top-left is (device px) and what it shows, as
/// instances placed relative to that corner.
type Unit = ([f32; 2], Vec<Instance>);

/// Where a recomposition tile is `clock` seconds into its run and the pieces it shows there (whichever
/// side it shows more of), for a retarget; `None` when it shows nothing.
fn tile_now(t: &RTile, clock: f32) -> Option<Unit> {
    use morph::kind;
    let u = morph::progress(&t.p, clock, Ease::InOutCubic);
    let at = morph::position(&t.p, u, 0.0);
    let k = t.p.misc[0];
    let shows_dst = if k == kind::DST_DUST { morph::alpha(k, u) > 0.5 } else { u >= t.p.misc[3] };
    let list = if shows_dst { &t.dst } else { &t.src };
    (!list.is_empty()).then(|| (at, list.clone()))
}

/// The tiles of a recomposition: the tiles in flight (`prev`) and the cells of the changed old glyphs
/// (`src`) fly to the cells of the changed new glyphs (`dst`), paired by Morton rank as a morph pairs
/// (DESIGN.md §1.10, §1.16). Cells are cut from whole glyphs only; the tile grows by whole device px
/// until each side has at most `spec.count_max` cells.
/// With `appear` (a clamp letting go: the new glyphs have nothing to come from), the new glyphs that no
/// tile flies to come in whole, cell by cell in reading order from the first frame, instead of fading
/// in at the end of the flight.
fn recompose_tiles(prev: Vec<Unit>, src: &[Instance], dst: &[Instance], spec: &MorphSpec, atlas: &[u8], appear: bool) -> Vec<RTile> {
    use morph::kind;
    let mut tile = spec.lattice.max(1.0);
    let (a, b) = loop {
        let (a, b) = (morph::cells(src, tile, atlas), morph::cells(dst, tile, atlas));
        let n = (a.len() + prev.len()).max(b.len()) as f32;
        if n <= spec.count_max.max(1.0) || tile >= 64.0 { break (a, b); }
        tile = (tile * (n / spec.count_max.max(1.0)).sqrt()).ceil().max(tile + 1.0);
    };
    let cut = |insts: &[Instance], x: f32, y: f32| -> Vec<Instance> {
        insts.iter().filter_map(|i| piece(i, [x, y, x + tile, y + tile]).map(|mut p| { p.rect[0] -= x; p.rect[1] -= y; p })).collect()
    };
    let th = spec.threshold;
    let (mut su, mut sdust): (Vec<Unit>, Vec<Unit>) = (prev, vec![]);
    for c in &a {
        let p = cut(src, c.x, c.y);
        if p.is_empty() { continue; }
        if c.cover >= th { su.push(([c.x, c.y], p)); } else { sdust.push(([c.x, c.y], p)); }
    }
    let (mut tu, mut tdust): (Vec<Unit>, Vec<Unit>) = (vec![], vec![]);
    for c in &b {
        let p = cut(dst, c.x, c.y);
        if p.is_empty() { continue; }
        if c.cover >= th { tu.push(([c.x, c.y], p)); } else { tdust.push(([c.x, c.y], p)); }
    }
    // Nothing to pair with: everything stays where it is and fades.
    if su.is_empty() || tu.is_empty() { sdust.append(&mut su); tdust.append(&mut tu); }
    let order = |v: &mut Vec<Unit>| {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (p, _) in v.iter() { x0 = x0.min(p[0]); y0 = y0.min(p[1]); x1 = x1.max(p[0]); y1 = y1.max(p[1]); }
        let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        v.sort_by_key(|(p, _)| (morph::morton((p[0] - x0) / w, (p[1] - y0) / h), p[1] as i64, p[0] as i64));
    };
    order(&mut su);
    order(&mut tu);
    let flight = spec.duration_ms / 1000.0;
    let stagger = spec.stagger.clamp(0.0, 0.9);
    let dur = flight * (1.0 - stagger);
    let seed = |n: usize| (morph::hash(n as u32 ^ 0x9e37_79b9) >> 8) as f32 / 16_777_216.0;
    let delay = |sd: f32| stagger * flight * 0.95 * sd;
    let mut out = Vec::with_capacity(su.len().max(tu.len()) + sdust.len() + tdust.len());
    for (n, (i, j, k)) in morph::pair(su.len(), tu.len()).into_iter().enumerate() {
        let sd = seed(n);
        let (s, t) = (&su[i], &tu[j]);
        let p = Particle { ab: [s.0[0], s.0[1], t.0[0], t.0[1]], run: [0.0, 1.0, delay(sd), dur], misc: [k, tile, sd, 0.3 + 0.4 * seed(n + 7_777_777)] };
        out.push(RTile { p, src: s.1.clone(), dst: t.1.clone() });
    }
    let n0 = out.len();
    for (n, (at, list)) in sdust.into_iter().enumerate() {
        let sd = seed(n0 + n);
        out.push(RTile { p: Particle { ab: [at[0], at[1], at[0], at[1]], run: [0.0, 1.0, delay(sd), dur], misc: [kind::SRC_DUST, tile, sd, 0.5] }, src: list, dst: vec![] });
    }
    let n0 = out.len();
    if appear && n0 == 0 {
        // In the order of the glyphs (`dst` is in reading order), then top to bottom and left to right
        // within one; each cell shows its pieces at once from its turn.
        // A cell is the glyph's whose centre is nearest (glyph boxes carry a margin and overlap).
        let glyph = |at: [f32; 2]| {
            let c = [at[0] + tile * 0.5, at[1] + tile * 0.5];
            let d = |i: &Instance| (i.rect[0] + i.rect[2] * 0.5 - c[0]).powi(2) + (i.rect[1] + i.rect[3] * 0.5 - c[1]).powi(2);
            (0..dst.len()).min_by(|&a, &b| d(&dst[a]).total_cmp(&d(&dst[b]))).unwrap_or(0)
        };
        let mut cells: Vec<(usize, Unit)> = tdust.into_iter().map(|u| (glyph(u.0), u)).collect();
        cells.sort_by(|a, b| (a.0, a.1.0[1] as i64, a.1.0[0] as i64).cmp(&(b.0, b.1.0[1] as i64, b.1.0[0] as i64)));
        let n = cells.len().max(1) as f32;
        for (k, (_, (at, list))) in cells.into_iter().enumerate() {
            let on = flight * 0.9 * k as f32 / n;
            out.push(RTile { p: Particle { ab: [at[0], at[1], at[0], at[1]], run: [0.0, 1.0, on, 1.0 / 480.0], misc: [kind::DST_DUST, tile, seed(k), 0.5] }, src: vec![], dst: list });
        }
        return out;
    }
    for (n, (at, list)) in tdust.into_iter().enumerate() {
        let sd = seed(n0 + n);
        out.push(RTile { p: Particle { ab: [at[0], at[1], at[0], at[1]], run: [0.0, 1.0, delay(sd), dur], misc: [kind::DST_DUST, tile, sd, 0.5] }, src: vec![], dst: list });
    }
    out
}

/// What a recomposition tile draws `clock` seconds into its run: the old glyphs' pieces, then (past the
/// tile's switch point) the new glyphs' pieces, where the tile is (`morph::progress` and
/// `morph::position`, in a straight line). At progress 0 that is exactly the old glyphs, at 1 exactly the
/// new ones. With `settled`, fades are rounded to on or off (where a retarget starts from).
fn tile_pieces(t: &RTile, clock: f32, settled: bool) -> Vec<Instance> {
    use morph::kind;
    let u = morph::progress(&t.p, clock, Ease::InOutCubic);
    let at = morph::position(&t.p, u, 0.0);
    let k = t.p.misc[0];
    let gate = morph::alpha(k, u);
    // A flying tile shows the old pieces until its own switch point (`misc[3]`, 0.3–0.7), then the new
    // ones, at full strength: pixels move and swap rather than fade, so the text stays crisp.
    let gate = if settled { if gate > 0.5 { 1.0 } else { 0.0 } } else { gate };
    let (sa, da) = if k == kind::SRC_DUST { (gate, 0.0) } else if k == kind::DST_DUST { (0.0, gate) }
                   else if u < t.p.misc[3] { (gate, 0.0) } else { (0.0, gate) };
    let mut out = vec![];
    for (list, a) in [(&t.src, sa), (&t.dst, da)] {
        if a < 1.0 / 512.0 { continue; }
        for p in list {
            let mut i = *p;
            i.rect[0] += at[0];
            i.rect[1] += at[1];
            i.paint[2] *= a;
            // Flying tiles do not read the field map: the hand-over must match the first stage exactly.
            i.look[2] = 0.0;
            out.push(i);
        }
    }
    out
}

/// One side of a morph with 3D elements, as cells for a tile size: the screen cells of everything but
/// the meshes, and a cell on every surface point of each mesh (tagged `emitter << 24 | point`).
fn mesh_side<'a>(insts: &[Instance], ems: &'a [(String, Rc<crate::mesh::Scene3d>, [f32; 4])], base: u32, lib: &'a crate::mesh::Library, atlas: &'a [u8], cap: f32) -> impl FnMut(f32) -> Vec<morph::Cell> + 'a {
    let flat: Vec<Instance> = insts.iter().filter(|i| i.paint[3] != MESH).copied().collect();
    move |t: f32| {
        let mut cs = morph::cells(&flat, t, atlas);
        for (e, (_, sc, r)) in ems.iter().enumerate() {
            for (k, sp) in sc.surface(lib, surface_count(r, t, cap), 0x51ab).iter().enumerate() {
                if let Some((fx, fy, _)) = sc.project(lib, sp, r[2] / r[3].max(1.0)) {
                    cs.push(morph::Cell { x: r[0] + fx * r[2] - t * 0.5, y: r[1] + fy * r[3] - t * 0.5, cover: 1.0, tag: (base + e as u32) << 24 | k as u32 });
                }
            }
        }
        cs
    }
}

/// How many surface points a 3D element of device rect `r` emits at tile size `t`: enough that the
/// half facing the camera covers its silhouette, at most `cap`.
fn surface_count(r: &[f32; 4], t: f32, cap: f32) -> usize {
    ((r[2] * r[3] / (t * t).max(1.0)) * 1.4).min(cap.max(1.0)).max(16.0) as usize
}

/// Whether node `i` is `root` or inside it.
fn within<M>(nodes: &[Node<M>], mut i: usize, root: usize) -> bool {
    loop {
        if i == root { return true; }
        match nodes[i].parent { Some(p) => i = p, None => return false }
    }
}

fn ident(key: &str, part: u32, glyph: u32) -> u64 {
    let mut h = DefaultHasher::new();
    (key, part, glyph).hash(&mut h);
    h.finish()
}

/// A paragraph whose face is arriving, `at` seconds into the change, with `d` the stand-in it had: the
/// new face's glyphs where the sweep along their line has passed (it takes `SWEEP` from the line's
/// start to its end), the stand-in's glyph for the same character, standing where the new one is,
/// where it has not; each glyph's looseness within `SWEEP_LOOSE` of its change, a sine up to `cap`.
/// With less motion both are drawn where the new ones are, the new fading in over `REDUCED_FADE`.
#[allow(clippy::too_many_arguments)]
fn sweep_glyphs(text: &mut Text, para: Para<'_>, origin: (f32, f32), proto: Instance, s: f32, rows: Option<(f32, f32)>, (at, d): (f32, crate::text::FaceDecl), reduced: bool, cap: f32) -> Vec<(Instance, u32, f32)> {
    let mut new: Vec<(Instance, u32, [f32; 5])> = vec![];
    text.emit_pens(para, origin, proto, s, rows, |i, g, p| new.push((i, g, p)));
    let stand = Para { face: d.stand_in, size: para.size * d.scale, width: f32::INFINITY, around: None, ..para };
    let mut old: Vec<(Instance, u32, [f32; 5])> = vec![];
    text.emit_pens(stand, origin, proto, s, None, |i, g, p| old.push((i, g, p)));
    let pixel = text.is_pixel(d.stand_in);
    // The stand-in's glyph for the new glyph `k` (in text order), moved to where that one is.
    let moved = |k: usize| -> Option<(Instance, u32)> {
        if old.is_empty() { return None; }
        let j = if old.len() == new.len() { k } else { (k * old.len() / new.len().max(1)).min(old.len() - 1) };
        let (mut i, g, p) = old[j];
        let (dx, dy) = (new[k].2[0] - p[0], new[k].2[1] - p[1]);
        let dx = if pixel { dx.round() } else { dx };
        i.rect[0] += dx;
        i.rect[1] += dy.round();
        Some((i, g))
    };
    let mut out = vec![];
    for (k, &(inst, g, p)) in new.iter().enumerate() {
        if reduced {
            let a = (at / REDUCED_FADE).clamp(0.0, 1.0);
            if a < 1.0 && let Some((mut o, og)) = moved(k) { o.paint[2] *= 1.0 - a; out.push((o, og, 0.0)); }
            let mut n = inst;
            n.paint[2] *= a;
            out.push((n, g, 0.0));
            continue;
        }
        let when = SWEEP * ((p[0] - p[2]) / p[3]).clamp(0.0, 1.0);
        let dt = at - when;
        let l = if cap > 0.0 && dt.abs() < SWEEP_LOOSE { cap * (std::f32::consts::PI * (dt + SWEEP_LOOSE) / (2.0 * SWEEP_LOOSE)).sin() } else { 0.0 };
        match (dt >= 0.0, moved(k)) {
            (false, Some((o, og))) => out.push((o, og, l)),
            _ => out.push((inst, g, l)),
        }
    }
    out
}

/// Lay out the taffy tree from `root` in `space`, each leaf measured by what it shows (`leaves`, by the
/// node index each taffy leaf carries: its content and line clamp).
fn compute_layout(tree: &mut taffy::TaffyTree<usize>, root: NodeId, space: Size<AvailableSpace>, text: &mut Text, leaves: &[(&Content, u16)]) -> Result<(), taffy::TaffyError> {
    let measure = |input: taffy::LayoutInput, _: NodeId, ctx: Option<&mut usize>, style: &Style| {
        let none = |_: *const (), _: f32| 0.0;
        let ctx_i = ctx.as_ref().map(|i| **i);
        let mut out = match ctx.map(|i| leaves[*i].0) {
            Some(Content::Text { s, size, line, font, around, nowrap, track, .. }) => taffy::compute_leaf_layout(input, style, none, |known, avail| {
                let limit = known.width.or(match avail.width { AvailableSpace::Definite(w) => Some(w), AvailableSpace::MinContent => Some(0.0), AvailableSpace::MaxContent => None });
                // Text flowing around obstacles fills whatever width it is given; text kept on one
                // line is as wide as it is.
                let limit = if around.is_some() { limit.filter(|w| *w > 0.0).or(Some(600.0)) } else if *nowrap { None } else { limit };
                // Clamped to lines: measured as it is shown (the width it is drawn at is one more).
                let clamp = ctx_i.map_or(0, |i| leaves[i].1);
                let cut = (clamp > 0 && around.is_none() && !*nowrap).then_some(()).and(limit).and_then(|w| text.clamp_lines(Para { s, face: font.resolve([0; 3]), size: *size, line: *line, width: w + 1.0, print: 0.0, around: None, track: *track }, clamp));
                let s = cut.as_deref().unwrap_or(s);
                let (w, h) = text.measure_around(s, font.resolve([0; 3]), *size, *line, limit, crate::text::Setting { around: around.as_deref(), track: *track });
                Size { width: known.width.unwrap_or(w), height: known.height.unwrap_or(h) }
            }),
            Some(Content::Picture { ratio, .. }) => taffy::compute_leaf_layout(input, style, none, |known, avail| {
                let w = known.width.or(avail.width.into_option()).unwrap_or(0.0);
                Size { width: w, height: known.height.unwrap_or((w * ratio).round()) }
            }),
            Some(Content::Canvas(d)) => taffy::compute_leaf_layout(input, style, none, |known, avail| {
                let w = known.width.or(avail.width.into_option()).unwrap_or(d.w);
                Size { width: w, height: known.height.unwrap_or((w * d.h / d.w.max(1e-3)).round()) }
            }),
            _ => taffy::compute_leaf_layout(input, style, none, |_, _| Size::ZERO),
        };
        // A box whose children all went is laid out here as a leaf: a positioning area makes
        // taffy drop the absolute children it held, which would otherwise outlive their nodes.
        out.oof_positioning_area = Some(taffy::OofPositioningArea { size: Size::ZERO, offset: taffy::Point::ZERO });
        out
    };
    tree.compute_layout_with_measure(root, space, measure)
}
/// Whether text `b` measures as `a` does, so a layout of one is a layout of the other: the same size
/// at its widest, at its narrowest and at the width `w` it was laid out at (a label beside a stage
/// whose number changed, in figures of one width).
fn same_measure(text: &mut Text, a: &Content, b: &Content, w: f32) -> bool {
    let (Content::Text { s: sa, size, line, font, around: None, nowrap, track, .. }, Content::Text { s: sb, size: size2, line: line2, font: font2, around: None, nowrap: nowrap2, track: track2, .. }) = (a, b) else { return false };
    if size != size2 || line != line2 || font != font2 || nowrap != nowrap2 || track != track2 { return false; }
    let face = font.resolve([0; 3]);
    let setting = crate::text::Setting { around: None, track: *track };
    [None, Some(0.0), Some(w.max(0.0))].into_iter().all(|limit| {
        let limit = if *nowrap { None } else { limit };
        let (x, y) = (text.measure_around(sa, face, *size, *line, limit, setting), text.measure_around(sb, face, *size, *line, limit, setting));
        (x.0 - y.0).abs() < 0.01 && (x.1 - y.1).abs() < 0.01
    })
}
/// Whether a subtree has an element at its own size (`El::own_size`).
fn has_own<M>(el: &El<M>) -> bool { el.own_size.is_some() || el.children.iter().any(has_own) }
/// Whether a view has a box marked `El::fit`.
fn has_fit<M>(el: &El<M>) -> bool { el.fit.is_some() || el.children.iter().any(has_fit) }
/// For the pass that finds natural sizes: each fitted subtree out of the flow, at the box's top left,
/// as large as it would be on its own.
/// A box that reflows is measured in place instead, as it will be drawn, zoomed by the scale it was
/// last probed at (`prev`, in order; 1 on the first pass).
fn fit_probe<M>(el: &mut El<M>, prev: &[f32], at: &mut usize) {
    // The subtree is measured at its own size: in a wide holder at the box's corner, where nothing
    // narrower than its content makes it wrap.
    if let Some(f) = el.fit {
        let k = prev.get(*at).copied().unwrap_or(1.0);
        *at += 1;
        if let Some(c) = el.children.first_mut() {
            if f.mode == crate::el::FitMode::Reflow {
                // In place, as it will be drawn, at the scale it was last probed at.
                if (k - 1.0).abs() > 1e-4 { let sub = std::mem::replace(c, crate::el::space(0.0, 0.0)).zoom(k); *c = sub; }
            } else {
                let mut holder = crate::el::col();
                holder.style.position = taffy::Position::Absolute;
                holder.style.align_items = AlignItems::START;
                holder.style.inset = taffy::Rect { left: length(0.0), top: length(0.0), right: auto(), bottom: auto() };
                holder.style.size.width = length(1.0e5);
                let mut sub = std::mem::replace(c, crate::el::space(0.0, 0.0));
                // With own-size elements, measured again zoomed (`Ui::fit_scales`).
                if (k - 1.0).abs() > 1e-4 && has_own(&sub) { sub = sub.zoom(k); }
                *c = holder.child(sub);
            }
        }
    }
    for c in &mut el.children { fit_probe(c, prev, at); }
}
/// How a box marked `El::fit` zooms its subtree: the scale, where it goes (logical px from the box), and
/// each pixel text's own size there (its text, its size at 1, the size it is set at).
/// A box that reflows keeps its subtree in the flow (`w`: its content box, set for those).
/// `nat`: for a subtree that holds own-size elements (`El::own_size`), its size as first measured at 1.
#[derive(Clone)]
struct Fitted { k: f32, at: (f32, f32), pixel: Vec<(String, f32, f32)>, w: Option<(f32, f32)>, nat: Option<(f32, f32)> }
/// Each fitted subtree zoomed by its scale and put where it goes in its box (`Ui::fit_scales`, in the
/// same order).
fn fit_apply<M>(el: &mut El<M>, ks: &[Fitted]) {
    fn snap<M>(el: &mut El<M>, k: f32, pixel: &[(String, f32, f32)]) {
        if let Content::Text { s, size, line, .. } = &mut el.content && let Some(p) = pixel.iter().find(|p| p.0 == *s && (p.1 * k - *size).abs() < 1e-3) {
            // Set larger than the scale makes it (a pixel face is never under its native size): its lines
            // as much further apart, so wrapped lines do not overlap and the box holds them.
            if p.2 > *size + 1e-4 && *size > 0.0 { *line *= p.2 / *size; }
            *size = p.2;
        }
        for c in &mut el.children { snap(c, k, pixel); }
    }
    fn go<M>(el: &mut El<M>, ks: &[Fitted], at: &mut usize) {
        if el.fit.is_some() {
            let (k, (x, y), pixel, w) = ks.get(*at).map_or((1.0, (0.0, 0.0), &[][..], None), |f| (f.k, f.at, &f.pixel[..], f.w));
            *at += 1;
            if let Some(c) = el.children.first_mut() {
                let mut z = std::mem::replace(c, crate::el::space(0.0, 0.0));
                if (k - 1.0).abs() > 1e-4 || w.is_none() { z = z.zoom(k); }
                snap(&mut z, k, pixel);
                // A reflowing subtree stays in the flow, zoomed in place, as it was measured.
                if w.is_none() {
                    z.style.position = taffy::Position::Absolute;
                    z.style.inset = taffy::Rect { left: length(x), top: length(y), right: auto(), bottom: auto() };
                }
                *c = z;
            }
        }
        for c in &mut el.children { go(c, ks, at); }
    }
    let mut at = 0;
    go(el, ks, &mut at);
}
/// Take every modal element out of the tree (they are laid over the window instead).
fn hoist<M>(el: &mut El<M>, out: &mut Vec<El<M>>) {
    let kids = std::mem::take(&mut el.children);
    for mut c in kids {
        if c.preview { in_preview(&mut c); el.children.push(c); continue; }
        if c.modal.is_some() { out.push(c); continue; }
        hoist(&mut c, out);
        el.children.push(c);
    }
}

/// A preview's subtree (`El::preview`): its modal elements stay where they are written, as ordinary
/// elements (no layer, nothing held loose beneath, no dismiss, no trapped focus, not modal to assistive
/// technology), and what is shown only with focus is always shown.
/// (What is shown only with focus is drawn over its siblings there: `flatten`.)
fn in_preview<M>(el: &mut El<M>) {
    el.modal = None;
    for c in &mut el.children { in_preview(c); }
}

/// Whether a ramp stroke goes from one accent hue to another (a stop with no accent has no hue).
fn ramp_hues(q: &Instance) -> bool {
    let code = -q.extra[0] - 1.0;
    let hues = code - (code / 4096.0).floor() * 4096.0;
    let (mh, fh) = ((hues / 64.0).floor(), hues - (hues / 64.0).floor() * 64.0);
    let accent = |v: f32| { let n = -v - 1.0; ((n - (n / 65536.0).floor() * 65536.0) / 256.0).floor() > 0.0 };
    let mut used = vec![];
    if q.paint[1] > 0.0 { used.push(q.look[1]); }
    if accent(q.uv[2]) { used.push(fh); }
    if q.uv[3] < -0.5 && accent(q.uv[3]) { used.push(mh); }
    used.windows(2).any(|w| w[0] != w[1])
}
/// A drawing's paint's hue as drawn (`Drawing::palette`): one of its own colours is its place in the
/// frame's table `own` (added if new), after the theme's three; past the palette or the table it is hue 0.
#[inline(never)]
fn own_hue(p: Paint, palette: &[[f32; 3]], own: &mut Vec<[f32; 3]>) -> f32 {
    let Some(c) = p.hue.checked_sub(3).and_then(|k| palette.get(k as usize)) else { return p.theme_hue() };
    let at = own.iter().position(|o| o == c).or_else(|| (own.len() < crate::draw::OWN_COLOURS).then(|| { own.push(*c); own.len() - 1 }));
    at.map_or(0.0, |k| 3.0 + k as f32)
}
/// A ramp's paint in one number (`Drawing::ramp`, the shader's `ramp_paint`): tone, accent and opacity
/// in 255ths, 8 bits each, exact in an `f32`.
fn ramp_pack(p: Paint) -> f32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round();
    q(p.tone) + 256.0 * q(p.accent) + 65536.0 * q(p.alpha)
}
/// What a stack's child `j` adds to its left and right margins, from the widths `o` its children take
/// (margins included): the first takes the widest one's room, centred in it, and each after it is
/// pulled back over that room, centred too, and takes none of its own.
fn stack_extra(o: &[f32], j: usize) -> (f32, f32) {
    let w = o.iter().fold(0.0f32, |a, &b| a.max(b));
    let half = (w - o.get(j).copied().unwrap_or(0.0)) / 2.0;
    if j == 0 { (half, half) } else { (half - w, half) }
}
/// A margin with `e` more (a margin that is not a length becomes `e`).
/// How far an element moved when its new style differs from the old only in an absolutely placed
/// element's offsets, each side a length before and after or the same (logical px); `None` otherwise.
fn moved_only(old: &Style, new: &Style) -> Option<(f32, f32)> {
    if old == new { return Some((0.0, 0.0)); }
    if old.position != taffy::Position::Absolute || new.position != taffy::Position::Absolute { return None; }
    let mut o = old.clone();
    o.inset = new.inset;
    if o != *new { return None; }
    use taffy::style::ExpandedLengthPercentageAuto as E;
    // Along one axis: the near side's change, or the far side's (which moves it the other way), the
    // other side auto.
    let axis = |a0: LengthPercentageAuto, a1: LengthPercentageAuto, b0: LengthPercentageAuto, b1: LengthPercentageAuto| -> Option<f32> {
        match (a0.expand(), a1.expand()) {
            // With both sides set the box would change size: not a move.
            (E::Length(x), E::Length(y)) => (b0 == b1 && matches!(b0.expand(), E::Auto)).then_some(y - x),
            _ if a0 != a1 => None,
            _ if matches!(a0.expand(), E::Auto) => match (b0.expand(), b1.expand()) { (E::Length(x), E::Length(y)) => Some(x - y), _ => (b0 == b1).then_some(0.0) },
            _ => (b0 == b1).then_some(0.0),
        }
    };
    Some((axis(old.inset.left, new.inset.left, old.inset.right, new.inset.right)?, axis(old.inset.top, new.inset.top, old.inset.bottom, new.inset.bottom)?))
}
fn add_margin(m: LengthPercentageAuto, e: f32) -> LengthPercentageAuto {
    match m.expand() { taffy::style::ExpandedLengthPercentageAuto::Length(x) => length(x + e), _ => length(e) }
}
/// What `flatten` needs to build a virtual list's rows (scroll offsets, viewport heights from the last
/// layout, the window's height) and the row ranges it built.
struct VirtCtx<'a> { scroll: &'a HashMap<String, f32>, views: &'a HashMap<String, f32>, window: f32, made: HashMap<String, (usize, usize)> }
/// Per node from node `base` on, what goes to the taffy tree: its style and its children.
#[derive(Default)]
struct Built { styles: Vec<Style>, kids: Vec<Vec<usize>>, base: usize, stacks: Vec<usize> }
/// A stage's subtree viewed alone, by its key (`Ui::stage_view`).
type StageView<S, M> = fn(&S, &str) -> Option<El<M>>;
/// What the last paint drew, kept for a paint that draws only some stages again (`Ui::kept`).
struct Kept {
    roots: Vec<usize>, insts: Vec<Instance>, owners: Vec<(usize, u64)>, tags: Vec<Option<Tag>>, runs: Vec<(usize, usize)>,
    log: Vec<crate::hit::DrawnGlyph>, log_runs: Vec<(usize, usize)>, laid: Vec<crate::hit::Laid>, unseen: Vec<(usize, Instance)>, own: Vec<[f32; 3]>,
    meshes: Vec<crate::scene::MeshDraw>,
}
impl Kept {
    /// Whether node `i` is in a stage drawn again.
    fn has<M>(&self, nodes: &[Node<M>], i: usize) -> bool { self.roots.iter().any(|&r| within(nodes, i, r)) }
}
/// Where each of `count` nodes' entries are in a list made node by node (`(0, 0)`: none); `None` when
/// a node's entries are not one run.
fn runs_of(nodes: impl Iterator<Item = usize>, count: usize) -> Option<Vec<(usize, usize)>> {
    let mut runs = vec![(0usize, 0usize); count];
    for (k, j) in nodes.enumerate() {
        let r = runs.get_mut(j)?;
        if r.1 == r.0 { *r = (k, k + 1); } else if r.1 == k { r.1 = k + 1; } else { return None; }
    }
    Some(runs)
}
/// What a stage's root was flattened with (`El::stage`, `Ui::stage_view`): so its subtree alone can be
/// flattened again in place.
#[derive(Clone)]
struct StageCtx { parent: Option<usize>, path: String, roles: [u8; 3], off: (bool, bool), flow: Option<(Rc<str>, usize)>, inh: Inherit, held: bool, local: bool, part: bool }

/// The rows `first..last` a viewport `h` tall at offset `off` shows.
fn visible_rows(off: f32, h: f32, count: usize, row_h: f32) -> (usize, usize) {
    let rh = row_h.max(1.0);
    let first = ((off / rh).floor().max(0.0) as usize).min(count);
    let last = (((off + h) / rh).ceil().max(0.0) as usize).min(count);
    (first, last.max(first))
}

/// What a leaf's measured size depends on: when it is unchanged, taffy's cached size stands.
fn measure_key(c: &Content) -> u64 {
    let mut h = DefaultHasher::new();
    match c {
        Content::Text { s, size, line, font, around, nowrap, .. } => {
            (s, size.to_bits(), line.to_bits(), font.resolve([0; 3]), around.as_ref().map_or(0, |a| a.id()), nowrap).hash(&mut h);
        }
        Content::Picture { ratio, .. } => ratio.to_bits().hash(&mut h),
        // Sized by its style alone.
        Content::Scene3d(_) | Content::Cells(_) | Content::CachedView { .. } => 3u8.hash(&mut h),
        Content::Canvas(d) => (d.h / d.w.max(1e-3)).to_bits().hash(&mut h),
        Content::None => 0u8.hash(&mut h),
    }
    h.finish()
}

/// Move an element tree into flat nodes (paint order). Unkeyed nodes are keyed by path.
/// Font roles are inherited: each text leaf's role is resolved here to the face of its nearest
/// `fonts` ancestor. So is being disabled: a disabled subtree has no handlers and cannot take focus.
/// `off` is (disabled, hidden), both inherited; `flow` is the nearest flow id above and its node.
/// A virtual list's rows are built here, for what its viewport shows.
#[allow(clippy::too_many_arguments)]
fn flatten<M>(el: El<M>, parent: Option<usize>, path: String, roles: [u8; 3], off: (bool, bool), flow: Option<(Rc<str>, usize)>, inh: Inherit, vx: &mut VirtCtx, built: &mut Built, nodes: &mut Vec<Node<M>>) -> usize {
    let stage_at = (el.props.stage || el.props.steps || el.props.local_messages).then(|| Box::new(StageCtx { parent, path: path.clone(), roles, off, flow: flow.clone(), inh: inh.clone(), held: el.props.stage, local: el.props.local_messages, part: el.props.steps }));
    let El { props: crate::el::Props { key, mut style, mut content, fill, fill_on, border, radius, mut focusable, tab_order, skip_to, focus_shown, scroll, horizontal, bar, reveal, disabled, mut commit, fonts, loose, wipe, flow: own_flow, hidden, shift, shed, flow_field, stirr, trace, annotate, gaze_scope, material, loosen, anchor, clips, calm, map_life, multiline, selectable, cursor, ripple, guide, skeleton, rtl, decode, form, turn, reveal_by, ongoing, pass, opacity, commit_scope, preview, fit, clamp, plate, cached, clip_radius, own_size, grain, bloom, show_clear, pen, view_scale, stage: _, stack, steps: _, local_messages: _ }, mut on_press, mut on_key, mut on_drag, mut on_blur, modal, mut on_text, virt, access, mut on_pick, mut on_wheel, mut on_move, on_visible, on_outline, on_pens, mut children } = el;
    // A view scale scales what it holds, not its pen (`El::view_scale`).
    if let Some(s) = view_scale { children = children.into_iter().map(|c| c.zoom_by(s, true)).collect(); }
    let mut inh = inh;
    if let Some(r) = rtl { inh.rtl = r; }
    style.direction = if inh.rtl { taffy::Direction::Rtl } else { taffy::Direction::Ltr };
    let keyed = !key.is_empty();
    let key = if key.is_empty() { path } else { key };
    let roles = fonts.unwrap_or(roles);
    let (off, hidden) = (off.0 || disabled, off.1 || hidden);
    if off || hidden { on_press = None; on_key = None; on_drag = None; on_blur = None; on_text = None; on_pick = None; on_wheel = None; on_move = None; focusable = false; commit = false; }
    // A skeleton takes no input (and is not drawn faint, as a disabled subtree is).
    inh.skeleton |= skeleton;
    // In a preview what is shown only with focus is always shown (and drawn over its siblings, below).
    let in_pv = inh.preview;
    let focus_shown = focus_shown && !in_pv && !preview;
    inh.preview |= preview;
    inh.fine |= cached;
    if inh.skeleton { on_press = None; on_key = None; on_drag = None; on_blur = None; on_text = None; on_pick = None; on_wheel = None; on_move = None; focusable = false; commit = false; }
    if let Content::Text { font, .. } = &mut content { *font = Font::Face(font.resolve(roles) | if inh.fine { crate::text::FINE } else { 0 }); }
    // A drawing's blade heads in a face by role take this subtree's faces (the drawing is copied only then).
    if let Content::Canvas(d) = &mut content && d.marks.iter().any(|m| matches!(m, crate::draw::Mark::Blade(b) if b.head.is_some_and(|h| !matches!(h.font, Font::Face(_))))) {
        for m in &mut Rc::make_mut(d).marks { if let crate::draw::Mark::Blade(b) = m && let Some(h) = &mut b.head { h.font = Font::Face(h.font.resolve(roles)); } }
    }
    if let Content::Cells(c) = &mut content { let f = Font::Face(c.font.resolve(roles)); if c.font != f { Rc::make_mut(c).font = f; } }
    let i = nodes.len();
    let flow = own_flow.map(|f| (Rc::from(f.as_str()), i)).or(flow);
    // A material override nests: a deeper one gets a higher level, so it wins where both draw.
    let mut inh = inh.with(material, loosen);
    inh.select |= selectable;
    // Ripple settings: written ones pass down; otherwise text and controls are nudged and legible,
    // everything else loosens (DESIGN.md §1.15).
    let is_text = matches!(content, Content::Text { .. });
    let own_control = focusable || on_press.is_some() || on_drag.is_some() || on_text.is_some();
    // A control keeps its label readable even in a subtree marked decoration (`legible(false)` on an
    // ancestor, a specimen that comes apart whole): it and what it holds are legible again unless it
    // says otherwise itself. A picture, a drawing or a 3D view that takes presses stays decoration.
    if own_control && ripple.legible.is_none() && inh.legible == Some(false) && !matches!(content, Content::Picture { .. } | Content::Canvas(_) | Content::Scene3d(_)) { inh.legible = None; }
    inh.control |= own_control;
    inh.field |= on_text.is_some();
    if ripple.respond.is_some() { inh.respond = ripple.respond; }
    if ripple.legible.is_some() { inh.legible = ripple.legible; }
    if ripple.scope.is_some() { inh.scope = ripple.scope.clone(); }
    if ripple.recompose.is_some() { inh.recompose = ripple.recompose; }
    // A 3D element is decoration (like a picture) even when it can be turned: fronts loosen it.
    let plain = is_text || (inh.control && !matches!(content, Content::Scene3d(_)));
    let respond = inh.respond.unwrap_or(if plain { inh.roles[0] } else { inh.roles[1] });
    let legible = inh.legible.unwrap_or(plain);
    let recompose = is_text && !inh.field && inh.recompose.unwrap_or(keyed);
    let virt_info = virt.as_ref().map(|v| (v.count, v.row_h, v.selected));
    if let Some(v) = virt {
        // Only the rows the viewport shows, and `overscan` more on each side, are built; a column as tall
        // as all the rows holds them, so the scroll extent is the whole list's.
        let offset = vx.scroll.get(&key).copied().unwrap_or(0.0);
        let h = vx.views.get(&key).copied().unwrap_or(vx.window);
        let (first, last) = visible_rows(offset, h, v.count, v.row_h);
        let (a, b) = (first.saturating_sub(v.overscan), (last + v.overscan).min(v.count));
        vx.made.insert(key.clone(), (a, b));
        // The built rows flow from a padding as tall as the rows above them.
        let mut rows = crate::el::col().key(format!("{key}/rows")).fill_width().height(v.count as f32 * v.row_h);
        rows.style.padding.top = length(a as f32 * v.row_h);
        for r in a..b {
            let mut row = (v.build)(r);
            if row.key.is_empty() { row.key = format!("{key}/row{r}"); }
            row.style.size.height = length(v.row_h);
            row.style.flex_shrink = 0.0;
            rows.children.push(row);
        }
        children = vec![rows];
    }
    let placeholder = NodeId::from(0u64);
    nodes.push(Node {
        key, parent, rect: Rect::default(), clip: Rect::default(), inner: Rect::default(), scroll, horizontal, focusable, tab_order, skip_to, focus_shown, written: 0, preview, disabled: off, extent: 0.0,
        content, fill, fill_on, border, radius, on_press, on_key, on_drag, on_blur, loose, wipe, bar, reveal, commit, modal, hidden, shift, shed, flow_field, stirr, trace, flow: flow.clone(),
        mat: inh.code(), loosen: inh.loosen, annotate, scope: gaze_scope, anchor, clips, calm, map_life, on_text, multiline, selectable: inh.select, cursor,
        virt: virt_info, access, respond, legible, ripple: inh.scope.clone(), ripple_scope: ripple.container || commit_scope, recompose, recompose_always: is_text && ripple.recompose == Some(true), unframed: ripple.unframed, keyed,
        on_pick, on_wheel, on_move, guide, on_visible, on_outline, on_pens, decode, skeleton: inh.skeleton, form, turn, xf: None, reveal_by, ongoing: ongoing || parent.is_some_and(|p| nodes[p].ongoing), pass, opacity, opacity_all: opacity * parent.map_or(1.0, |p| nodes[p].opacity_all), commit_scope, fit, clamp, shown: None, plate, cached, clip_radius, clip_r: 0.0, own_size, grain, bloom, show_clear, view_scale, stage: stage_at, op0: (0.0, 0.0), pen, own_in_flow: own_size.is_some() && style.position != taffy::Position::Absolute, local: taffy::Layout::new(), exact: (0.0, 0.0), tid: placeholder,
    });
    { let n = nodes.last_mut().expect("pushed"); n.op0 = (n.opacity, n.opacity_all); }
    crate::el::snap_style(&mut style);
    // Every element is the box its absolutely placed children are pinned in (all four insets against
    // its padding box, as CSS does for a positioned parent), not the window.
    if style.position == taffy::Position::Static { style.position = taffy::Position::Relative; }
    built.styles.push(style);
    built.kids.push(vec![]);
    if stack { built.stacks.push(nodes.len() - 1); }
    // In a preview what is shown only with focus is drawn over its siblings, as when it has focus (its
    // fill covers the ink beneath it): it is laid out and drawn after them, and keeps its written place
    // for Tab (`Node::written`).
    let mut kids: Vec<(usize, El<M>)> = children.into_iter().enumerate().collect();
    if inh.preview { kids.sort_by_key(|(_, c)| c.focus_shown); }
    for (n, c) in kids {
        let p = format!("{}/{n}", nodes[i].key);
        let k = flatten(c, Some(i), p, roles, (off, hidden), flow.clone(), inh.clone(), vx, built, nodes);
        nodes[k].written = n;
        built.kids[i - built.base].push(k);
    }
    i
}

/// A thread's width in device px: at least half a pixel, and a whole number of pixels (at least one)
/// when the frame is two colours only, so a threshold neither breaks nor thickens it unevenly.
fn line_px(w: f32, binary: bool) -> f32 { if binary { w.round().max(1.0) } else { w.max(0.5) } }

#[cfg(test)]
mod tests {
    use crate::el::{El, Paint, canvas, col, layers, row, text};
    /// A crowded page: thousands of strokes and dots under rows of filled, loosened controls and lines
    /// of text, some nested deep, one stroke over the whole page and one far off it.
    fn crowded(_: &()) -> El<()> {
        let (w, h) = (1200.0, 900.0);
        let mut d = crate::draw::Drawing::new(w, h);
        for k in 0..3000 {
            let (x, y) = ((k * 37 % 1200) as f32, (k * 53 % 900) as f32);
            if k % 3 == 0 { d.dot(x, y, 2.0, Paint::DIM); } else { d.line((x, y), (x + 9.0, y + 4.0), 1.0, Paint::FAINT); }
        }
        d.line((-50.0, -50.0), (w + 50.0, h + 50.0), 3.0, Paint::DIM);
        d.line((1e7, 1e7), (1e7 + 5.0, 1e7), 1.0, Paint::DIM);
        let mut over = col().gap(4.0).pad(8.0, 8.0);
        for r in 0..24 {
            let mut line = row().gap(6.0);
            for c in 0..8 {
                let label = text(format!("cell {r}·{c}"), 11.0 + (c % 3) as f32 * 3.0, Paint::TEXT);
                let inner = if c % 2 == 0 { row().child(row().child(label)) } else { row().child(label) };
                line = line.child(row().key(format!("b{r}/{c}")).fill(Paint::GROUND).loose(if (r + c) % 3 == 0 { 0.0 } else { 0.3 }).on_press(()).pad(4.0, 2.0).child(inner));
            }
            over = over.child(line).child(text("A line of text to read over the strokes, long enough to wrap twice across the page width there.", 12.0, Paint::TEXT));
        }
        col().key("page").width(w).height(h).child(layers([canvas(d).key("strokes").width(w), over]))
    }
    #[test]
    fn holds_and_islands_by_the_grid_and_the_parents_are_the_quadratic_answers() {
        for dpr in [1.0f32, 2.0] {
            let fonts = crate::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts");
            let mut u = super::Ui::new((), crowded, |_, _| {}, fonts, (1200.0 * dpr) as u32, (900.0 * dpr) as u32, dpr);
            u.tick(1.0);
            u.settle();
            assert!(u.scene.instances.len() > 4000, "a crowded page: {}", u.scene.instances.len());
            let (fast, slow) = (u.holds(true), u.holds(false));
            assert!(fast.len() > 500, "dpr {dpr}: {} holds", fast.len());
            assert_eq!(fast, slow, "dpr {dpr}: holds");
            let (fast, slow) = (u.islands(true), u.islands(false));
            assert!(fast.len() > 50, "dpr {dpr}: {} islands", fast.len());
            let bits = |v: &[crate::scene::Instance]| v.iter().map(|i| bytemuck::bytes_of(i).to_vec()).collect::<Vec<_>>();
            assert_eq!(bits(&fast), bits(&slow), "dpr {dpr}: islands");
        }
    }
}
