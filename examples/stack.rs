//! Recipe: a stack of cards that fans under the pointer (docs/recipes/stack.md). The angles are state:
//! the 3D element reports which card the pointer is on (`Pick`), `Stack::update` schedules each card
//! beneath with its own delay, and the tick steps their springs only while one moves.
use amenui::el::{El, Font, Paint, col, text};
use amenui::text::Face;
use amenui::ui::Ui;
use amenui::widgets::{Stack, StackMsg, card_stack};

pub struct App { pub stack: Stack, pub now: f32 }

#[derive(Clone, Debug, PartialEq)]
pub enum Msg { Stack(StackMsg) }

pub fn view(s: &App) -> El<Msg> {
    let st = &s.stack;
    col().key("page").pad(24.0, 24.0).gap(12.0)
        .child(text("A stack of cards", 20.0, Paint::INK).font(Font::Display).key("title"))
        .child(card_stack("deck", st, 480.0, 320.0, Msg::Stack))
        // The formula, from the same fields.
        .child(text(format!("angle(n) = {:.0}° × {:.2}^n · delay(n) = {:.0} ms × n", st.angle0, st.falloff, st.delay_ms), 12.0, Paint::DIM).font(Font::Mono).key("formula"))
}

pub fn update(s: &mut App, m: Msg) {
    let Msg::Stack(m) = m;
    s.stack.update(m, s.now);
}

pub fn ui(faces: Vec<Face>, w: u32, h: u32, scale: f32) -> Ui<App, Msg> {
    Ui::new(App { stack: Stack::default(), now: 0.0 }, view, update, faces, w, h, scale)
        .on_tick(|s, t| { s.now = t; s.stack.step(t) })
        .on_wake(|s, t| s.stack.moving().then_some(t))
}
