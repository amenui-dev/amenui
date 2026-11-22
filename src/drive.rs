//! Driving a page from outside, deterministically (DESIGN.md §1.17): the `render` tool and the tests open
//! a recipe (or a page an application adds, `render_with`), play a list of input events on a fixed 60 Hz clock, and read back one
//! frame (PNG) and the laid-out tree (JSON), or a run of frames at a fixed rate (`frames`). Same
//! arguments, same bytes.
use crate::error::{DriveError, LookError, SurfaceError, ViewProblem};
use crate::el::{Key, Mods};
use crate::offscreen::{Offscreen, png_bytes_with};
use crate::style::{Look, Material, THEMES};
use crate::tree::TreeNode;
use crate::ui::{PointerKind, Ui};
use serde::Deserialize;

/// One input event of a script, at `at_ms` on the page's clock (which starts at 0).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Event {
    /// When, ms on the page's clock.
    #[serde(default)]
    pub at_ms: f32,
    /// When, as a place on the look's beat (`"3.2.1"`: bar 3, beat 2, step 1; DESIGN.md §1.32), in
    /// place of `at_ms`. Refused when the look has no beat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(flatten)]
    pub input: Input,
}

/// What happens. A pointer event names a point (`x`, `y`, logical px) or an element (`target`, its key:
/// the centre of its box).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Input {
    Press { x: Option<f32>, y: Option<f32>, target: Option<String>, #[serde(default)] shift: bool },
    Release { x: Option<f32>, y: Option<f32>, target: Option<String> },
    Move { x: Option<f32>, y: Option<f32>, target: Option<String> },
    /// A key by its DOM name ("Enter", "ArrowUp", "a", " ", "Tab", "Escape", "F2").
    Key { key: String, #[serde(default)] shift: bool, #[serde(default)] cmd: bool, #[serde(default)] alt: bool },
    Wheel { x: Option<f32>, y: Option<f32>, target: Option<String>, #[serde(default)] dx: f32, #[serde(default)] dy: f32 },
    /// Text typed into the focused field.
    Text { text: String },
    /// A command for the runtime, as an application's `on_commands` would give it (`morph::Cmd`).
    Cmd { cmd: CmdSpec },
}

/// The commands a script can send (DESIGN.md §1.32), in JSON as `{"ripplr_at": {"key": "title",
/// "strength": 1}}`, `"retrigger"` and so on.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CmdSpec {
    /// `Cmd::Ripplr`: a front from (x, y), logical px.
    Ripplr { x: f32, y: f32, #[serde(default = "one")] strength: f32 },
    /// `Cmd::RipplrAt`: a front from an element's centre.
    RipplrAt { key: String, #[serde(default = "one")] strength: f32 },
    /// `Cmd::Stir` at a point (`x`, `y`), at an element (`key`), or along a path (`path`, `from`, `to`).
    Stir {
        #[serde(default)] x: Option<f32>, #[serde(default)] y: Option<f32>, #[serde(default)] key: Option<String>,
        #[serde(default)] path: Option<Vec<[f32; 2]>>, #[serde(default)] from: f32, #[serde(default = "one")] to: f32,
        #[serde(default)] v: [f32; 2], #[serde(default = "one")] energy: f32, #[serde(default = "radius")] radius: f32,
    },
    /// `Cmd::Press`: press an element as a person would.
    Press { key: String },
    /// `Cmd::Commit`: play the look's transition from (x, y).
    Commit { x: f32, y: f32, #[serde(default)] scope: Option<String> },
    /// `Cmd::Hold`: stop the beat clock for this many steps.
    Hold(u8),
    /// `Cmd::Retrigger`: play the current step again.
    Retrigger,
    /// `Cmd::Rewind`: take the beat clock back this many beats.
    Rewind(f32),
    /// `Cmd::Set`: set one number of the look by its name.
    Set { name: String, value: f32 },
    /// `Cmd::ClearFocus`: nothing has the focus.
    ClearFocus,
    /// `Cmd::ClearPlate`: empty the plate `key`.
    ClearPlate { key: String },
}
fn one() -> f32 { 1.0 }
fn radius() -> f32 { 120.0 }
impl CmdSpec {
    /// The command it stands for.
    pub fn cmd(&self) -> Result<crate::morph::Cmd, DriveError> {
        use crate::morph::{Cmd, StirAt};
        Ok(match self.clone() {
            CmdSpec::Ripplr { x, y, strength } => Cmd::Ripplr { x, y, strength },
            CmdSpec::RipplrAt { key, strength } => Cmd::RipplrAt { key, strength },
            CmdSpec::Stir { x, y, key, path, from, to, v, energy, radius } => {
                let at = match (key, path, x, y) {
                    (Some(k), _, _, _) => StirAt::Key(k),
                    (None, Some(p), _, _) => StirAt::Path { points: std::rc::Rc::new(p), from, to },
                    (None, None, Some(x), Some(y)) => StirAt::Point([x, y]),
                    _ => return Err(DriveError::NoPosition),
                };
                Cmd::Stir { at, v, energy, radius, scope: crate::el::Scope::Global }
            }
            CmdSpec::Press { key } => Cmd::Press { key },
            CmdSpec::Commit { x, y, scope } => Cmd::Commit { x, y, scope },
            CmdSpec::Hold(n) => Cmd::Hold(n),
            CmdSpec::Retrigger => Cmd::Retrigger,
            CmdSpec::Rewind(b) => Cmd::Rewind(b),
            CmdSpec::ClearFocus => Cmd::ClearFocus,
            CmdSpec::ClearPlate { key } => Cmd::ClearPlate { key },
            CmdSpec::Set { name, value } => {
                Look::default().set(&name, value).map_err(|e| DriveError::BadCommand(e.to_string()))?;
                Cmd::Set { name, value }
            }
        })
    }
}

/// Read a script: a JSON list of events.
pub fn parse_events(json: &str, source: &str) -> Result<Vec<Event>, DriveError> {
    serde_json::from_str(json).map_err(|e| DriveError::BadEvents { file: source.to_owned(), message: e.to_string() })
}

/// A page that can be driven and drawn, whatever its state and message types.
pub trait Driven {
    fn now(&self) -> f32;
    fn tick(&mut self, t: f32);
    fn apply(&mut self, input: &Input) -> Result<(), DriveError>;
    fn look(&self) -> Look;
    fn tree(&self) -> TreeNode;
    fn problems(&self) -> Vec<ViewProblem>;
    fn needs_frame(&self, t: f32) -> bool;
    fn draw(&mut self, o: &mut Offscreen, look: &Look) -> Vec<u8>;
    /// Take a command for the runtime (`Ui::command`); a page that has none refuses it.
    fn command(&mut self, _cmd: crate::morph::Cmd) -> Result<(), DriveError> { Err(DriveError::BadCommand("this page takes no commands".into())) }
    /// The look as a recipe string (`Preset::recipe`), when the page has one.
    fn recipe(&self) -> Option<String> { None }
    /// Something visible moves on purpose (`El::ongoing`).
    fn ongoing(&self) -> bool { false }
    /// A frame was drawn (or counts as drawn): what was asked for is done.
    fn frame_drawn(&mut self) {}
    /// The audit of the frame just drawn (`audit::audit`): draws it again with the glyphs hidden for
    /// the ground under the text. `px` is the frame as drawn. A page wrapping a `Ui` forwards to it;
    /// the default finds nothing.
    fn audit(&mut self, _o: &mut Offscreen, _look: &Look, _px: &[u8]) -> Vec<crate::audit::Finding> { vec![] }
    /// `audit` of a page drawn in `preset`: its rules measured on the same frame too
    /// (`audit::audit_under`). The default leaves the rules out.
    fn audit_under(&mut self, o: &mut Offscreen, look: &Look, px: &[u8], _preset: &crate::preset::Preset) -> Vec<crate::audit::Finding> { self.audit(o, look, px) }
}
impl<S, M: Clone> Driven for Ui<S, M> {
    fn now(&self) -> f32 { Ui::now(self) }
    fn tick(&mut self, t: f32) { Ui::tick(self, t) }
    fn look(&self) -> Look { Ui::look(self) }
    fn tree(&self) -> TreeNode { self.export_tree() }
    fn problems(&self) -> Vec<ViewProblem> { Ui::problems(self) }
    fn needs_frame(&self, t: f32) -> bool { Ui::needs_frame(self, t) }
    fn draw(&mut self, o: &mut Offscreen, look: &Look) -> Vec<u8> { o.render(self, look) }
    fn ongoing(&self) -> bool { Ui::ongoing(self) }
    fn frame_drawn(&mut self) { Ui::frame_drawn(self) }
    fn command(&mut self, cmd: crate::morph::Cmd) -> Result<(), DriveError> {
        let check = match &cmd {
            crate::morph::Cmd::RipplrAt { key, .. } | crate::morph::Cmd::Press { key } => Some(key.clone()),
            crate::morph::Cmd::Stir { at: crate::morph::StirAt::Key(k), .. } => Some(k.clone()),
            _ => None,
        };
        if let Some(k) = check && self.node(&k).is_none() { return Err(DriveError::UnknownTarget { near: near_keys(self, &k), key: k }); }
        Ui::command(self, cmd);
        Ok(())
    }
    fn audit(&mut self, o: &mut Offscreen, look: &Look, px: &[u8]) -> Vec<crate::audit::Finding> { audit_ui(self, o, look, px, None) }
    fn audit_under(&mut self, o: &mut Offscreen, look: &Look, px: &[u8], preset: &crate::preset::Preset) -> Vec<crate::audit::Finding> { audit_ui(self, o, look, px, Some(preset)) }
    fn apply(&mut self, input: &Input) -> Result<(), DriveError> {
        let at = |ui: &Self, x: &Option<f32>, y: &Option<f32>, target: &Option<String>| -> Result<(f32, f32), DriveError> {
            if let Some(k) = target {
                return ui.node(k).map(|n| n.rect.center()).ok_or_else(|| DriveError::UnknownTarget { key: k.clone(), near: near_keys(ui, k) });
            }
            match (x, y) { (Some(x), Some(y)) => Ok((*x, *y)), _ => Err(DriveError::NoPosition) }
        };
        match input {
            Input::Press { x, y, target, shift } => { let (x, y) = at(self, x, y, target)?; self.pointer_down(1, PointerKind::Mouse, x, y, *shift); }
            Input::Release { x, y, target } => { let (x, y) = at(self, x, y, target)?; self.pointer_up(1, PointerKind::Mouse, x, y); }
            Input::Move { x, y, target } => { let (x, y) = at(self, x, y, target)?; self.pointer_moved(1, PointerKind::Mouse, x, y); }
            Input::Wheel { x, y, target, dx, dy } => { let (x, y) = at(self, x, y, target)?; self.wheel_xy(x, y, *dx, *dy); }
            Input::Key { key, shift, cmd, alt } => {
                let k = Key::from_name(key).ok_or_else(|| DriveError::UnknownKey { name: key.clone() })?;
                self.key_mods(k, Mods { shift: *shift, cmd: *cmd, alt: *alt });
            }
            Input::Text { text } => { self.text_input(text); }
            Input::Cmd { cmd } => { let c = cmd.cmd()?; Driven::command(self, c)?; }
        }
        Ok(())
    }
}

/// Keys of the page that contain `k` (or its last part), to suggest when a target is not there.
fn near_keys<S, M: Clone>(ui: &Ui<S, M>, k: &str) -> String {
    let tail = k.rsplit('/').next().unwrap_or(k);
    let mut out: Vec<&str> = ui.nodes().iter().filter(|n| n.keyed && (n.key.contains(k) || n.key.ends_with(tail))).map(|n| n.key.as_str()).take(8).collect();
    if out.is_empty() { out = ui.nodes().iter().filter(|n| n.keyed).map(|n| n.key.as_str()).take(8).collect(); }
    out.join(", ")
}

pub use crate::style::{THEME_IDS, material_named as material, theme_named as theme};

/// `Driven::audit` for a runtime: the frame drawn again with the glyphs hidden (the ground under the
/// text) and, in another material, in plain (text the material wears away); a preset's rules when given.
fn audit_ui<S, M: Clone>(ui: &mut Ui<S, M>, o: &mut Offscreen, look: &Look, px: &[u8], preset: Option<&crate::preset::Preset>) -> Vec<crate::audit::Finding> {
    let (partial, hide) = (o.r.partial, o.r.hide_glyphs);
    o.r.partial = false;
    let mut frame = |ui: &mut Ui<S, M>, look: &Look, hide: bool| { o.r.hide_glyphs = hide; ui.invalidate(); o.render(ui, look) };
    let ground = frame(ui, look, true);
    // The same frame in plain (the material alone swapped), for text the material wears away.
    let plain = (look.material != Material::Plain).then(|| {
        let p = Look { material: Material::Plain, ..*look };
        (frame(ui, &p, false), frame(ui, &p, true))
    });
    (o.r.partial, o.r.hide_glyphs) = (partial, hide);
    ui.invalidate();
    // Text bent by the finishing pass is measured where the bend drew it.
    let bend = if look.post.active() { look.post.curvature.amount } else { 0.0 };
    let size = ui.device_size();
    let (px, ground) = (&crate::audit::unbend(px, size, bend)[..], crate::audit::unbend(&ground, size, bend));
    let plain = plain.map(|(p, pg)| (crate::audit::unbend(&p, size, bend), crate::audit::unbend(&pg, size, bend)));
    let mut f = match preset { Some(p) => crate::audit::audit_under(ui, px, &ground, &look.theme, &p.name, &p.rules), None => crate::audit::audit(ui, px, &ground) };
    if let Some((p, pg)) = &plain { f.extend(crate::audit::eroded(ui, (px, &ground), (p, pg), look.material.name())); }
    crate::audit::sort(&mut f);
    f
}

/// A page with the look's material and theme replaced (for recipes, whose look comes from their state).
struct Styled { inner: Box<dyn Driven>, material: Option<Material>, theme: Option<usize> }
impl Driven for Styled {
    fn now(&self) -> f32 { self.inner.now() }
    fn tick(&mut self, t: f32) { self.inner.tick(t) }
    fn apply(&mut self, input: &Input) -> Result<(), DriveError> { self.inner.apply(input) }
    fn look(&self) -> Look {
        let mut l = self.inner.look();
        if let Some(m) = self.material { l.material = m; }
        if let Some(t) = self.theme { l.theme = THEMES[t]; }
        l
    }
    fn tree(&self) -> TreeNode { self.inner.tree() }
    fn problems(&self) -> Vec<ViewProblem> { self.inner.problems() }
    fn needs_frame(&self, t: f32) -> bool { self.inner.needs_frame(t) }
    fn draw(&mut self, o: &mut Offscreen, look: &Look) -> Vec<u8> { self.inner.draw(o, look) }
    fn ongoing(&self) -> bool { self.inner.ongoing() }
    fn frame_drawn(&mut self) { self.inner.frame_drawn() }
    fn command(&mut self, cmd: crate::morph::Cmd) -> Result<(), DriveError> { self.inner.command(cmd) }
    fn audit(&mut self, o: &mut Offscreen, look: &Look, px: &[u8]) -> Vec<crate::audit::Finding> { self.inner.audit(o, look, px) }
    fn audit_under(&mut self, o: &mut Offscreen, look: &Look, px: &[u8], preset: &crate::preset::Preset) -> Vec<crate::audit::Finding> { self.inner.audit_under(o, look, px, preset) }
}

/// A page drawn in a preset (`RenderSpec::preset`): its look is the preset's over the page's own (the
/// material and theme asked for on top), and its audit holds it to the preset's rules.
struct Preseted { inner: Box<dyn Driven>, preset: crate::preset::Preset, material: Option<Material>, theme: Option<usize> }
impl Driven for Preseted {
    fn now(&self) -> f32 { self.inner.now() }
    fn tick(&mut self, t: f32) { self.inner.tick(t) }
    fn apply(&mut self, input: &Input) -> Result<(), DriveError> { self.inner.apply(input) }
    fn look(&self) -> Look {
        let mut l = self.preset.apply(&self.inner.look());
        if let Some(m) = self.material { l.material = m; }
        if let Some(t) = self.theme { l.theme = THEMES[t]; }
        l
    }
    fn tree(&self) -> TreeNode { self.inner.tree() }
    fn problems(&self) -> Vec<ViewProblem> { self.inner.problems() }
    fn needs_frame(&self, t: f32) -> bool { self.inner.needs_frame(t) }
    fn draw(&mut self, o: &mut Offscreen, look: &Look) -> Vec<u8> { self.inner.draw(o, look) }
    fn ongoing(&self) -> bool { self.inner.ongoing() }
    fn frame_drawn(&mut self) { self.inner.frame_drawn() }
    fn command(&mut self, cmd: crate::morph::Cmd) -> Result<(), DriveError> { self.inner.command(cmd) }
    fn recipe(&self) -> Option<String> { Some(self.preset.recipe()) }
    fn audit(&mut self, o: &mut Offscreen, look: &Look, px: &[u8]) -> Vec<crate::audit::Finding> { self.inner.audit_under(o, look, px, &self.preset) }
    fn audit_under(&mut self, o: &mut Offscreen, look: &Look, px: &[u8], preset: &crate::preset::Preset) -> Vec<crate::audit::Finding> { self.inner.audit_under(o, look, px, preset) }
}
/// `open` with every page drawn in `preset` (its look over the page's own, the material and theme asked
/// for on top) and audited by its rules too (`audit::audit_under`): for `render_with`, `audit_with` and
/// `frames`, as `render --preset` does.
pub fn in_preset<'a>(preset: &'a crate::preset::Preset, open: Opener<'a>) -> Box<OpenFn<'a>> {
    Box::new(move |page, faces, size, (material, theme, locale)| {
        let inner = open(page, faces, size, (material, theme, locale))?;
        Ok(Box::new(Preseted { inner, preset: preset.clone(), material, theme }) as Box<dyn Driven>)
    })
}
/// The preset `given` names: a built-in one by its hash (`normcore`) or name, else a preset JSON file
/// (`Preset::from_json`, its rules checked).
pub fn preset_named(given: &str) -> Result<crate::preset::Preset, LookError> {
    let all = crate::preset::presets();
    if let Some(p) = all.iter().find(|p| p.hash == given || p.name.eq_ignore_ascii_case(given)) { return Ok(p.clone()); }
    let known = || all.iter().map(|p| p.hash.clone()).collect::<Vec<_>>().join(", ");
    let json = std::fs::read_to_string(given).map_err(|_| LookError::UnknownPreset { given: given.to_owned(), known: known() })?;
    crate::preset::Preset::from_json(&json).map_err(|e| LookError::BadPreset { file: given.to_owned(), message: e.to_string() })
}

/// Open a recipe (`recipe-form`, or just `form`) at `w` × `h` device px and `scale`, with the material
/// and theme given (else the recipe's).
pub fn open(page: &str, faces: Vec<crate::text::Face>, w: u32, h: u32, scale: f32, material: Option<Material>, theme: Option<usize>) -> Result<Box<dyn Driven>, LookError> {
    open_in(page, faces, (w, h, scale), (material, theme, None))
}
/// How `render_with` opens a page: (page, faces, (w, h, scale), (material, theme, locale)).
pub type Opener<'a> = &'a OpenFn<'a>;
/// What an `Opener` points at: page, faces, (width, height, scale), (material, theme, locale) to the page.
pub type OpenFn<'a> = dyn Fn(&str, Vec<crate::text::Face>, (u32, u32, f32), (Option<Material>, Option<usize>, Option<crate::i18n::Locale>)) -> Result<Box<dyn Driven>, LookError> + 'a;
/// `open` with the look and the language given together: (material, theme, locale). Recipes keep
/// their own language.
pub fn open_in(page: &str, faces: Vec<crate::text::Face>, (w, h, scale): (u32, u32, f32), (material, theme, _locale): (Option<Material>, Option<usize>, Option<crate::i18n::Locale>)) -> Result<Box<dyn Driven>, LookError> {
    if let Some(c) = page.strip_prefix("component-") {
        let all = || crate::recipes::components::all().iter().map(|(n, _)| format!("component-{n}")).collect::<Vec<_>>().join(", ");
        return match crate::recipes::components::ui(c, faces, w, h, scale) { Some(u) => Ok(styled(Box::new(u), material, theme)), None => Err(LookError::UnknownPage { given: page.to_owned(), recipes: all() }) };
    }
    let name = page.strip_prefix("recipe-").unwrap_or(page);
    match crate::recipes::open(name, faces, w, h, scale) {
        Some(inner) => Ok(styled(inner, material, theme)),
        None => Err(LookError::UnknownPage { given: page.to_owned(), recipes: crate::recipes::NAMES.iter().map(|n| format!("recipe-{n}")).collect::<Vec<_>>().join(", ") }),
    }
}
/// A page drawn in the material and theme given (else its own).
pub fn styled(inner: Box<dyn Driven>, material: Option<Material>, theme: Option<usize>) -> Box<dyn Driven> { Box::new(Styled { inner, material, theme }) }

/// The rates `frames` takes: those that step the 60 Hz clock by a whole number of ticks.
pub const FPS: [u32; 5] = [60, 30, 20, 15, 12];

/// Play `events` on a fixed 60 Hz clock from 0 to `at_ms` (each event is applied on the first frame at
/// or after its time, in the order given), then tick exactly at `at_ms`.
pub fn play(d: &mut dyn Driven, events: &[Event], at_ms: f32) -> Result<(), DriveError> {
    let mut p = Player::new(events, &d.look().beat)?;
    p.to(d, at_ms.max(0.0) / 1000.0)?;
    // Events after the end are not played.
    Ok(())
}

/// The 60 Hz clock and the events still to play, so a run of frames plays on from where it is.
struct Player<'a> { order: Vec<(usize, f32, &'a Event)>, next: usize, k: u32 }
impl<'a> Player<'a> {
    /// The events in time order; an `at` on the beat is put in ms by `beat`.
    fn new(events: &'a [Event], beat: &crate::beat::Beat) -> Result<Self, DriveError> {
        let mut order = vec![];
        for (i, e) in events.iter().enumerate() {
            let ms = match &e.at {
                Some(at) => beat.parse_at(at).map(|s| s * 1000.0).ok_or_else(|| DriveError::AtEvent { index: i, inner: Box::new(DriveError::BadBeatTime { at: at.clone() }) })?,
                None => e.at_ms,
            };
            order.push((i, ms, e));
        }
        order.sort_by(|a, b| a.1.total_cmp(&b.1));
        Ok(Self { order, next: 0, k: 0 })
    }
    /// Tick from where the clock is to `end` s (each event applied on the first tick at or after its
    /// time, in the order given), then exactly at `end`.
    fn to(&mut self, d: &mut dyn Driven, end: f32) -> Result<(), DriveError> {
        loop {
            let t = (self.k as f32 / 60.0).min(end);
            while self.next < self.order.len() && self.order[self.next].1 / 1000.0 <= t + 1e-6 {
                d.tick(t);
                let (index, _, e) = self.order[self.next];
                d.apply(&e.input).map_err(|e| DriveError::AtEvent { index, inner: Box::new(e) })?;
                self.next += 1;
            }
            d.tick(t);
            if t >= end { return Ok(()); }
            self.k += 1;
        }
    }
}

/// What one `render` call asks for.
#[derive(Clone, Debug)]
pub struct RenderSpec {
    pub page: String,
    pub w: u32,
    pub h: u32,
    pub scale: f32,
    pub material: Option<String>,
    pub theme: Option<String>,
    pub at_ms: f32,
    pub events: Vec<Event>,
    /// The language (`en`, `zh-Hans`), else the page's own (DESIGN.md §1.20).
    pub locale: Option<String>,
}
/// The result of a render: the PNG bytes, the tree, and what the view got wrong.
pub struct Rendered { pub png: Vec<u8>, pub tree: TreeNode, pub problems: Vec<ViewProblem> }

/// Open, play, draw: the whole `render` tool as a function, for the recipes.
pub fn render(spec: &RenderSpec, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>) -> Result<Rendered, DriveError> {
    render_with(spec, faces, o, &open_in)
}
/// `render` with the pages an application adds (its `open` falls back to `open_in` for recipes).
pub fn render_with(spec: &RenderSpec, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>, open: Opener) -> Result<Rendered, DriveError> {
    render_inner(spec, faces, o, open, false).map(|r| r.0)
}
/// `render`, and the audit of the frame (`audit::audit`, `audit::restless`), most serious first.
pub fn audit(spec: &RenderSpec, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>) -> Result<(Rendered, Vec<crate::audit::Finding>), DriveError> {
    audit_with(spec, faces, o, &open_in)
}
/// `audit` with the pages an application adds.
pub fn audit_with(spec: &RenderSpec, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>, open: Opener) -> Result<(Rendered, Vec<crate::audit::Finding>), DriveError> {
    render_inner(spec, faces, o, open, true)
}
/// A run of frames: the page opened once, its events played on the fixed 60 Hz clock, and frame `n` of
/// `range` drawn at `n / fps` s (`fps` one of `FPS`), handed to `each` with its number. Frame `n` at 60
/// is the frame `render` draws at `n × 1000 / 60` ms; at 30 it is frame `2n` at 60. The wall clock
/// is never read.
pub fn frames(spec: &RenderSpec, range: std::ops::Range<u32>, fps: u32, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>, open: Opener, each: &mut dyn FnMut(u32, &Rendered)) -> Result<(), DriveError> {
    frames_inner(spec, range, fps, faces, o, open, false, &mut |n, r, _| each(n, r))
}
/// `frames`, each frame audited (`audit::audit`; the restless check is left to the last frame).
pub fn audit_frames(spec: &RenderSpec, range: std::ops::Range<u32>, fps: u32, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>, open: Opener, each: &mut dyn FnMut(u32, &Rendered, &[crate::audit::Finding])) -> Result<(), DriveError> {
    frames_inner(spec, range, fps, faces, o, open, true, each)
}
#[allow(clippy::too_many_arguments)]
fn frames_inner(spec: &RenderSpec, range: std::ops::Range<u32>, fps: u32, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>, open: Opener, audit: bool, each: &mut dyn FnMut(u32, &Rendered, &[crate::audit::Finding])) -> Result<(), DriveError> {
    if !FPS.contains(&fps) { return Err(DriveError::BadFps { given: fps }); }
    if range.is_empty() { return Err(DriveError::NoFrames { from: range.start, to: range.end }); }
    let mut d = open_spec(spec, faces, open)?;
    let mut own;
    let o = match o { Some(o) => o, None => { own = Offscreen::try_new(spec.w, spec.h)?; &mut own } };
    if (o.w, o.h) != (spec.w, spec.h) { return Err(SurfaceError::SizeMismatch { w: spec.w, h: spec.h, have_w: o.w, have_h: o.h }.into()); }
    let step = 60 / fps;
    let mut p = Player::new(&spec.events, &d.look().beat)?;
    // Each frame drawn whole, as a single `render` draws it.
    let partial = o.r.partial;
    o.r.partial = false;
    let recipe = d.recipe();
    let texts: Vec<(&str, &str)> = recipe.as_deref().map(|r| ("amenui-recipe", r)).into_iter().collect();
    let mut out = Ok(());
    for n in range {
        let ticks = n * step;
        if let Err(e) = p.to(d.as_mut(), ticks as f32 / 60.0) { out = Err(e); break; }
        let look = d.look();
        let px = d.draw(o, &look);
        let findings = if audit { d.audit(o, &look, &px) } else { vec![] };
        let r = Rendered { png: png_bytes_with(spec.w, spec.h, &px, &texts), tree: d.tree(), problems: d.problems() };
        each(n, &r, &findings);
    }
    o.r.partial = partial;
    out
}
fn open_spec(spec: &RenderSpec, faces: Vec<crate::text::Face>, open: Opener) -> Result<Box<dyn Driven>, DriveError> {
    let material = spec.material.as_deref().map(material).transpose()?;
    let theme = spec.theme.as_deref().map(theme).transpose()?;
    if spec.w == 0 || spec.h == 0 { return Err(SurfaceError::ZeroSize { w: spec.w, h: spec.h }.into()); }
    let locale = match spec.locale.as_deref() { Some(t) => Some(crate::i18n::Locale::from_tag(t).ok_or_else(|| LookError::UnknownLocale { given: t.to_owned(), known: crate::i18n::Locale::ALL.iter().map(|l| l.tag()).collect::<Vec<_>>().join(", ") })?), None => None };
    Ok(open(&spec.page, faces, (spec.w, spec.h, spec.scale.max(0.25)), (material, theme, locale))?)
}
fn render_inner(spec: &RenderSpec, faces: Vec<crate::text::Face>, o: Option<&mut Offscreen>, open: Opener, audit: bool) -> Result<(Rendered, Vec<crate::audit::Finding>), DriveError> {
    let mut d = open_spec(spec, faces, open)?;
    play(d.as_mut(), &spec.events, spec.at_ms)?;
    let look = d.look();
    let mut own;
    let o = match o { Some(o) => o, None => { own = Offscreen::try_new(spec.w, spec.h)?; &mut own } };
    if (o.w, o.h) != (spec.w, spec.h) { return Err(SurfaceError::SizeMismatch { w: spec.w, h: spec.h, have_w: o.w, have_h: o.h }.into()); }
    let px = d.draw(o, &look);
    let recipe = d.recipe();
    let texts: Vec<(&str, &str)> = recipe.as_deref().map(|r| ("amenui-recipe", r)).into_iter().collect();
    let (tree, problems) = (d.tree(), d.problems());
    let findings = if audit {
        let mut f = d.audit(o, &look, &px);
        // Restless: the clock runs on from the frame with no input.
        let last = spec.events.iter().map(|e| e.at_ms).fold(0.0f32, f32::max).max(spec.at_ms) / 1000.0;
        f.extend(crate::audit::restless(d.as_mut(), last));
        crate::audit::sort(&mut f);
        f
    } else { vec![] };
    Ok((Rendered { png: png_bytes_with(spec.w, spec.h, &px, &texts), tree, problems }, findings))
}
