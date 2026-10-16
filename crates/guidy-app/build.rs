fn main() {
    // Release/update endpoints are deployment-specific: set them when building
    // a release (empty disables the related UI).
    for (var, default) in [
        ("GUIDY_RELEASES_URL", ""),
        ("GUIDY_UPDATE_ENDPOINT", ""),
        ("GUIDY_UPDATE_PUBKEY", ""),
    ] {
        println!("cargo:rerun-if-env-changed={var}");
        let value = std::env::var(var).unwrap_or_else(|_| default.to_string());
        println!("cargo:rustc-env={var}={value}");
    }
    // Mirrors guidy-core's link arguments, which (unlike link-lib directives)
    // do not propagate from a library to the final binary.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // CPAL links CoreAudio functions that only exist on macOS 14.2+.
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/guidy:$ORIGIN/../lib");
    }
}
