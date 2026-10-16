//! Embeds every `assets/locales/<code>/translation.json`.
use std::fmt::Write;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut codes: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("translation.json").is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    codes.sort();
    let mut out = String::from("pub static LOCALE_FILES: &[(&str, &str)] = &[\n");
    for code in codes {
        let path = dir.join(&code).join("translation.json");
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(
            out,
            "    ({:?}, include_str!({:?})),",
            code,
            path.canonicalize().unwrap()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("locales.rs");
    std::fs::write(dest, out).unwrap();
}
