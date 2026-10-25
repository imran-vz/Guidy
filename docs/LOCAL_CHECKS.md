# Local verification

Automated CI is disabled for this project. There are no self-hosted runner
requirements. Run the applicable checks on the device where development happens,
before opening or merging a pull request. Keep the existing checks and tests;
removing CI does not waive verification.

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask gate
# Optional unsigned macOS packaging verification:
cargo install cargo-packager --locked
cargo packager --release -p guidy-app --formats app,dmg
```

Install stable Rust and CMake. Linux development also needs ALSA, GTK 3, xkbcommon, Wayland/X11, fontconfig and Vulkan development packages. Packaging for macOS requires macOS. `cargo xtask gate` preserves the unit/UI/CLI test and parity checks; run on the device used for development.

Record the commands run and their results in the pull request. State any skipped
checks, unavailable fixtures, or untested platforms explicitly.
