# Guidy

Offline speech-to-text for the desktop. Press a shortcut, speak, and Guidy pastes the transcript into the focused app. Transcription runs locally.

Guidy is a GPUI port of [Handy](https://github.com/cjpais/Handy) (MIT). It is pure Rust, with [gpui-kit](https://gpui-kit.com) components and a Zed-style look (One Dark/Light, IBM Plex Sans, Lilex). See [`docs/MIGRATION_PLAN.md`](docs/MIGRATION_PLAN.md) for the design and [`docs/PARITY.md`](docs/PARITY.md) for the feature-by-feature parity record.

## Layout

| Path | What |
|---|---|
| `crates/guidy-core` | Backend: audio, VAD, transcription engines, models, settings, history, post-processing, shortcuts, tray, CLI |
| `crates/guidy-i18n` | Translations (26 locales) |
| `crates/guidy-app` | The `guidy` binary: GPUI windows, pages, onboarding, overlay |
| `xtask` | Test and parity gate |
| `assets` | Theme, fonts, icons, sounds, tray glyphs, locales |

## Build and run

Requires stable Rust and CMake. Linux also needs ALSA, GTK 3, xkbcommon, Wayland/X11, fontconfig and Vulkan dev packages (see `.github/workflows/ci.yml`).

```sh
cargo run -p guidy-app            # the app
cargo run -p guidy-app -- --help  # CLI flags (--start-hidden, --toggle-transcription, ...)
```

Debug builds also read these variables:

| Variable | Effect |
|---|---|
| `GUIDY_DEV_SKIP_PERMISSIONS=1` | Skip the macOS permission onboarding |
| `GUIDY_DEV_START_SECTION=<general\|models\|advanced\|history\|postprocessing\|debug\|about>` | Open on a page |
| `GUIDY_DEV_OVERLAY_DEMO=1` | Cycle the recording overlay through its states |
| `GUIDY_RESOURCE_DIR=<dir>` | Override where `resources/` is looked up |

## Tests

```sh
cargo xtask test          # unit, UI (headless GPUI) and CLI tests
cargo xtask test --full   # plus the real-model engine tests (needs a downloaded model)
cargo xtask parity        # every Handy test, command, event and PARITY row is covered
cargo xtask gate          # test + parity; this is what CI runs
```

## Packaging

```sh
cargo install cargo-packager --locked
cargo packager --release -p guidy-app --formats app,dmg   # macOS
cargo packager --release -p guidy-app --formats nsis      # Windows
cargo packager --release -p guidy-app --formats deb,appimage
```

Output goes to `target/packages/`. The packages are unsigned. On macOS the app gets an ad-hoc signature, so Gatekeeper asks for confirmation on first launch.

For local macOS development, use `scripts/package-macos.sh [--install]` instead. Ad-hoc signatures change on every rebuild, which silently invalidates the Microphone and Accessibility grants. The script re-signs the app with a stable self-signed identity (`Guidy Local Dev` by default; override with `GUIDY_SIGN_IDENTITY`) when one is in your keychain, so grants persist across rebuilds. To create the identity once:

```sh
cat > /tmp/guidy-cert.cnf <<'EOF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Guidy Local Dev
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
openssl req -x509 -newkey rsa:2048 -nodes -keyout /tmp/guidy.key -out /tmp/guidy.pem -days 3650 -config /tmp/guidy-cert.cnf
openssl pkcs12 -export -inkey /tmp/guidy.key -in /tmp/guidy.pem -name "Guidy Local Dev" -out /tmp/guidy.p12 \
  -passout pass:guidy -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
security import /tmp/guidy.p12 -k ~/Library/Keychains/login.keychain-db -P guidy -T /usr/bin/codesign
rm /tmp/guidy.key /tmp/guidy.pem /tmp/guidy.p12 /tmp/guidy-cert.cnf
```

These variables are read at build time:

| Variable | Effect |
|---|---|
| `GUIDY_UPDATE_ENDPOINT` | URL of the updater's `latest.json`. Empty disables updates. |
| `GUIDY_UPDATE_PUBKEY` | Minisign public key for update signatures (`cargo packager signer generate`) |
| `GUIDY_RELEASES_URL` | Link shown for portable installs and release notes |

## License

MIT, see [`LICENSE`](LICENSE). Third-party code and assets are listed in [`NOTICE.md`](NOTICE.md).
