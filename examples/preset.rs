//! Recipe: a custom preset (docs/recipes/preset.md). A preset is data: start from one of the twelve, change
//! its numbers (or read it from JSON), and give the runtime the look it makes. Its transition plays when
//! a commit applies it — here a button that switches between the plain look and the custom one, and one
//! that cracks the page open onto a third.
use amenui::el::{El, Paint, col, row, text};
use amenui::preset::{Preset, Step, StepKind, Transition, presets};
use amenui::style::Look;
use amenui::text::Face;
use amenui::ui::Ui;
use amenui::widgets::{Variant, button_as};

pub struct App { pub custom: Preset, pub on: bool, pub cracked: Preset, pub crack: bool }

#[derive(Clone, Debug, PartialEq)]
pub enum Msg { Switch, Crack }

/// Bauhaus's look, arriving by rows of 18 px that reveal top to bottom, softly, in 600 ms.
pub fn custom() -> Preset {
    let mut p = presets().into_iter().find(|p| p.hash == "bauhaus").expect("a preset");
    p.name = "Rows".into();
    p.transition = Transition::of(600.0, &[Step { kind: StepKind::Reveal, from: 0.0, to: 1.0, a: 2.0, b: 0.0, c: 18.0, d: 24.0, e: 0.0 }]);
    p.interaction.press_ms = 240.0;
    p
}

/// Tech-Manual's look (dark ink on light paper), arriving through cracks: the old page splits into
/// long pieces leaning to 70°, and the cracks widen until only the new page is left, in 800 ms.
pub fn cracked() -> Preset {
    let mut p = presets().into_iter().find(|p| p.hash == "tech-manual").expect("a preset");
    p.name = "Crack".into();
    let [a, b, c, d, e] = StepKind::CRACK;
    p.transition = Transition::of(800.0, &[Step { kind: StepKind::Crack, from: 0.0, to: 1.0, a, b, c, d, e }]);
    p
}

pub fn view(s: &App) -> El<Msg> {
    col().key("page").pad(32.0, 28.0).gap(16.0)
        .child(text(if s.crack { "Cracked open" } else if s.on { "A style of its own" } else { "As it was" }, 28.0, Paint::INK).key("title"))
        .child(text("Press the button: the new look arrives through its own transition.", 15.0, Paint::TEXT))
        // A commit: the frame as it was stays, and the preset's transition reveals the new one.
        .child(row().gap(12.0)
            .child(button_as("switch", "Switch", Variant::Primary, Msg::Switch).commit())
            .child(button_as("crack", "Crack", Variant::Default, Msg::Crack).commit()))
}

pub fn update(s: &mut App, m: Msg) {
    match m { Msg::Switch => { s.on = !s.on; s.crack = false } Msg::Crack => s.crack = !s.crack }
}

fn look(s: &App) -> Look {
    if s.crack { s.cracked.apply(&Look::default()) } else if s.on { s.custom.apply(&Look::default()) } else { Look::default() }
}

pub fn ui(faces: Vec<Face>, w: u32, h: u32, scale: f32) -> Ui<App, Msg> {
    Ui::new(App { custom: custom(), on: false, cracked: cracked(), crack: false }, view, update, faces, w, h, scale).on_look(look)
}
