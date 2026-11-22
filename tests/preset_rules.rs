//! A preset's rules (`preset::Rule`, DESIGN.md §7.2): each of the five, on a page broken for it on
//! purpose, reports one P1 with what was measured and the rule's limit, and the same page without the
//! rule reports nothing of it; a rule and the audit's own floor finding the same thing report once; a
//! preset whose rules are looser than the floors is refused when it is read; an older preset reads with
//! no rules; every recipe in each of the ten presets has no P0 or P1. Needs a GPU adapter (lavapipe will
//! do); without one the tests say so and pass.
use amenui::audit::{Finding, Kind, Level};
use amenui::drive::{Driven, RenderSpec, audit_with, in_preset, open_in};
use amenui::el::{El, Paint, col, space, text};
use amenui::offscreen::Offscreen;
use amenui::preset::{Of, Preset, Rule, presets};
use amenui::ui::Ui;

fn fonts() -> Vec<amenui::text::Face> { amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).expect("fonts") }

const W: u32 = 640;
const H: u32 = 360;
const PROSE: &str = "Running text set in the dim grey, long enough to be prose on its own line here.";

fn dim_prose(_: &()) -> El<()> { col().pad(40.0, 40.0).child(text(PROSE, 14.0, Paint::DIM).width(520.0).key("prose")) }
fn ten_px(_: &()) -> El<()> { col().pad(40.0, 40.0).child(text("a caption drawn ten px tall", 10.0, Paint::TEXT).key("caption")) }
fn near_top(_: &()) -> El<()> { col().pad(40.0, 6.0).child(text("A title close to the top", 20.0, Paint::TEXT).key("title")) }
fn wide(_: &()) -> El<()> {
    let s = "A line of running text that goes on and on, well past what a comfortable column would hold for reading.";
    col().pad(20.0, 40.0).child(text(s, 12.0, Paint::TEXT).width(600.0).key("wide"))
}
fn accented(_: &()) -> El<()> { col().pad(40.0, 40.0).gap(12.0).child(text("Accent below", 15.0, Paint::TEXT).key("label")).child(space(200.0, 60.0).fill(Paint::ACCENT).key("block")) }
fn grey(_: &()) -> El<()> { col().pad(40.0, 40.0).child(text(PROSE, 14.0, Paint::tone(0.18)).width(520.0).key("grey")) }

/// A page broken for one rule.
type View = fn(&()) -> El<()>;

/// Normcore (plain, light) with only `rules`.
fn with(rules: Vec<Rule>) -> Preset {
    let mut p = presets().into_iter().find(|p| p.hash == "normcore").expect("normcore");
    p.rules = rules;
    p
}

fn findings(o: &mut Offscreen, view: View, preset: &Preset) -> Vec<Finding> {
    let mut ui = Ui::new((), view, |_, _| {}, fonts(), W, H, 1.0);
    for k in 0..=60 { ui.tick(k as f32 / 60.0); }
    ui.settle();
    let look = preset.apply(&Driven::look(&ui));
    let px = o.render(&mut ui, &look);
    ui.audit_under(o, &look, &px, preset)
}
fn of(f: &[Finding], kind: Kind, key: &str) -> Vec<Finding> { f.iter().filter(|f| f.kind == kind && f.key == key).cloned().collect() }

#[test]
fn each_rule_on_a_page_broken_for_it_reports_one_p1_and_none_without_it() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let cases: [(&str, View, Rule, Kind, &str, &str); 5] = [
        ("contrast", dim_prose, Rule::Contrast { of: Of::Prose, min: 7.0 }, Kind::Contrast, "prose", "needs 7:1 for prose"),
        ("text_size", ten_px, Rule::TextSize { min: 12.0 }, Kind::Small, "caption", "drawn 10.0 px tall (Normcore needs 12)"),
        ("margin", near_top, Rule::Margin { min: 24.0 }, Kind::Margin, "title", "from the window's top edge (Normcore needs 24): move it down"),
        ("measure", wide, Rule::Measure { max: 80.0 }, Kind::Measure, "wide", "(Normcore allows 80): make the column about"),
        ("accent", accented, Rule::Accent { max: 0.01 }, Kind::Accent, "", "of the frame at rest (Normcore allows 1%)"),
    ];
    for (name, view, rule, kind, key, says) in cases {
        let f = findings(&mut o, view, &with(vec![rule]));
        let found = of(&f, kind, key);
        assert_eq!(found.len(), 1, "{name}: one finding: {f:#?}");
        let x = &found[0];
        assert_eq!(x.level, Level::P1, "{name}: {x}");
        assert!(x.fix.contains(says), "{name}: {}", x.fix);
        let limit = match rule { Rule::Contrast { min, .. } | Rule::TextSize { min } | Rule::Margin { min } => min, Rule::Measure { max } | Rule::Accent { max } => max };
        assert_eq!(x.limit, limit, "{name}");
        let broken = match rule { Rule::Measure { .. } | Rule::Accent { .. } => x.measured > limit, _ => x.measured < limit };
        assert!(broken, "{name}: measured {} against {limit}", x.measured);
        // Without the rule the audit's own floors do not see it.
        let plain = findings(&mut o, view, &with(vec![]));
        assert!(of(&plain, kind, key).iter().all(|f| f.level > Level::P1), "{name} without the rule: {plain:#?}");
        assert!(!amenui::audit::fails(&plain), "{name}: the page is otherwise clean: {plain:#?}");
    }
}

#[test]
fn a_rule_and_the_floor_on_the_same_text_report_once_at_the_floor_s_level() {
    let Ok(mut o) = Offscreen::try_new(W, H) else { eprintln!("no GPU adapter here: skipped"); return };
    let f = findings(&mut o, grey, &with(vec![Rule::Contrast { of: Of::Prose, min: 7.0 }, Rule::Contrast { of: Of::Read, min: 4.5 }]));
    let found = of(&f, Kind::Contrast, "grey");
    assert_eq!(found.len(), 1, "{f:#?}");
    assert!(found[0].measured < 3.0, "the floor's P0: {}", found[0]);
    assert_eq!((found[0].level, found[0].limit), (Level::P0, 7.0), "{}", found[0]);
    assert!(found[0].fix.contains("Normcore needs 7:1 for prose"), "{}", found[0].fix);
}

#[test]
fn a_preset_looser_than_the_floors_is_refused_and_an_older_one_reads_with_no_rules() {
    let mut p = with(vec![Rule::Contrast { of: Of::Read, min: 4.0 }]);
    let e = Preset::from_json(&p.to_json()).expect_err("refused");
    assert_eq!(e.to_string(), "preset \"Normcore\": contrast.min 4 is below the audit's floor (4.5). A preset's rules can only be stricter.");
    p.rules = vec![Rule::TextSize { min: 8.0 }];
    assert!(Preset::from_json(&p.to_json()).expect_err("refused").to_string().contains("text_size.min 8 is below the audit's floor (9)"));
    p.rules = vec![Rule::Accent { max: 1.5 }];
    assert!(Preset::from_json(&p.to_json()).is_err());
    // A recipe that loosens a built-in preset's rule is refused too.
    let mut q = presets()[0].clone();
    q.rules[1] = Rule::Contrast { of: Of::Read, min: 3.0 };
    assert!(Preset::from_recipe(&q.recipe()).is_err());
    // Every built-in preset carries rules, and they pass.
    for p in presets() { assert!(p.rules.len() >= 5, "{}", p.name); p.check().expect("tight"); }
    // A preset written before rules reads with none.
    let mut v: serde_json::Value = serde_json::from_str(&presets()[3].to_json()).expect("json");
    v.as_object_mut().expect("object").remove("rules");
    assert!(Preset::from_json(&v.to_string()).expect("reads").rules.is_empty());
}

#[test]
fn every_recipe_in_each_preset_has_no_p0_or_p1() {
    let Ok(mut o) = Offscreen::try_new(800, 600) else { eprintln!("no GPU adapter here: skipped"); return };
    let mut bad = vec![];
    for page in amenui::recipes::NAMES {
        for p in presets() {
            let spec = RenderSpec { page: format!("recipe-{page}"), w: 800, h: 600, scale: 1.0, material: None, theme: None, at_ms: 1500.0, events: vec![], locale: None };
            let (_, f) = audit_with(&spec, fonts(), Some(&mut o), &*in_preset(&p, &open_in)).expect("audit");
            bad.extend(f.into_iter().filter(|f| matches!(f.level, Level::P0 | Level::P1)).map(|f| format!("{page} {}: {f}  [{:.2} / {:.2}]", p.hash, f.measured, f.limit)));
        }
    }
    assert!(bad.is_empty(), "{} findings:\n{}", bad.len(), bad.join("\n"));
}
