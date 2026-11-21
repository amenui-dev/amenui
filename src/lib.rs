extern crate self as amenui;

pub mod add;
pub mod app;
pub mod audit;
pub mod beat;
pub mod budget;
pub mod decode;
pub mod dirty;
pub mod draw;
pub mod el;
pub mod error;
pub mod experiment;
pub mod field;
pub mod fieldmap;
pub mod strand;
pub mod gaze;
pub mod form;
pub mod fasthash;
pub mod gesture;
pub mod hit;
pub mod i18n;
pub mod markdown;
pub mod materials;
pub mod marks;
pub mod mesh;
pub mod morph;
pub mod pair;
pub mod motes;
pub mod perf;
pub mod pictures;
pub mod post;
pub mod preset;
#[cfg(not(target_arch = "wasm32"))]
pub mod offscreen;
#[cfg(not(target_arch = "wasm32"))]
pub mod drive;
pub mod recipes;
#[cfg(not(target_arch = "wasm32"))]
pub mod sample;
pub mod sources;
pub mod stage;
/// AGENTS.md, so `cargo test` compiles every example in it.
#[cfg(doctest)]
#[doc = include_str!("../AGENTS.md")]
pub mod agents {}
/// README.md, so its quick start is compiled and run.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub mod readme {}
pub mod render;
pub mod scene;
pub mod style;
pub mod text;
pub mod tree;
pub mod ui;
pub mod widgets;
pub mod warmup;
pub mod wrap;
