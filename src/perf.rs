//! Frame times, counters and the automatic quality level (DESIGN.md §1.12). No GPU here.
use std::collections::VecDeque;

/// Milliseconds on a monotonic clock: `performance.now()` on the web, `Instant` elsewhere.
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    { web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now()) }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

/// What one drawn frame cost on the CPU, ms: updating the state and laying out, building the scene
/// (instances), and recording and submitting the GPU work.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameCost { pub update_layout: f32, pub scene: f32, pub submit: f32, /** Of `update_layout`: shaping and breaking text. */ pub text: f32 }
impl FrameCost { pub fn total(&self) -> f32 { self.update_layout + self.scene + self.submit } }

/// Work counters, for tests and the readout: they only go up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    /// Full layout passes (taffy runs over the tree), and passes that found nothing to lay out.
    pub layouts: u64,
    pub layouts_skipped: u64,
    /// Paragraphs shaped and broken into lines by parley (a cache miss).
    pub shaped: u64,
    /// Views built (the application's `view` called).
    pub views: u64,
    /// Frames that viewed and drew only stages that stepped (`Ui::on_step`), the rest of the page kept
    /// from the last paint (not counted when that paint had to draw the page whole).
    pub restages: u64,
    /// Scenes built from the laid-out nodes.
    pub scenes: u64,
    /// Texts' words or lines found for `El::reveal_by` (a cache miss: once per text, width and face).
    pub reveals: u64,
    /// Instances whose ink the field map read for `creep` (one that had not changed since the last scene
    /// is not read again).
    pub inked: u64,
    /// Cached subtrees (`El::cached`) passed by in a scene: nothing they draw from changed, so they were
    /// neither drawn nor compared, and their plates were laid on as they were.
    pub kept: u64,
}

/// The readout's numbers. The runtime adds update / layout and scene time as it spends it; the
/// renderer adds the submit time and closes the frame.
#[derive(Clone, Debug, Default)]
pub struct Perf {
    pub pending: FrameCost,
    /// The drawn frames of the last two seconds: when (ms) and what they cost.
    pub frames: VecDeque<(f64, FrameCost)>,
    pub drawn: u64,
    pub skipped: u64,
    pub counters: Counters,
    /// What the host set: the graphics backend's name and the device pixel ratio.
    pub backend: String,
    /// The material resolution the last frame used, and whether it was chosen automatically.
    pub quality: f32,
    pub auto: bool,
    pub particles: u32,
    /// How the last frame's particles were simulated (`ParticleTier::name`, or "cpu").
    pub particle_path: &'static str,
    /// The adapter's name, as the host reports it.
    pub adapter: String,
    /// The last frame: the share of the screen redrawn (0..1), render passes recorded, an estimate of
    /// the renderer's texture memory (MB).
    pub dirty: f32,
    pub passes: u32,
    pub tex_mb: f32,
    /// Frames that came more than 1.5 frame periods (at 60 Hz) after the one before while frames were
    /// coming one after another (a gap over 250 ms is a pause, not a drop).
    pub dropped: u64,
    /// The stages' device scale (DESIGN.md §1.35) when it is below the device's own, device px a logical
    /// px; 0 at the device's scale.
    pub stage_scale: f32,
    /// How the last frame was drawn (`FramePath::line`).
    pub path: FramePath,
}
/// How a frame was drawn, for a readout that says why a frame cost what it did: whether it moved the
/// last frame (a scroll, `dirty::Move`) and, when it did not, the first reason; whether the material
/// pass was the one made for the material alone (`Renderer::specialise`) and, when not, why; the share of
/// the frame shaded at full resolution (the shade boxes, or all of it); and the scales of the softened
/// scroll, the stages and the reduced pass (1 at the device's own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramePath {
    pub moved: bool,
    /// Why the frame did not move the last one or was drawn whole: empty when neither was asked.
    pub why: &'static str,
    pub specialised: Option<u8>,
    /// The materials drawn by a pass each (a mask by id), when the page has several.
    pub gated: u16,
    pub spec_why: &'static str,
    pub shade: f32,
    pub soft: f32,
    pub stage: f32,
    pub reduced: f32,
    /// What put a partial frame's shaded boxes there: the three largest reasons, each with its boxes'
    /// share of the window (summed before they are merged, so they may overlap; `dirty::Parts::why`).
    pub shade_why: [(&'static str, f32); 3],
    /// The layouts (view and taffy) since the frame before, and where the first was asked for (the
    /// `Ui::relayout` call): a frame that only scrolled or repainted has none.
    pub layouts: u32,
    pub layout_from: Option<&'static std::panic::Location<'static>>,
}
impl Default for FramePath { fn default() -> Self { Self { moved: false, why: "", specialised: None, gated: 0, spec_why: "", shade: 0.0, soft: 1.0, stage: 1.0, reduced: 1.0, shade_why: [("", 0.0); 3], layouts: 0, layout_from: None } } }
/// The three largest reasons of `why` (pixels each) as shares of `whole` pixels, largest first.
pub fn top_why(why: &[(&'static str, f32)], whole: f32) -> [(&'static str, f32); 3] {
    let mut v: Vec<(&'static str, f32)> = why.iter().map(|&(w, a)| (w, (a / whole.max(1.0)).min(1.0))).collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    std::array::from_fn(|k| v.get(k).copied().unwrap_or(("", 0.0)))
}
impl FramePath {
    /// One line: `move yes  spec no (override)  shade 34% (band 20%, changed 9%, stage moved 5%)  soft 1  stage 0.75  lo 0.75`,
    /// then `layout 2 (src/ui.rs:2081)` when the frame was laid out.
    pub fn line(&self) -> String {
        let or = |w: &str| if w.is_empty() { String::new() } else { format!(" ({w})") };
        let names = |mask: u16| (0..16u8).filter(|m| mask & (1 << m) != 0).map(|m| crate::style::Material::from_id(m).name()).collect::<Vec<_>>().join("+");
        let spec = match (self.specialised, self.gated) { (Some(m), _) => format!("yes {}", crate::style::Material::from_id(m).name()), (None, g) if g != 0 => format!("each {}", names(g)), _ => format!("no{}", or(self.spec_why)) };
        let why: Vec<String> = self.shade_why.iter().filter(|w| !w.0.is_empty()).map(|w| format!("{} {:.0}%", w.0, w.1 * 100.0)).collect();
        let why = if why.is_empty() { String::new() } else { format!(" ({})", why.join(", ")) };
        let laid = match (self.layouts, self.layout_from) { (0, _) => String::new(), (n, Some(l)) => format!("  layout {n} ({}:{})", l.file(), l.line()), (n, None) => format!("  layout {n}") };
        format!("move {}  spec {spec}  shade {:.0}%{why}  soft {}  stage {}  lo {}{laid}", if self.moved { "yes".to_string() } else { format!("no{}", or(self.why)) }, self.shade * 100.0, self.soft, self.stage, self.reduced)
    }
}
/// The frame period the drop count measures against, ms.
pub const PERIOD_MS: f64 = 1000.0 / 60.0;
impl Perf {
    /// A frame was drawn now: what was spent since the last one is its cost.
    pub fn frame(&mut self, submit_ms: f32) {
        let mut c = std::mem::take(&mut self.pending);
        c.submit = submit_ms;
        crate::budget::feed(c.total());
        let now = now_ms();
        if let Some((t, _)) = self.frames.back() { let d = now - t; if d > PERIOD_MS * 1.5 && d < 250.0 { self.dropped += 1; } }
        self.frames.push_back((now, c));
        while self.frames.len() > 240 || self.frames.front().is_some_and(|(t, _)| now - t > 4000.0) { self.frames.pop_front(); }
        self.drawn += 1;
    }
    /// What a status line shows of the stages' scale (dim): `stage 1.5x` while it is below the device's
    /// own scale, nothing at it.
    pub fn stage_label(&self) -> Option<String> { (self.stage_scale > 0.0).then(|| format!("stage {}x", self.stage_scale)) }
    /// The interval between the last two frames, ms (the instant frame rate is 1000 over it).
    pub fn interval(&self) -> Option<f64> { let n = self.frames.len(); (n >= 2).then(|| self.frames[n - 1].0 - self.frames[n - 2].0) }
    /// Frames drawn in the last second (to `now` ms), and the last, mean and largest cost of each part.
    pub fn summary(&self, now: f64) -> Summary {
        let last: Vec<FrameCost> = self.frames.iter().filter(|(t, _)| now - t <= 1000.0).map(|f| f.1).collect();
        let n = last.len();
        let part = |f: fn(&FrameCost) -> f32| -> [f32; 3] {
            if n == 0 { return [0.0; 3]; }
            let v: Vec<f32> = last.iter().map(f).collect();
            [v[n - 1], v.iter().sum::<f32>() / n as f32, v.iter().copied().fold(0.0, f32::max)]
        };
        Summary { fps: n as u32, update_layout: part(|c| c.update_layout), scene: part(|c| c.scene), submit: part(|c| c.submit), total: part(FrameCost::total) }
    }
}

/// Last, mean, max (ms) over the frames of the last second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary { pub fps: u32, pub update_layout: [f32; 3], pub scene: [f32; 3], pub submit: [f32; 3], pub total: [f32; 3] }

/// The quality levels of the material pass: its resolution as a fraction of the device's.
pub const LEVELS: [f32; 3] = [1.0, 0.75, 0.5];

/// Automatic quality (DESIGN.md §1.12): the material's resolution while something moves (scrolling, or a
/// large loose area), by the intervals between frames, since a GPU's time is not measured on the web. It
/// goes by the frames drawn while something moves: one level down when the 95th percentile of the last 30
/// such intervals is above 1.5 display frames or 20 ms, whichever is less; one level up after 2 s of them
/// with no missed frame and every frame's cost under 60 % of the display frame. The level is kept for the
/// session. The display frame is the 10th percentile of the intervals of the last second, never under 4 ms.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutoQuality {
    pub level: usize,
    /// The last frame (ms); the last second's intervals; the moving frames since the last step (interval,
    /// cost); and since when the moving frames have all been in time and cheap (ms of moving frames).
    last: Option<f64>,
    second: VecDeque<(f64, f32)>,
    moving: VecDeque<f32>,
    calm_ms: f64,
    /// The display frame as last measured, ms.
    pub frame_ms: f32,
}
/// Moving frames looked at for a step down.
pub const MOVING_FRAMES: usize = 30;
/// Moving time in time and cheap before a step up, ms.
pub const CALM_MS: f64 = 2000.0;
impl AutoQuality {
    pub fn quality(&self) -> f32 { LEVELS[self.level.min(LEVELS.len() - 1)] }
    /// A frame drawn at `t` ms that cost `cost` ms, while something moved or not; whether the level
    /// stepped.
    pub fn feed(&mut self, t: f64, cost: f32, moving: bool) -> bool {
        let Some(prev) = self.last.replace(t) else { return false };
        let d = t - prev;
        if !(0.0..PAUSE_MS).contains(&d) { return false; }
        self.second.push_back((t, d as f32));
        while self.second.front().is_some_and(|f| t - f.0 > 1000.0) { self.second.pop_front(); }
        if self.second.len() >= 10 { self.frame_ms = pct(self.second.iter().map(|f| f.1).collect(), 0.1).max(LEAST_FRAME_MS); }
        if !moving || self.frame_ms <= 0.0 { return false; }
        let miss = (self.frame_ms * 1.5).min(SLOWEST_MS);
        self.moving.push_back(d as f32);
        if self.moving.len() > MOVING_FRAMES { self.moving.pop_front(); }
        self.calm_ms = if d as f32 > miss || cost > self.frame_ms * 0.6 { 0.0 } else { self.calm_ms + d };
        if self.moving.len() >= MOVING_FRAMES && self.level + 1 < LEVELS.len() && pct(self.moving.iter().copied().collect(), 0.95) > miss {
            self.level += 1;
            self.moving.clear();
            self.calm_ms = 0.0;
            return true;
        }
        if self.level > 0 && self.calm_ms >= CALM_MS {
            self.level -= 1;
            self.moving.clear();
            self.calm_ms = 0.0;
            return true;
        }
        false
    }
}

/// The device scales a stage steps through when the machine cannot draw it in time (DESIGN.md §1.35),
/// below the device's own.
pub const STAGE_SCALES: [f32; 4] = [2.0, 1.5, 1.0, 0.75];
/// A frame interval longer than this is a pause, not a frame, ms.
const PAUSE_MS: f64 = 250.0;
/// The display frame is never taken as shorter than this, ms.
const LEAST_FRAME_MS: f32 = 4.0;
/// The interval a piece must not run under for long while a lower scale is available (50 fps), ms.
const SLOWEST_MS: f32 = 20.0;
/// The steps up a stage's scale takes at most until `StageScale::reset` (a route's visit), all levels
/// together.
pub const STAGE_UPS: u8 = 2;
/// The wait for a step up with no missed frame, and the time after one within which a step down holds
/// the lower level, ms.
const UP_WAIT_MS: f64 = 5000.0;
/// A step up is taken when the frames' cost there, by the new scale's pixels, would be under this share
/// of the display frame.
const UP_FIT: f32 = 0.85;

/// The stage's device scale (DESIGN.md §1.35): one step down when the 95th percentile frame interval
/// over 3 s is above 1.5 display frames (or 50 fps); one step up after 5 s with no missed frame and the
/// frames' cost under 60 % of the display frame, when the 95th percentile cost there times the pixels'
/// ratio (the new scale over the old, squared) is under 85 % of it, never while a pointer is down. At
/// most `STAGE_UPS` steps up until `reset` (a new route, a new window size); a step down within 5 s of a
/// step up holds the lower level until then too. Every step is a cut.
#[derive(Clone, Debug, Default)]
pub struct StageScale {
    /// The device's scale and the scales for it: its own, then each of `STAGE_SCALES` below it.
    device: f32,
    levels: Vec<f32>,
    /// The index into `levels`.
    pub level: usize,
    /// How many times it stepped up, and when it last did (ms).
    ups: u8,
    up_at: Option<f64>,
    /// It steps up no more until `reset`: a step down came within `UP_WAIT_MS` of a step up, or it has
    /// taken `STAGE_UPS`.
    pub held: bool,
    /// The last frame (ms), and the frames since the last step: when, the interval before it, its cost.
    last: Option<f64>,
    frames: VecDeque<(f64, f32, f32)>,
    /// The display frame as last measured, ms.
    pub frame_ms: f32,
    /// When it last stepped, and when a frame last missed or cost over 60 % of the display frame.
    stepped: f64,
    over: f64,
    /// It has had frames to go by (with none it never holds a stage back from being held).
    pub fed: bool,
    /// How many steps it has taken.
    pub steps: u32,
}
impl StageScale {
    /// The scales for a device of scale `device`.
    pub fn levels(device: f32) -> Vec<f32> { std::iter::once(device).chain(STAGE_SCALES.iter().copied().filter(|&s| s < device - 0.01)).collect() }
    /// From the device's own scale again, forgetting the steps up.
    pub fn reset(&mut self) { let d = self.device; *self = Self::default(); self.device = d; self.levels = Self::levels(d); }
    /// The stage's scale now, device px a logical px.
    pub fn scale(&self) -> f32 { self.levels.get(self.level).copied().unwrap_or(self.device.max(1.0)) }
    /// Whether it is as low as it goes.
    pub fn lowest(&self) -> bool { self.level + 1 >= self.levels.len() }
    /// A frame drawn at `t` ms that cost `cost` ms on a device of scale `device`, with a pointer down or
    /// not. Whether the scale stepped.
    pub fn feed(&mut self, t: f64, cost: f32, device: f32, pointer_down: bool) -> bool {
        if (device - self.device).abs() > 1e-3 || self.levels.is_empty() { self.device = device; self.reset(); }
        self.fed = true;
        let Some(prev) = self.last.replace(t) else { self.stepped = t; self.over = t; return false };
        let d = t - prev;
        if !(0.0..PAUSE_MS).contains(&d) { return false; }
        self.frames.push_back((t, d as f32, cost));
        while self.frames.front().is_some_and(|f| t - f.0 > 20_000.0) { self.frames.pop_front(); }
        // The display frame: the 10th percentile of the last second's intervals.
        let second: Vec<f32> = self.frames.iter().filter(|f| t - f.0 <= 1000.0).map(|f| f.1).collect();
        if second.len() >= 10 { self.frame_ms = pct(second, 0.1).max(LEAST_FRAME_MS); }
        if self.frame_ms <= 0.0 { return false; }
        let miss = (self.frame_ms * 1.5).min(SLOWEST_MS);
        if d as f32 > miss || cost > self.frame_ms * 0.6 { self.over = t; }
        if t - self.stepped >= 3000.0 && !self.lowest() {
            let three: Vec<f32> = self.frames.iter().filter(|f| t - f.0 <= 3000.0).map(|f| f.1).collect();
            if pct(three, 0.95) > miss {
                // Down again soon after going up: the level up does not fit, and is not tried again.
                if self.up_at.is_some_and(|u| t - u < UP_WAIT_MS) { self.held = true; }
                return self.step(t, self.level + 1);
            }
        }
        if self.level > 0 && !pointer_down && !self.held && t - self.stepped.max(self.over) >= UP_WAIT_MS {
            // What the frames would cost at the scale up, by its pixels.
            let r = self.levels[self.level - 1] / self.levels[self.level];
            let costs: Vec<f32> = self.frames.iter().filter(|f| t - f.0 <= UP_WAIT_MS).map(|f| f.2).collect();
            if pct(costs, 0.95) * r * r < UP_FIT * self.frame_ms {
                self.ups += 1;
                self.up_at = Some(t);
                self.held = self.ups >= STAGE_UPS;
                return self.step(t, self.level - 1);
            }
        }
        false
    }
    /// Go to `level` now, as a step does (for checks: the next step is no sooner than one would be).
    pub fn set_level(&mut self, level: usize) { let t = self.last.unwrap_or(0.0); self.step(t, level.min(self.levels.len().saturating_sub(1))); }
    fn step(&mut self, t: f64, to: usize) -> bool {
        self.level = to;
        self.stepped = t;
        self.over = t;
        self.frames.clear();
        self.steps += 1;
        true
    }
}
/// The `q` quantile of `v` (by rank).
fn pct(mut v: Vec<f32>, q: f32) -> f32 {
    if v.is_empty() { return 0.0; }
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q).round() as usize]
}
