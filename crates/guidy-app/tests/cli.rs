//! Headless CLI behavior (PARITY S-02).

use std::path::PathBuf;
use std::process::Command;

fn guidy() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_guidy"));
    // Isolate app data so the user's settings never leak into the result.
    let home = std::env::temp_dir().join(format!("guidy-cli-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    cmd.env("HOME", &home);
    if std::env::var_os("HF_HOME").is_none() {
        if let Some(real_home) = dirs_home() {
            cmd.env("HF_HOME", real_home.join(".cache/huggingface"));
        }
    }
    cmd
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn help_lists_the_cli_flags() {
    let out = guidy().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--start-hidden",
        "--no-tray",
        "--toggle-transcription",
        "--toggle-post-process",
        "--cancel",
        "--debug",
        "--transcribe-file",
        "--model",
        "--device-index",
        "--list-devices",
        "--list-models",
        "--repeat",
        "--json",
    ] {
        assert!(text.contains(flag), "missing {flag} in --help");
    }
    assert!(text.contains("Guidy"));
    assert!(!text.contains("Handy"));
}

#[test]
fn list_models_json_is_the_model_registry() {
    let out = guidy().args(["--list-models", "--json"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let models: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let models = models.as_array().unwrap();
    assert!(models.len() > 10);
    assert!(models
        .iter()
        .all(|m| m["id"].is_string() && m["name"].is_string()));
}

#[test]
fn missing_wav_is_a_usage_error() {
    let out = guidy()
        .args(["--transcribe-file", "/definitely/not/here.wav", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// Same fixture, same model as Handy → same transcript (golden file captured
/// from Handy 0.9.8's CLI).
#[test]
// IGNORE-REASON: needs the Nemotron streaming model in the HF cache; run by `cargo xtask test --full`.
#[ignore]
fn transcription_matches_handy_golden_output() {
    let golden: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(workspace().join("fixtures/golden/handy-cli-speech.json"))
            .unwrap(),
    )
    .unwrap();
    let wav = workspace().join("fixtures/audio/speech-16k-mono.wav");
    let out = guidy()
        .args(["--transcribe-file"])
        .arg(&wav)
        .args(["--json", "--model", golden["model"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for key in ["text", "model", "audio_secs", "requested_device"] {
        assert_eq!(got[key], golden[key], "{key} differs from Handy");
    }
}
