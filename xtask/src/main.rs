//! Project automation.
//!
//!   cargo xtask test          full workspace test suite
//!   cargo xtask test --full   also the model-gated tests (real model + worker)
//!   cargo xtask parity        PARITY.md / Handy-coverage gate
//!   cargo xtask gate          test + parity (what CI runs)

use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut c: Command) -> bool {
    eprintln!("$ {:?}", c);
    c.status().map(|s| s.success()).unwrap_or(false)
}

/// Locate a Nemotron (or any GGUF) model in the Hugging Face cache.
fn find_test_model() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("GUIDY_TEST_MODEL") {
        return Some(p.into());
    }
    let hf = std::env::var_os("HF_HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/huggingface"))
    })?;
    let hub = hf
        .join("hub")
        .join("models--handy-computer--nemotron-3.5-asr-streaming-0.6b-gguf/snapshots");
    for snap in std::fs::read_dir(hub).ok()?.flatten() {
        let f = snap
            .path()
            .join("nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf");
        if f.exists() {
            return Some(f);
        }
    }
    None
}

fn test(full: bool) -> bool {
    let mut c = cargo();
    c.args(["test", "--workspace"]);
    if full {
        let Some(model) = find_test_model() else {
            eprintln!("--full needs a model: set GUIDY_TEST_MODEL or download Nemotron Streaming 3.5 in Guidy");
            return false;
        };
        if !run({
            let mut b = cargo();
            b.args(["build", "-p", "guidy-app", "--bin", "guidy"]);
            b
        }) {
            return false;
        }
        let exe =
            root()
                .join("target/debug")
                .join(if cfg!(windows) { "guidy.exe" } else { "guidy" });
        c.env("GUIDY_TRANSCRIBE_WORKER_EXE", exe)
            .env("GUIDY_TEST_MODEL", model)
            .env(
                "GUIDY_TEST_WAV",
                root().join("fixtures/audio/speech-16k-mono.wav"),
            )
            .args(["--", "--include-ignored", "--test-threads=1"]);
    }
    run(c)
}

/// All test names in the workspace (`module::path::name`).
fn list_tests() -> BTreeSet<String> {
    let out = cargo()
        .args(["test", "--workspace", "--", "--list", "--format", "terse"])
        .output()
        .expect("cargo test --list");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_suffix(": test"))
        .map(str::to_string)
        .collect()
}

fn read(p: &str) -> String {
    std::fs::read_to_string(root().join(p)).unwrap_or_else(|e| panic!("read {p}: {e}"))
}

fn source_text(dirs: &[&str]) -> String {
    fn walk(dir: &Path, out: &mut String) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
            }
        }
    }
    let mut s = String::new();
    for d in dirs {
        walk(&root().join(d), &mut s);
    }
    s
}

/// Does a test gated to `platform` (see docs/handy-platform-tests.txt) build
/// on this host? xtask runs on the platform it tests.
fn built_here(platform: &str) -> bool {
    match platform {
        "macos-aarch64" => cfg!(all(target_os = "macos", target_arch = "aarch64")),
        "macos" => cfg!(target_os = "macos"),
        "unix" => cfg!(unix),
        "not-linux" => !cfg!(target_os = "linux"),
        other => panic!("handy-platform-tests.txt: unknown platform `{other}`"),
    }
}

/// Platform-gated tests that don't build on this host (full paths).
fn not_built_here() -> BTreeSet<String> {
    read("docs/handy-platform-tests.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(' '))
        .filter(|(platform, _)| !built_here(platform))
        .map(|(_, test)| test.trim().to_string())
        .collect()
}

fn parity() -> bool {
    let mut ok = true;
    let tests = list_tests();
    let elsewhere = not_built_here();
    let elsewhere_short: BTreeSet<&str> = elsewhere
        .iter()
        .map(|t| t.rsplit("::").next().unwrap())
        .collect();
    let short: BTreeSet<String> = tests
        .iter()
        .map(|t| t.rsplit("::").next().unwrap().to_string())
        .collect();

    // 1. Every Handy unit test survives (renamed module only).
    let baseline = read("docs/handy-baseline-tests.txt");
    let missing: Vec<_> = baseline
        .lines()
        .filter(|t| !elsewhere.contains(*t))
        .map(|l| l.replace("tauri_impl", "global_hotkey_impl"))
        .filter(|t| !tests.contains(t))
        .collect();
    if missing.is_empty() {
        let skipped = baseline.lines().filter(|t| elsewhere.contains(*t)).count();
        println!(
            "✓ all {} Handy tests are present{}",
            baseline.lines().count() - skipped,
            if skipped > 0 {
                format!(" ({skipped} more only build on other platforms)")
            } else {
                String::new()
            }
        );
    } else {
        ok = false;
        println!("✗ Handy tests missing: {missing:?}");
    }

    // 2. Every Handy command has an implementation.
    let src = source_text(&["crates/guidy-core/src", "crates/guidy-app/src"]);
    let commands = read("docs/handy-commands.txt");
    let missing: Vec<&str> = commands
        .lines()
        .filter(|c| {
            !c.is_empty()
                && !src.contains(&format!("fn {c}("))
                && !src.contains(&format!("fn {c}<"))
        })
        .collect();
    if missing.is_empty() {
        println!(
            "✓ all {} Handy commands are implemented",
            commands.lines().count()
        );
    } else {
        ok = false;
        println!("✗ commands without an implementation: {missing:?}");
    }

    // 3. Every Handy event is still emitted.
    let events = read("docs/handy-events.txt");
    let missing: Vec<&str> = events
        .lines()
        .filter(|e| !e.is_empty() && !src.contains(&format!("\"{e}\"")))
        .collect();
    if missing.is_empty() {
        println!("✓ all {} Handy events are emitted", events.lines().count());
    } else {
        ok = false;
        println!("✗ events never emitted: {missing:?}");
    }

    // 4. Every PARITY row is covered by an existing test or a manual check.
    let doc = read("docs/PARITY.md");
    let row = Regex::new(r"^\|\s*([A-Z]{1,2}-\d+)\s*\|(.*)\|\s*([^|]*)\|\s*$").unwrap();
    let manual_ids: BTreeSet<String> = Regex::new(r"(?m)^\|\s*(HW-\d+)\s*\|")
        .unwrap()
        .captures_iter(&doc)
        .map(|c| c[1].to_string())
        .collect();
    let (mut rows, mut covered) = (0, 0);
    for line in doc.lines() {
        let Some(c) = row.captures(line) else {
            continue;
        };
        let id = &c[1];
        if id.starts_with("HW") {
            continue;
        }
        rows += 1;
        let cover = c[3].trim();
        let mut row_ok = !cover.is_empty() && cover != "TODO";
        for item in cover.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(name) = item.strip_prefix("test:") {
                let name = name.trim();
                if !(tests.contains(name) || short.contains(name) || elsewhere_short.contains(name))
                {
                    println!("✗ {id}: unknown test `{name}`");
                    row_ok = false;
                }
            } else if let Some(m) = item.strip_prefix("manual:") {
                if !manual_ids.contains(m.trim()) {
                    println!("✗ {id}: unknown manual check `{}`", m.trim());
                    row_ok = false;
                }
            } else {
                println!("✗ {id}: cover must be `test: …` or `manual: HW-xx`, got `{item}`");
                row_ok = false;
            }
        }
        if row_ok {
            covered += 1;
        } else {
            ok = false;
            if cover == "TODO" || cover.is_empty() {
                println!("✗ {id}: not covered");
            }
        }
    }
    println!(
        "{} {covered}/{rows} PARITY rows covered",
        if covered == rows { "✓" } else { "✗" }
    );
    ok
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ok = match args.first().map(String::as_str) {
        Some("test") => test(args.iter().any(|a| a == "--full")),
        Some("parity") => parity(),
        Some("gate") => test(args.iter().any(|a| a == "--full")) && parity(),
        _ => {
            eprintln!("usage: cargo xtask <test [--full] | parity | gate [--full]>");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
