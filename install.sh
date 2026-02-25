#!/usr/bin/env bash
set -euo pipefail

OWNER="ChrisJohnson89"
REPO="Ferromon"
BIN_NAME="ferro"

say() { printf "%s\n" "$*"; }
die() { say "ERROR: $*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"
}

need curl
need tar

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "${OS}:${ARCH}" in
  linux:x86_64)
    TARGET="x86_64-unknown-linux-musl"
    ;;
  darwin:arm64|darwin:aarch64)
    TARGET="aarch64-apple-darwin"
    ;;
  darwin:x86_64)
    TARGET="x86_64-apple-darwin"
    ;;
  *)
    die "unsupported platform: OS=${OS} ARCH=${ARCH}"
    ;;
esac

API_LIST="https://api.github.com/repos/${OWNER}/${REPO}/releases"

say "→ Fetching latest release with assets for ${TARGET}…"
RELEASES_JSON="$(curl -fsSL -H 'User-Agent: ferromon-installer' "$API_LIST")" || die "failed to query GitHub releases"

need python3

VER="$(printf "%s" "$RELEASES_JSON" | python3 - "$TARGET" <<'PY'
import json, sys
rels=json.loads(sys.stdin.read())
target = sys.argv[1] if len(sys.argv) > 1 else ""
for r in rels:
    tag=r.get('tag_name')
    assets=r.get('assets') or []
    names={a.get('name') for a in assets}
    want=f"ferromon-{tag}-{target}.tar.gz"
    if want in names:
        print(tag)
        raise SystemExit(0)
print("")
PY
)"

[ -n "${VER:-}" ] || die "no suitable release found for target ${TARGET}"

ASSET="ferromon-${VER}-${TARGET}.tar.gz"
SHA="${ASSET}.sha256"
URL="https://github.com/${OWNER}/${REPO}/releases/download/${VER}/${ASSET}"
SHA_URL="https://github.com/${OWNER}/${REPO}/releases/download/${VER}/${SHA}"

TMPDIR="$(mktemp -d)"
cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

say "→ Downloading ${ASSET}…"
curl -fL "$URL" -o "$TMPDIR/$ASSET" || die "download failed (asset may not exist yet): $URL"

say "→ Downloading checksum…"
curl -fL "$SHA_URL" -o "$TMPDIR/$SHA" || die "checksum download failed: $SHA_URL"

EXPECTED="$(awk '{print $1}' "$TMPDIR/$SHA" | head -n 1)"
[ -n "${EXPECTED:-}" ] || die "could not read checksum"

say "→ Verifying checksum…"
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$TMPDIR/$ASSET" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL="$(shasum -a 256 "$TMPDIR/$ASSET" | awk '{print $1}')"
else
  die "need sha256sum or shasum for checksum verification"
fi

[ "$ACTUAL" = "$EXPECTED" ] || die "checksum mismatch (expected $EXPECTED got $ACTUAL)"

say "→ Extracting…"
tar -xzf "$TMPDIR/$ASSET" -C "$TMPDIR" || die "failed to extract"
[ -f "$TMPDIR/$BIN_NAME" ] || die "archive did not contain '$BIN_NAME'"
chmod +x "$TMPDIR/$BIN_NAME"

INSTALL_DIR="/usr/local/bin"
DEST="$INSTALL_DIR/$BIN_NAME"

install_to_user() {
  USER_DIR="$HOME/.local/bin"
  mkdir -p "$USER_DIR"
  cp "$TMPDIR/$BIN_NAME" "$USER_DIR/$BIN_NAME"
  say "✓ Installed to $USER_DIR/$BIN_NAME"
  say "  If you get 'command not found', add this to your shell profile:"
  say "    export PATH=\"$HOME/.local/bin:\$PATH\""
}

if [ -w "$INSTALL_DIR" ]; then
  cp "$TMPDIR/$BIN_NAME" "$DEST"
  say "✓ Installed to $DEST"
elif command -v sudo >/dev/null 2>&1; then
  sudo cp "$TMPDIR/$BIN_NAME" "$DEST"
  say "✓ Installed to $DEST (via sudo)"
else
  install_to_user
fi

say "→ Done: $BIN_NAME $VER ($TARGET)"
INSTALLED_VER="$($BIN_NAME --version 2>/dev/null || true)"
if [ -n "$INSTALLED_VER" ]; then
  say "→ Installed version: $INSTALLED_VER"
fi
