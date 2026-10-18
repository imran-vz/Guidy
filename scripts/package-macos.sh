#!/usr/bin/env bash
# Build Guidy.app and sign it with a stable local identity, so macOS privacy
# grants (Microphone, Accessibility) survive rebuilds.
#
# Ad-hoc signatures pin TCC grants to the binary's hash, so every rebuild
# silently invalidates them. If a code-signing identity named
# $GUIDY_SIGN_IDENTITY (default "Guidy Local Dev") is in the keychain, the app
# is re-signed with it; otherwise it is re-signed ad-hoc.
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
if [[ ! -x "$HELPER" ]]; then
  # cargo-packager didn't bundle it (non-main binary): copy it in.
  echo "Copying guidy-llm into the bundle"
  cp "$ROOT/target/release/guidy-llm" "$HELPER"
fi

if security find-identity -p codesigning | grep -q "\"$IDENTITY\""; then
  echo "Signing with \"$IDENTITY\""
  SIGN_ID="$IDENTITY"
else
  echo "No \"$IDENTITY\" identity found; re-signing ad-hoc." >&2
  echo "Privacy grants will reset on every rebuild." >&2
  SIGN_ID="-"
fi

# cargo-packager signs every binary with the app's entitlements, the helper
# included, and `codesign --deep` would do the same. So nested code is signed
# first, one by one (inside-out) and without entitlements, then the bundle on
# its own (no --deep), which seals the nested signatures as they are.
NESTED=("$HELPER")
while IFS= read -r -d '' f; do
  NESTED+=("$f")
done < <(find "$APP/Contents" -type d -name '*.framework' -prune -print0 \
  -o -type f -name '*.dylib' -print0)
for f in "${NESTED[@]}"; do
  codesign --force --options runtime -s "$SIGN_ID" "$f"
done
codesign --force --options runtime \
  --entitlements "$ROOT/assets/Entitlements.plist" \
  -s "$SIGN_ID" "$APP"
codesign --verify --strict --deep "$APP"

if [[ "${1:-}" == "--install" ]]; then
  osascript -e 'quit app "Guidy"' 2>/dev/null || true
  rm -rf /Applications/Guidy.app
  cp -R "$APP" /Applications/
  echo "Installed /Applications/Guidy.app"
fi
