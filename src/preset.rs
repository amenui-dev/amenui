//! Presets as data (DESIGN.md §4.1): a style preset is plain, serialisable data — theme, material and its
//! numbers, type, annotation, how it answers (interaction character) and how it arrives (a transition).
//! A transition is a small program of up to four steps over the same primitives (reveals with a shape,
//! bands, fades, a light bar, colour planes, a loosening, layers, detection boxes, a carve, an
//! outline-first draw), each a kind and five numbers, run by the material pass between the
//! frame as it was and the new one. It plays for "switch to this style" and for "enter a page in this
//! style": always the target's.
use crate::el::Response;
use crate::morph::Recompose;
use crate::style::{Gaze, Look, Material, MaterialParams, Markr, THEMES, Theme, styles, theme_index};
use serde::{Deserialize, Serialize};

/// What one step of a transition does. The numbers `a`–`e` mean what `StepKind::params` says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    #[default]
    None,
    /// The new frame appears where the shape has reached: a = shape (0 radial from the press, 1 a line at
    /// angle b, 2 rows of c px top to bottom, 3 cells of c px in hashed order, 4 a torn line at angle b
    /// with c px of noise), d = edge softness px, e = rows just reached show only the lattice's dots (0–1
    /// of the window, the phosphor's lag).
    Reveal,
    /// Horizontal bands of a px tear sideways by up to b px with c px between the colour channels; d hard
    /// cuts between the frames; locked (straight, new) from e on.
    Bands,
    /// A cross-fade, the new frame arriving from a px away at angle b.
    Fade,
    /// A light bar sweeping at angle a, b px wide; behind it the new frame with toner noise of
    /// strength c settling within d px.
    Bar,
    /// a rows of flat colour planes slide across, b of the theme's hues, and uncover the new frame.
    Planes,
    /// Everything loosens by up to a (most at the middle of the step), through the material.
    Loosen,
    /// The layers part by up to a px at angle b (the brighter the nearer), swap, and close again.
    Layers,
    /// Boxes find up to a elements (the largest), drawn with b px lines, and replace what is in them.
    Boxes,
    /// Strokes grow along the new outlines up to a px from them; the ink floods from b (0–1) on.
    Carve,
    /// Outlines (a px) and frames draw first, in reading order; the content fills from b (0–1) on.
    Outline,
    /// The old page cracks and the new one shows through the cracks as they widen: a = the size of the
    /// pieces (16–400 px), b = the angle the cracks lean to (0–180°, 90 vertical bands), c = how strongly
    /// (0 a dried mud floor, 1 bands), d = how ragged their edges are (0–12 px), e = hard edges (1) or
    /// soft (0). The pattern is hashed in the shader from the numbers alone: no earlier frame is read.
    Crack,
}
impl StepKind {
    pub const ALL: [StepKind; 12] = [StepKind::None, StepKind::Reveal, StepKind::Bands, StepKind::Fade, StepKind::Bar, StepKind::Planes, StepKind::Loosen, StepKind::Layers, StepKind::Boxes, StepKind::Carve, StepKind::Outline, StepKind::Crack];
    /// Crack's numbers as `StepKind::params` names them, at their defaults: pieces 120 px, leaning 70°,
    /// 0.6 toward bands, 3 px ragged, hard edges.
    pub const CRACK: [f32; 5] = [120.0, 70.0, 0.6, 2.0, 1.0];
    pub fn id(self) -> f32 { Self::ALL.iter().position(|k| *k == self).unwrap_or(0) as f32 }
    /// The names of `a`–`e` for this kind (empty: unused), for number controls.
    pub fn params(self) -> [&'static str; 5] {
        match self {
            StepKind::None => ["", "", "", "", ""],
            StepKind::Reveal => ["形状", "角度", "尺寸", "柔边", "点阵带"],
            StepKind::Bands => ["带高", "错位", "色差", "硬切", "锁定"],
            StepKind::Fade => ["位移", "角度", "", "", ""],
            StepKind::Bar => ["角度", "光条宽", "墨粉", "沉降", ""],
            StepKind::Planes => ["行数", "色数", "", "", ""],
            StepKind::Loosen => ["松开", "", "", "", ""],
            StepKind::Layers => ["层距", "角度", "", "", ""],
            StepKind::Boxes => ["框数", "线宽", "", "", ""],
            StepKind::Carve => ["刻痕", "灌墨", "", "", ""],
            StepKind::Outline => ["线宽", "填充", "", "", ""],
            StepKind::Crack => ["裂块", "角度", "偏向", "毛边", "硬边"],
        }
    }
}

/// One step: its kind, when it runs (fractions 0–1 of the transition) and its numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Step { pub kind: StepKind, pub from: f32, pub to: f32, pub a: f32, pub b: f32, pub c: f32, pub d: f32, pub e: f32 }
fn step(kind: StepKind, from: f32, to: f32, v: [f32; 5]) -> Step { Step { kind, from, to, a: v[0], b: v[1], c: v[2], d: v[3], e: v[4] } }

/// How a style arrives. `duration_ms` 0 is at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Transition { pub duration_ms: f32, pub steps: [Step; 4] }
impl Transition {
    pub fn of(duration_ms: f32, s: &[Step]) -> Self { let mut steps = [Step::default(); 4]; for (i, x) in s.iter().take(4).enumerate() { steps[i] = *x; } Self { duration_ms, steps } }
    /// How loose everything is at `x` (0–1 of the transition), from its Loosen steps.
    pub fn loosen(&self, x: f32) -> f32 {
        self.steps.iter().filter(|s| s.kind == StepKind::Loosen).map(|s| { let e = ((x - s.from) / (s.to - s.from).max(1e-4)).clamp(0.0, 1.0); s.a * (std::f32::consts::PI * e).sin() }).sum::<f32>().max(0.0)
    }
    /// The uniform form: a header and two vec4s per step.
    pub fn pack(&self, origin: [f32; 2], start: f32) -> [[f32; 4]; 9] {
        let mut o = [[0.0; 4]; 9];
        o[0] = [self.duration_ms / 1000.0, origin[0], origin[1], start];
        for (i, s) in self.steps.iter().enumerate() { o[1 + i * 2] = [s.kind.id(), s.from, s.to, s.a]; o[2 + i * 2] = [s.b, s.c, s.d, s.e]; }
        o
    }
}

/// How a style answers: press feedback, fronts, what text and decoration do by default, recomposition.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interaction {
    /// How long the burst over a pressed control lasts.
    pub press_ms: f32,
    /// Fronts: speed (px/s), width (px), how long they live (s).
    pub front_speed: f32,
    pub front_width: f32,
    pub front_life: f32,
    /// What text and controls do, and what everything else does, when nothing says (DESIGN.md §1.15).
    pub text: Response,
    pub decoration: Response,
    pub recompose: Recompose,
    /// How scrolling looks (DESIGN.md §1.8.3).
    #[serde(default)]
    pub scroll: crate::style::Scroll,
    /// The field map: how strong the trails of moving things are, how they spread and settle (DESIGN.md §1.29).
    #[serde(default)]
    pub map: crate::fieldmap::MapFx,
    /// The tempo things fall on (DESIGN.md §1.32; `bpm` 0: off).
    #[serde(default)]
    pub beat: crate::beat::Beat,
}
impl Default for Interaction {
    fn default() -> Self { Self { press_ms: 350.0, front_speed: 760.0, front_width: 26.0, front_life: 1.4, text: Response::Nudge, decoration: Response::Loosen, recompose: Recompose::default(), scroll: crate::style::Scroll::default(), map: crate::fieldmap::MapFx::default(), beat: crate::beat::Beat::default() } }
}

/// A style preset as data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub hash: String,
    /// The theme by name (`style::THEMES`), and the faces for display, body and mono (`text::SANS` …).
    pub theme: String,
    pub faces: [u8; 3],
    pub material: Material,
    /// The material's numbers (`style::MAT_PARAMS`).
    pub params: [f32; 8],
    pub gaze: Gaze,
    pub loose_scale: f32,
    /// The lattice pitch of text and shapes.
    pub pitch: f32,
    pub pic: Markr,
    pub interaction: Interaction,
    pub transition: Transition,
    /// The finishing pass (post effects, dither, palette), off unless a preset says.
    #[serde(default)]
    pub post: crate::post::PostSpec,
    /// Marks on 3D elements by depth.
    #[serde(default)]
    pub marks: crate::style::Marks,
    /// What the audit holds pages in this preset to, beyond its own floors (`audit::audit_under`). Rules
    /// only tighten: a looser one is refused when the preset is read (`Preset::check`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
}

/// Which text a contrast rule is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Of {
    /// Running text: legible text no larger than the theme's body size with more than 40 characters in
    /// the element (paragraphs, list items, code).
    Prose,
    /// All text to read.
    Read,
}

/// One rule a preset holds its pages to (DESIGN.md §7.2). Each is measured on the frame as drawn and only
/// tightens the audit's own floors.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Each line's strongest ink against what is drawn under it, as `Kind::Contrast` measures: at least
    /// `min`:1.
    Contrast { of: Of, min: f32 },
    /// Text to read drawn at least `min` logical px tall (size × scale × its pose's vertical scale, over
    /// the scale).
    TextSize { min: f32 },
    /// The ink of text to read at least `min` logical px from each edge of the window (text in a scroll
    /// view: from the left, the right, and the top while it is scrolled to the top).
    Margin { min: f32 },
    /// At most `max` characters on a line of running text (wide characters, CJK and full-width, count 2).
    Measure { max: f32 },
    /// At most `max` (0–1) of the frame at rest in the theme's accent (within 24/255 in every channel).
    Accent { max: f32 },
}
impl Rule {
    /// Its name, as the audit's findings and the JSON call it.
    pub fn name(&self) -> &'static str {
        match self { Rule::Contrast { .. } => "contrast", Rule::TextSize { .. } => "text_size", Rule::Margin { .. } => "margin", Rule::Measure { .. } => "measure", Rule::Accent { .. } => "accent" }
    }
}

impl Preset {
    /// Whether every rule only tightens the audit's floors: a contrast under `audit::MIN_CONTRAST` or a
    /// text size under `audit::MIN_TEXT_PX` (at scale 1) is refused, by name; so is a margin, measure
    /// or accent share that is not a number of its unit (negative, or an accent share over 1).
    pub fn check(&self) -> Result<(), crate::error::PresetError> {
        let looser = |rule: &'static str, given: f32, floor: f32| crate::error::PresetError::Looser { preset: self.name.clone(), rule, given, floor };
        for r in &self.rules {
            match *r {
                Rule::Contrast { min, .. } if min.is_nan() || min < crate::audit::MIN_CONTRAST => return Err(looser("contrast.min", min, crate::audit::MIN_CONTRAST)),
                Rule::TextSize { min } if min.is_nan() || min < crate::audit::MIN_TEXT_PX => return Err(looser("text_size.min", min, crate::audit::MIN_TEXT_PX)),
                Rule::Margin { min } if min.is_nan() || min < 0.0 => return Err(looser("margin.min", min, 0.0)),
                Rule::Measure { max } if max.is_nan() || max <= 0.0 => return Err(looser("measure.max", max, 1.0)),
                Rule::Accent { max } if !(0.0..=1.0).contains(&max) => return Err(looser("accent.max", max, if max > 1.0 { 1.0 } else { 0.0 })),
                _ => {}
            }
        }
        Ok(())
    }
    pub fn theme(&self) -> Theme {
        let t = THEMES[theme_index(&self.theme)];
        Theme { display: self.faces[0], body: self.faces[1], mono: self.faces[2], ..t }
    }
    /// The look this preset gives (the gallery's other choices stay as they are in `base`).
    pub fn apply(&self, base: &Look) -> Look {
        let mut l = *base;
        l.theme = self.theme();
        l.material = self.material;
        l.params = MaterialParams::all();
        if let Some(p) = l.params.get_mut(self.material.id() as usize) { *p = MaterialParams(self.params); }
        l.gaze = self.gaze;
        l.loose_scale = self.loose_scale;
        l.fx.pitch = self.pitch;
        let i = &self.interaction;
        (l.fx.wave_speed, l.fx.wave_width, l.fx.wave_life) = (i.front_speed, i.front_width, i.front_life);
        l.recompose = i.recompose;
        l.press_ms = i.press_ms;
        l.roles = [i.text, i.decoration];
        l.scroll = i.scroll;
        l.map = i.map;
        l.beat = i.beat;
        l.transition = self.transition;
        l.post = self.post.look();
        l.marks = self.marks;
        l
    }
    pub fn to_json(&self) -> String { serde_json::to_string_pretty(self).unwrap_or_default() }
    /// The preset as a recipe: what differs from the built-in preset it started from (by `hash`; the
    /// first one when it has no built-in), as `look:v3:` + base64url of `{"base": hash, "set": {path:
    /// value}}`. Short enough for a URL hash; every PNG the render tool writes carries one.
    pub fn recipe(&self) -> String {
        let all = presets();
        let base = all.iter().find(|p| p.hash == self.hash).unwrap_or(&all[0]);
        let (a, b) = (leaves(&serde_json::to_value(self).unwrap_or_default()), leaves(&serde_json::to_value(base).unwrap_or_default()));
        let set: serde_json::Map<String, serde_json::Value> = a.into_iter().filter(|(k, v)| b.get(k) != Some(v)).collect();
        let doc = serde_json::json!({ "base": base.hash, "set": set });
        format!("{RECIPE_PREFIX}{}", b64url(serde_json::to_string(&doc).unwrap_or_default().as_bytes()))
    }
    /// The preset a recipe describes: its base with every path set.
    pub fn from_recipe(s: &str) -> Result<Self, crate::error::PresetError> {
        let bad = |m: &str| crate::error::PresetError::Recipe(m.to_owned());
        let s = s.trim().trim_start_matches('#');
        if s.starts_with("look:v1:") {
            let renamed: Vec<String> = crate::style::RENAMED.iter().map(|(a, b)| format!("{a} → {b}")).collect();
            return Err(bad(&format!("it is a look:v1: recipe, from before the materials were renamed ({}); make it again, and it starts with {RECIPE_PREFIX}", renamed.join(", "))));
        }
        if let Some(old) = s.strip_prefix("look:v2:") {
            // From before the beat: a held pose was `interaction.hold_ms`.
            let hold = un_b64url(old).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|d| d.get("set").and_then(|v| v.get("interaction.hold_ms")).and_then(|v| v.as_f64()));
            let beat = match hold {
                Some(h) if h > 0.0 => { let b = crate::beat::Beat::of_hold(h as f32); format!("; its `interaction.hold_ms` {h} is now `interaction.beat` {{ bpm: {}, steps: {}, swing: 0 }}", b.bpm, b.steps) }
                _ => String::new(),
            };
            return Err(bad(&format!("it is a look:v2: recipe, from before the beat{beat}; make it again, and it starts with {RECIPE_PREFIX}")));
        }
        let body = s.strip_prefix(RECIPE_PREFIX).ok_or_else(|| bad(&format!("it does not start with {RECIPE_PREFIX}")))?;
        let bytes = un_b64url(body).ok_or_else(|| bad("its body is not base64url"))?;
        let doc: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| bad(&e.to_string()))?;
        let all = presets();
        let hash = doc.get("base").and_then(|v| v.as_str()).ok_or_else(|| bad("it names no base"))?;
        let base = all.into_iter().find(|p| p.hash == hash).ok_or_else(|| bad(&format!("no preset is called {hash}")))?;
        let mut v = serde_json::to_value(&base).map_err(|e| bad(&e.to_string()))?;
        if let Some(set) = doc.get("set").and_then(|v| v.as_object()) {
            for (path, x) in set {
                let mut at = &mut v;
                for k in path.split('.') {
                    at = match at { serde_json::Value::Object(m) => m.get_mut(k), serde_json::Value::Array(a) => k.parse::<usize>().ok().and_then(|i| a.get_mut(i)), _ => None }
                        .ok_or_else(|| bad(&format!("no field at {path}")))?;
                }
                *at = x.clone();
            }
        }
        // Read back as text: one way of reading a preset (`from_json`'s) is built, not a second from a value.
        let p: Self = serde_json::from_str(&v.to_string()).map_err(|e| bad(&e.to_string()))?;
        p.check()?;
        Ok(p)
    }
    pub fn from_json(s: &str) -> Result<Self, crate::error::PresetError> {
        let p: Self = serde_json::from_str(s).map_err(|e| crate::error::PresetError::Json(e.to_string()))?;
        p.check()?;
        Ok(p)
    }
    /// Every number in the preset, by its path in the JSON (`transition.steps.0.a`), for number controls
    /// made from the data itself.
    pub fn numbers(&self) -> Vec<(String, f32)> {
        let v = serde_json::to_value(self).unwrap_or_default();
        let mut out = vec![];
        walk(&v, String::new(), &mut out);
        out
    }
    /// Set the number at `path` (as `numbers` names it).
    pub fn set_number(&mut self, path: &str, x: f32) -> Result<(), crate::error::PresetError> {
        let mut v = serde_json::to_value(&*self).map_err(|e| crate::error::PresetError::Json(e.to_string()))?;
        let mut at = &mut v;
        for k in path.split('.') {
            at = match at { serde_json::Value::Object(m) => m.get_mut(k), serde_json::Value::Array(a) => k.parse::<usize>().ok().and_then(|i| a.get_mut(i)), _ => None }
                .ok_or_else(|| crate::error::PresetError::NoSuchNumber(path.to_owned()))?;
        }
        if !at.is_number() { return Err(crate::error::PresetError::NoSuchNumber(path.to_owned())); }
        // Whole numbers as integers, so integer fields (a dither's levels) take them too.
        *at = if x.fract() == 0.0 && x.abs() < 1e9 { serde_json::json!(x as i64) } else { serde_json::json!(x) };
        *self = serde_json::from_str(&v.to_string()).map_err(|e| crate::error::PresetError::Json(e.to_string()))?;
        Ok(())
    }
}
/// What starts every look recipe (the version is part of it: a later format gets a new prefix).
pub const RECIPE_PREFIX: &str = "look:v3:";
/// Every leaf of a JSON value (numbers, strings, booleans) by its dotted path.
fn leaves(v: &serde_json::Value) -> std::collections::BTreeMap<String, serde_json::Value> {
    fn go(v: &serde_json::Value, path: String, out: &mut std::collections::BTreeMap<String, serde_json::Value>) {
        let join = |k: &str| if path.is_empty() { k.to_owned() } else { format!("{path}.{k}") };
        match v {
            serde_json::Value::Object(m) => for (k, x) in m { go(x, join(k), out); },
            serde_json::Value::Array(a) => for (i, x) in a.iter().enumerate() { go(x, join(&i.to_string()), out); },
            _ => { out.insert(path, v.clone()); }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    go(v, String::new(), &mut out);
    out
}
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/// Base64url without padding (RFC 4648 §5).
pub fn b64url(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..c.len() + 1 { out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char); }
    }
    out
}
/// The bytes of base64url text (padding and whitespace ignored), or `None` when it is not base64url.
pub fn un_b64url(s: &str) -> Option<Vec<u8>> {
    let vals: Vec<u32> = s.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').map(|b| B64.iter().position(|x| *x == b).map(|p| p as u32)).collect::<Option<_>>()?;
    let mut out = Vec::with_capacity(vals.len() * 3 / 4);
    for c in vals.chunks(4) {
        if c.len() == 1 { return None; }
        let n = c.iter().enumerate().fold(0u32, |n, (i, v)| n | v << (18 - 6 * i));
        for i in 0..c.len() - 1 { out.push((n >> (16 - 8 * i)) as u8); }
    }
    Some(out)
}
fn walk(v: &serde_json::Value, path: String, out: &mut Vec<(String, f32)>) {
    let join = |k: &str| if path.is_empty() { k.to_owned() } else { format!("{path}.{k}") };
    match v {
        serde_json::Value::Number(n) => out.push((path, n.as_f64().unwrap_or(0.0) as f32)),
        serde_json::Value::Object(m) => for (k, x) in m { walk(x, join(k), out); },
        serde_json::Value::Array(a) => for (i, x) in a.iter().enumerate() { walk(x, join(&i.to_string()), out); },
        _ => {}
    }
}

/// The ten presets, in the order of `style::styles()`, each with its own interaction and transition.
pub fn presets() -> Vec<Preset> {
    use StepKind::*;
    styles().iter().map(|st| {
        let th = THEMES[st.theme];
        let base = Interaction::default();
        let (interaction, transition) = match st.hash {
            // Detection boxes find each element, then replace it.
            "gaze" => (Interaction { press_ms: 260.0, ..base }, Transition::of(820.0, &[step(Boxes, 0.0, 1.0, [12.0, 1.0, 0.0, 0.0, 0.0])])),
            // Row by row, the dots latch on, the old rows linger a little.
            "dotmatrix" => (Interaction { press_ms: 420.0, front_speed: 620.0, ..base }, Transition::of(760.0, &[step(Reveal, 0.0, 1.0, [2.0, 0.0, 7.0, 0.0, 0.08])])),
            // A photocopier's light bar; toner noise settling behind it.
            "lofi-print" => (Interaction { press_ms: 380.0, ..base }, Transition::of(880.0, &[step(Bar, 0.0, 1.0, [0.0, 70.0, 0.5, 160.0, 0.0])])),
            // Bands tear, the channels split, three hard cuts, then lock.
            "glitch" => (Interaction { press_ms: 300.0, front_speed: 1100.0, front_width: 16.0, ..base }, Transition::of(620.0, &[step(Bands, 0.0, 1.0, [14.0, 90.0, 8.0, 3.0, 0.82])])),
            // Carved in: strokes along the outlines, the ink last.
            "linocut" => (Interaction { press_ms: 420.0, front_speed: 520.0, ..base }, Transition::of(900.0, &[step(Carve, 0.0, 1.0, [9.0, 0.62, 0.0, 0.0, 0.0])])),
            // Layers part in depth, swap, close.
            // Frames and lines first, the content after.
            "tech-manual" => (Interaction { press_ms: 300.0, ..base }, Transition::of(880.0, &[step(Outline, 0.0, 1.0, [1.2, 0.55, 0.0, 0.0, 0.0]), step(Boxes, 0.0, 0.5, [10.0, 1.0, 0.0, 0.0, 0.0])])),
            // Nothing.
            "normcore" => (Interaction { press_ms: 200.0, ..base }, Transition::of(0.0, &[])),
            // A short fade and a small move.
            "cyberminimal" => (Interaction { press_ms: 160.0, front_width: 12.0, ..base }, Transition::of(120.0, &[step(Fade, 0.0, 1.0, [6.0, 90.0, 0.0, 0.0, 0.0])])),
            // Everything loosens into thread, then the outlines draw and the content fills.
            "loose-thread" => (Interaction { press_ms: 320.0, front_speed: 600.0, front_width: 30.0, beat: crate::beat::Beat::of_hold(83.0), text: Response::Loosen, ..base },
                Transition::of(700.0, &[step(Loosen, 0.0, 0.55, [1.0, 0.0, 0.0, 0.0, 0.0]), step(Outline, 0.45, 1.0, [1.0, 0.6, 0.0, 0.0, 0.0])])),
            // Flat colour planes slide across and uncover the new page.
            _ => (Interaction { press_ms: 380.0, front_speed: 680.0, front_width: 40.0, ..base }, Transition::of(860.0, &[step(Planes, 0.0, 1.0, [5.0, 3.0, 0.0, 0.0, 0.0])])),
        };
        // How scrolling looks in each: the same speeds, the trace sized to the preset's character.
        let mut interaction = interaction;
        let sc = &mut interaction.scroll;
        match st.hash {
            "normcore" => sc.trail = 0.0,
            "cyberminimal" => sc.trail = 14.0,
            "dotmatrix" => sc.trail = 40.0,
            "lofi-print" => sc.misreg = 4.0,
            "glitch" => { sc.slip = 34.0; sc.slip_ms = 90.0; }
            "linocut" => sc.trail = 34.0,
            _ => {}
        }
        // How strong each preset's trails are (DESIGN.md §1.29): none in Normcore, quiet in the plain
        // presets, strong where the material shows energy (dots, glitch).
        let map = &mut interaction.map;
        match st.hash {
            "normcore" => map.stirr = 0.0,
            "cyberminimal" => { map.stirr = 0.25; map.stirr_radius = 32.0; }
            "tech-manual" => map.stirr = 0.2,
            "gaze" => map.stirr = 0.3,
            "bauhaus" => map.stirr = 0.4,
            "lofi-print" | "linocut" => map.stirr = 0.5,
            "glitch" => { map.stirr = 0.7; map.energy_life = 0.25; }
            "dotmatrix" => { map.stirr = 0.8; map.energy_life = 0.7; }
            // The thread wanders and creeps; no breathing, so it rests plain.
            "loose-thread" => { map.stirr = 1.0; map.meander = 0.5; map.creep = 0.15; map.energy_life = 2.0; map.bleed = 0.06; map.breath = 0.0; }
            _ => {}
        }
        Preset {
            name: st.name.into(), hash: st.hash.into(), theme: th.name.into(), faces: th.roles(), material: st.material, params: st.params.0, gaze: st.gaze,
            loose_scale: st.loose_scale, pitch: st.pitch, pic: st.pic, interaction, transition, post: post_of(st.hash), marks: crate::style::Marks::default(),
            rules: rules_of(st.hash),
        }
    }).collect()
}

/// Each preset's rules (DESIGN.md §7.2): running text at 7:1 and all text to read at 4.5:1 everywhere;
/// the text size each theme's `small` is (Neo-Dotmatrix's pixel face at its native 12); the margin
/// and measure its layout keeps; an accent share only in the styles with one signal colour.
fn rules_of(hash: &str) -> Vec<Rule> {
    // (text size, margin, measure, accent share or none)
    let (size, margin, measure, accent) = match hash {
        "gaze" => (11.0, 16.0, 80.0, None),
        "dotmatrix" => (12.0, 14.0, 64.0, None),
        "lofi-print" => (12.0, 16.0, 72.0, None),
        "glitch" => (12.0, 24.0, 72.0, None),
        "linocut" => (12.0, 16.0, 68.0, None),
        "tech-manual" => (11.0, 24.0, 80.0, Some(0.02)),
        "normcore" => (12.0, 12.0, 80.0, Some(0.02)),
        "cyberminimal" => (11.0, 16.0, 80.0, Some(0.01)),
        "bauhaus" => (12.0, 16.0, 68.0, None),
        "loose-thread" => (11.0, 16.0, 72.0, Some(0.01)),
        _ => return vec![],
    };
    let mut r = vec![Rule::Contrast { of: Of::Prose, min: 7.0 }, Rule::Contrast { of: Of::Read, min: 4.5 }, Rule::TextSize { min: size }, Rule::Margin { min: margin }, Rule::Measure { max: measure }];
    r.extend(accent.map(|max| Rule::Accent { max }));
    r
}

/// Each preset's finishing pass: Neo-Dotmatrix is quantised to an amber palette through a 4×4
/// dither; Glitch and Lo-Fi Print carry screen and print artefacts; the rest have none.
fn post_of(hash: &str) -> crate::post::PostSpec {
    use crate::post::*;
    match hash {
        "dotmatrix" => PostSpec { dithr: Dithr { kind: DithrKind::Bayer4, levels: 4, strength: 0.8 }, palette: Palette { kind: PaletteKind::Amber, colors: vec![] }, bloom: Bloom { strength: 0.35, threshold: 0.6, radius: 6.0 }, ..PostSpec::default() },
        "glitch" => PostSpec { chroma: Chroma { offset: 1.5 }, scanlines: Scanlines { strength: 0.22, period: 3.0 }, grain: Grain { amount: 0.05, size: 1.0 }, vignette: Vignette { strength: 0.35, radius: 0.6 }, curvature: Curvature { amount: 0.03 }, clear_text: true, ..PostSpec::default() },
        "lofi-print" => PostSpec { grain: Grain { amount: 0.06, size: 1.5 }, vignette: Vignette { strength: 0.18, radius: 0.7 }, ..PostSpec::default() },
        _ => PostSpec::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_round_trip_as_json_and_numbers() {
        for p in presets() {
            let j = p.to_json();
            assert_eq!(Preset::from_json(&j).expect("parses"), p, "{}", p.name);
            assert!(p.transition.duration_ms <= 900.0, "{}: {} ms", p.name, p.transition.duration_ms);
            let n = p.numbers();
            assert!(n.iter().any(|(k, _)| k == "transition.duration_ms") && n.iter().any(|(k, _)| k == "interaction.press_ms"));
            let mut q = p.clone();
            q.set_number("transition.steps.0.a", 3.5).expect("sets");
            assert_eq!(q.transition.steps[0].a, 3.5);
            assert!(q.set_number("transition.nothing", 1.0).is_err());
        }
        assert!(Preset::from_json("{\"name\": 3}").is_err());
        // The finishing pass is data in the recipe too.
        let mut p = presets()[0].clone();
        p.set_number("post.dithr.levels", 3.0).expect("a post number");
        p.set_number("post.scanlines.strength", 0.25).expect("a post number");
        let q = Preset::from_recipe(&p.recipe()).expect("the recipe reads");
        assert_eq!(q.post, p.post);
        assert!(presets().iter().any(|p| p.post.palette.kind != crate::post::PaletteKind::Off && p.post.dithr.kind != crate::post::DithrKind::None));
        // An older preset without the field reads with everything off.
        let mut v: serde_json::Value = serde_json::from_str(&presets()[7].to_json()).expect("json");
        v.as_object_mut().expect("object").remove("post");
        assert_eq!(Preset::from_json(&v.to_string()).expect("parses").post, crate::post::PostSpec::default());
    }
}
