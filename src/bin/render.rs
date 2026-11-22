//! One deterministic frame of any recipe, and its tree (DESIGN.md §1.17). Fixed 60 Hz
//! clock from 0, fixed seeds: the same arguments give byte-identical PNGs.
//!
//! ```text
//! usage: render --page <recipe-name> [--size 1280x800] [--scale 1] [--preset <hash|file.json>] [--material <id>] [--theme <id>] [--locale en|zh-Hans]
//!               [--at <ms>] [--events <file.json>] [--out out.png] [--tree out.json] [--strict]
//!        render --page <recipe-name> --frames <from>..<to> [--fps 60] --out dir/%05d.png [...]
//! ```
//!
//! `--frames` draws a run of frames on the same fixed clock (`amenui::drive::frames`): frame `n` at
//! `n / fps` s, `--fps` one of 60, 30, 20, 15 or 12, each written where `--out` says with `%05d` (or
//! `%d`, `%04d`…) replaced by the frame's number. With `--audit` every frame is measured and the most
//! serious finding is reported with its frame.
//!
//! `--events` is a JSON list such as `[{"at_ms": 200, "kind": "press", "target": "button/live"},
//! {"at_ms": 260, "kind": "release", "target": "button/live"}]` (kinds: press, release, move, key, wheel,
//! text; see `amenui::drive::Input`). Problems in the view (duplicate keys…) are printed; `--strict` makes
//! them an error. `--audit` measures the frame (`amenui::audit`: text cut off, contrast, glyphs over each
//! other, text too small, a page that never settles) and prints the findings, most serious first;
//! with `--strict`, a P0 or P1 finding is an error too. `--preset` draws the page in a preset (a built-in
//! one's hash, such as `normcore`, or a preset's JSON file) and holds it to the preset's rules as well
//! (`amenui::preset::Rule`: contrast, text size, margin, measure, accent).
use amenui::drive::{RenderSpec, parse_events};

const USAGE: &str = "usage: render --page <recipe-name> [--size 1280x800] [--scale 1] [--preset <hash|file.json>] [--material <id>] [--theme <id>] [--locale en|zh-Hans] [--at <ms> | --frames <from>..<to> [--fps 60]] [--events <file.json>] [--out out.png | dir/%05d.png] [--tree out.json] [--audit] [--strict]";

/// `--out` with the frame's number put in for `%d` or `%0Nd`.
fn numbered(pattern: &str, n: u32) -> Option<String> {
    let at = pattern.find('%')?;
    let rest = &pattern[at + 1..];
    let end = rest.find('d')?;
    let spec = &rest[..end];
    let width: usize = if spec.is_empty() { 0 } else { spec.strip_prefix('0').unwrap_or(spec).parse().ok()? };
    Some(format!("{}{n:0width$}{}", &pattern[..at], &rest[end + 1..]))
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("render: {msg}");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut spec = RenderSpec { page: String::new(), w: 1280, h: 800, scale: 1.0, material: None, theme: None, at_ms: 1000.0, events: vec![], locale: None };
    let (mut out, mut tree, mut strict, mut audited, mut preset) = ("out.png".to_owned(), None, false, false, None::<String>);
    let (mut frames, mut fps, mut at_given) = (None, 60u32, false);
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if flag == "--strict" { strict = true; i += 1; continue; }
        if flag == "--audit" { audited = true; i += 1; continue; }
        if flag == "--help" || flag == "-h" { println!("{USAGE}"); return; }
        let Some(v) = args.get(i + 1).cloned() else { fail(format!("`{flag}` needs a value.\n{USAGE}")) };
        match flag {
            "--page" => spec.page = v,
            "--size" => {
                let (w, h) = v.split_once('x').unwrap_or_else(|| fail(format!("`--size {v}`: write width x height in device px, e.g. `--size 1280x800`.")));
                spec.w = w.parse().unwrap_or_else(|_| fail(format!("`--size {v}`: `{w}` is not a whole number of px.")));
                spec.h = h.parse().unwrap_or_else(|_| fail(format!("`--size {v}`: `{h}` is not a whole number of px.")));
            }
            "--scale" => spec.scale = v.parse().unwrap_or_else(|_| fail(format!("`--scale {v}`: write device px per logical px, e.g. 1 or 2."))),
            "--preset" => preset = Some(v),
            "--material" => spec.material = Some(v),
            "--theme" => spec.theme = Some(v),
            "--locale" => spec.locale = Some(v),
            "--at" => { at_given = true; spec.at_ms = v.parse().unwrap_or_else(|_| fail(format!("`--at {v}`: write the time of the frame in ms, e.g. 1000."))) }
            "--frames" => {
                let bad = format!("`--frames {v}`: write the first frame and one past the last, e.g. `--frames 0..120`.");
                let (a, b) = v.split_once("..").unwrap_or_else(|| fail(&bad));
                frames = Some(a.parse::<u32>().unwrap_or_else(|_| fail(&bad))..b.parse::<u32>().unwrap_or_else(|_| fail(&bad)));
            }
            "--fps" => fps = v.parse().unwrap_or_else(|_| fail(format!("`--fps {v}`: use 60, 30, 20, 15 or 12."))),
            "--events" => {
                let json = std::fs::read_to_string(&v).unwrap_or_else(|e| fail(format!("`--events {v}` could not be read ({e}).")));
                spec.events = parse_events(&json, &v).unwrap_or_else(|e| fail(e));
            }
            "--out" => out = v,
            "--tree" => tree = Some(v),
            _ => fail(format!("unknown flag `{flag}`.\n{USAGE}")),
        }
        i += 2;
    }
    if spec.page.is_empty() { fail(format!("`--page` is required.\n{USAGE}")); }
    let faces = amenui::text::load_bundled(concat!(env!("CARGO_MANIFEST_DIR"), "/assets")).unwrap_or_else(|e| fail(e));
    // A preset: every page opened in it, its rules in the audit.
    let preset = preset.map(|p| amenui::drive::preset_named(&p).unwrap_or_else(|e| fail(e)));
    let in_preset = preset.as_ref().map(|p| amenui::drive::in_preset(p, &amenui::drive::open_in));
    let open: amenui::drive::Opener = match &in_preset { Some(f) => &**f, None => &amenui::drive::open_in };
    if let Some(range) = frames {
        if at_given { fail("give `--at` or `--frames`, not both."); }
        if numbered(&out, 0).is_none() { fail(format!("`--out {out}`: with `--frames`, put `%05d` where the frame's number goes, e.g. `--out frames/%05d.png`.")); }
        let (mut worst, mut fails, mut written, mut problems) = (None::<(u32, amenui::audit::Finding)>, false, 0u32, false);
        let mut each = |n: u32, r: &amenui::drive::Rendered, f: &[amenui::audit::Finding]| {
            let path = numbered(&out, n).expect("checked above");
            if let Some(dir) = std::path::Path::new(&path).parent() && !dir.as_os_str().is_empty() { let _ = std::fs::create_dir_all(dir); }
            std::fs::write(&path, &r.png).unwrap_or_else(|e| fail(format!("`{path}` could not be written ({e}).")));
            written += 1;
            problems |= !r.problems.is_empty();
            let measured: Vec<_> = f.iter().filter(|f| f.kind != amenui::audit::Kind::Structure).cloned().collect();
            fails |= amenui::audit::fails(&measured);
            if let Some(m) = measured.first() && worst.as_ref().is_none_or(|(_, w)| m.level < w.level) { worst = Some((n, m.clone())); }
        };
        if audited { amenui::drive::audit_frames(&spec, range.clone(), fps, faces, None, open, &mut each) }
        else { amenui::drive::frames(&spec, range.clone(), fps, faces, None, open, &mut |n, r| each(n, r, &[])) }.unwrap_or_else(|e| fail(e));
        if audited {
            match &worst { Some((n, f)) => println!("audit: {}: the most serious, frame {n}: {f}  [{:.2} / {:.2}]", spec.page, f.measured, f.limit), None => println!("audit: {}: frames {}..{}: nothing found", spec.page, range.start, range.end) }
        }
        eprintln!("render: {} {}x{} frames {}..{} at {fps} fps -> {written} files ({out})", spec.page, spec.w, spec.h, range.start, range.end);
        if strict && (problems || fails) { std::process::exit(1); }
        return;
    }
    let (r, findings) = if audited { amenui::drive::audit_with(&spec, faces, None, open) } else { amenui::drive::render_with(&spec, faces, None, open).map(|r| (r, vec![])) }.unwrap_or_else(|e| fail(e));
    for p in &r.problems { eprintln!("render: problem: {p}"); }
    if audited {
        // The view's problems are in the problems above; the audit lists them too, so leave them out here.
        let measured: Vec<_> = findings.iter().filter(|f| f.kind != amenui::audit::Kind::Structure).collect();
        if measured.is_empty() { println!("audit: {}: nothing found", spec.page); }
        for f in &measured { println!("{f}  [{:.2} / {:.2}]", f.measured, f.limit); }
    }
    std::fs::write(&out, &r.png).unwrap_or_else(|e| fail(format!("`--out {out}` could not be written ({e}).")));
    if let Some(t) = tree {
        let json = serde_json::to_string_pretty(&r.tree).unwrap_or_else(|e| fail(e));
        std::fs::write(&t, json).unwrap_or_else(|e| fail(format!("`--tree {t}` could not be written ({e}).")));
    }
    eprintln!("render: {} {}x{} at {} ms -> {out}", spec.page, spec.w, spec.h, spec.at_ms);
    if strict && (!r.problems.is_empty() || amenui::audit::fails(&findings)) { std::process::exit(1); }
}
