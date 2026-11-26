#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_NAME="md98-rust9x:1.100.0-v2"
PACKAGE="${1:-md98-win32}"
BIN_NAME="${2:-md98}"
TARGET="i586-rust9x-windows-msvc"

if command -v podman >/dev/null 2>&1; then
    CONTAINER_BIN="podman"
    VOL_OPT=":Z"
elif command -v docker >/dev/null 2>&1; then
    CONTAINER_BIN="docker"
    VOL_OPT=""
else
    echo "ERROR: Neither podman nor docker found in PATH." >&2
    exit 1
fi

if ! "$CONTAINER_BIN" image exists "$IMAGE_NAME" 2>/dev/null; then
    echo "==> Building container image $IMAGE_NAME..."
    "$CONTAINER_BIN" build -f "$ROOT_DIR/infra/Dockerfile.rust9x" -t "$IMAGE_NAME" "$ROOT_DIR"
fi

mkdir -p "$ROOT_DIR/dist" "$ROOT_DIR/.cargo-cache"

echo "==> Cross-compiling $PACKAGE ($BIN_NAME.exe) for $TARGET (Pentium Pro, no MMX/SSE/SSE2)..."
"$CONTAINER_BIN" run --rm \
    -v "$ROOT_DIR:/work${VOL_OPT}" \
    -v "$ROOT_DIR/.cargo-cache:/opt/cargo/registry${VOL_OPT}" \
    -w /work \
    "$IMAGE_NAME" \
    cargo +rust9x build \
        --release \
        -p "$PACKAGE" \
        --target "$TARGET" \
        -Z build-std=std,panic_abort \
        -Z build-std-features=compiler-builtins-mem,optimize_for_size

OUT_EXE="$ROOT_DIR/target/$TARGET/release/${BIN_NAME}.exe"
DIST_EXE="$ROOT_DIR/dist/${BIN_NAME}.exe"

if [[ ! -f "$OUT_EXE" ]]; then
    echo "ERROR: Expected binary not found at $OUT_EXE" >&2
    exit 1
fi

cp "$OUT_EXE" "$DIST_EXE"

echo "==> Running Win98 SE static PE audit on $DIST_EXE..."
python3 "$ROOT_DIR/infra/audit_pe.py" "$DIST_EXE"
echo "==> Build & PE audit PASSED: $DIST_EXE"
