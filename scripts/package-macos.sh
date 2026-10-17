#!/usr/bin/env bash
# Build Guidy.app and sign it with a stable local identity, so macOS privacy
# grants (Microphone, Accessibility) survive rebuilds.
#
# Ad-hoc signatures pin TCC grants to the binary's hash, so every rebuild
# silently invalidates them. If a code-signing identity named
# $GUIDY_SIGN_IDENTITY (default "Guidy Local Dev") is in the keychain, the app
# is re-signed with it; otherwise the ad-hoc signature is kept.
#
# Usage: scripts/package-macos.sh [--install]
#   --install  replace /Applications/Guidy.app with the new build
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IDENTITY="${GUIDY_SIGN_IDENTITY:-Guidy Local Dev}"
APP="$ROOT/target/packages/Guidy.app"

cd "$ROOT"
cargo packager --release -p guidy-app --formats app

# The built-in llama.cpp helper must sit next to the main executable.
HELPER="$APP/Contents/MacOS/guidy-llm"
HELPER_COPIED=0
if [[ ! -x "$HELPER" ]]; then
  # cargo-packager didn't bundle it (non-main binary): copy it in.
  echo "Copying guidy-llm into the bundle"
  cp "$ROOT/target/release/guidy-llm" "$HELPER"
  HELPER_COPIED=1
fi

if security find-identity -p codesigning | grep -q "\"$IDENTITY\""; then
  echo "Signing with \"$IDENTITY\""
  # Nested executables are signed first (inside-out); the helper needs no
  # entitlements of its own.
  codesign --force --options runtime -s "$IDENTITY" "$HELPER"
  codesign --force --deep --options runtime \
    --entitlements "$ROOT/assets/Entitlements.plist" \
    -s "$IDENTITY" "$APP"
  codesign --verify --strict "$APP"
else
  if [[ "$HELPER_COPIED" == 1 ]]; then
    # The copied helper isn't covered by the packager's ad-hoc signature:
    # sign it and reseal the bundle.
    codesign --force -s - "$HELPER"
    codesign --force --deep -s - \
      --entitlements "$ROOT/assets/Entitlements.plist" "$APP"
  fi
  echo "No \"$IDENTITY\" identity found; keeping the ad-hoc signature." >&2
  echo "Privacy grants will reset on every rebuild." >&2
fi

if [[ "${1:-}" == "--install" ]]; then
  osascript -e 'quit app "Guidy"' 2>/dev/null || true
  rm -rf /Applications/Guidy.app
  cp -R "$APP" /Applications/
  echo "Installed /Applications/Guidy.app"
fi
