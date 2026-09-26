#!/usr/bin/env bash
# Compiles and runs this workspace's real `x86_64-pc-windows-gnu` target on a
# Linux host that has no `rustup` and no network access to
# static.rust-lang.org (only a distro-packaged rustc/cargo and crates.io) —
# and then actually *executes* the resulting Win32 binaries under Wine,
# rather than only type-checking them.
#
# # Why this exists
#
# `rustnative-windows`'s `native` module is `#[cfg(windows)]`-gated, so a
# plain `cargo check --workspace` on Linux excludes it entirely — it has
# never, in this project's history, been compiled by anything running on a
# non-Windows host. `cargo check --target x86_64-pc-windows-gnu` doesn't
# help either: the distro's rustc ships a prebuilt standard library only for
# its own host target, not for `x86_64-pc-windows-gnu`, and the normal way
# to get one (`rustup target add x86_64-pc-windows-gnu`) needs network
# access to `static.rust-lang.org`, which is not always available.
#
# This script routes around both problems:
#
#   1. `RUSTC_BOOTSTRAP=1` makes a *stable* rustc/cargo accept nightly-only
#      flags, including `-Z build-std`, which compiles `core`/`alloc`/`std`
#      from source against whatever target you ask for instead of requiring
#      a prebuilt one.
#   2. The matching library source is fetched directly from `rust-lang/rust`
#      on GitHub at the exact tag matching the installed rustc's version,
#      confirmed against the installed rustc's own reported commit hash —
#      plus its `library/backtrace` submodule, fetched separately at the
#      pinned commit `rust-lang/rust`'s `.gitmodules` records for it, since
#      a plain sparse checkout does not pull submodules.
#   3. The two small "Rust runtime startup object" files a real Windows link
#      normally needs (`rsbegin.o`/`rsend.o` — see
#      `library/rtstartup/*.rs`) aren't produced by `-Z build-std` itself
#      (they're bootstrap-specific build steps), so this script compiles
#      them directly with `rustc --emit=obj` and drops them into the
#      sysroot's target `lib/` directory, where the linker's default search
#      path picks them up.
#   4. `x86_64-w64-mingw32-gcc`/`-ld` (from `gcc-mingw-w64-x86-64`) do the
#      actual linking, exactly as they would for any other Rust
#      `windows-gnu` cross-compilation setup.
#   5. The resulting PE32+ binaries are run under Wine. With an Xvfb virtual
#      display available (`DISPLAY` pointing at it), Wine's real X11 driver
#      loads and actual `CreateWindowExW` calls succeed — not just
#      `cargo test`'s non-`#[cfg(windows)]` subset, but real native window
#      creation, menu realization, and control layout, executed and,
#      running headed under Xvfb, screenshot-able.
#
# This is a genuinely new verification capability for this codebase, not a
# replacement for real Windows CI (see `.github/workflows/ci.yml`) — Wine is
# not Windows, and differences between the two are exactly the kind of thing
# this technique cannot catch. Treat a pass here as "compiles, links, and
# behaves plausibly under Wine", and a real Windows CI run as the actual
# release gate.
#
# # Usage
#   tools/windows-cross-test.sh              # build + cargo test under wine
#   tools/windows-cross-test.sh --run-example  # also launch hello-label
#                                               # under Xvfb and screenshot it
#
# # Requirements (all installed via apt in this project's dev environment)
#   - gcc-mingw-w64-x86-64 (provides x86_64-w64-mingw32-gcc/-ld)
#   - wine, wine64
#   - xvfb, imagemagick (only for --run-example's screenshot)
#   - git (for the sparse/partial clones below)
#   - network access to github.com/codeload.github.com (to fetch the
#     matching library source) and crates.io (for this workspace's own
#     dependencies) — NOT to static.rust-lang.org.

set -euo pipefail

TARGET=x86_64-pc-windows-gnu
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

SYSROOT="$(rustc --print sysroot)"
LIBRARY_SRC="$SYSROOT/lib/rustlib/src/rust/library"
TARGET_LIB_DIR="$SYSROOT/lib/rustlib/$TARGET/lib"

RUSTC_COMMIT="$(rustc --version --verbose | awk '/^commit-hash/ {print $2}')"
RUSTC_VERSION="$(rustc --version --verbose | awk '/^release/ {print $2}')"

echo "== Host rustc: $RUSTC_VERSION ($RUSTC_COMMIT) =="

if [[ ! -f "$LIBRARY_SRC/Cargo.lock" ]]; then
    echo "== Fetching library/ source for rust-lang/rust @ $RUSTC_VERSION =="
    git clone --filter=blob:none --no-checkout --depth 1 \
        --branch "$RUSTC_VERSION" https://github.com/rust-lang/rust.git \
        "$WORK_DIR/rust-src-repo"
    (
        cd "$WORK_DIR/rust-src-repo"
        git sparse-checkout set library
        git checkout "$RUSTC_VERSION"
        CHECKED_OUT_COMMIT="$(git rev-parse HEAD)"
        if [[ "$CHECKED_OUT_COMMIT" != "$RUSTC_COMMIT"* ]]; then
            echo "error: checked-out rust-lang/rust commit ($CHECKED_OUT_COMMIT)" >&2
            echo "       does not match installed rustc's commit ($RUSTC_COMMIT)." >&2
            echo "       Refusing to continue: the library source would not" >&2
            echo "       actually match the compiler being used to build it." >&2
            exit 1
        fi
    )
    mkdir -p "$SYSROOT/lib/rustlib/src/rust"
    cp -r "$WORK_DIR/rust-src-repo/library" "$LIBRARY_SRC"

    echo "== Fetching library/backtrace submodule at its pinned commit =="
    # NOTE: the trailing slash on `library/` is load-bearing. Without it,
    # `git ls-tree <tree> library` matches the single tree entry named
    # `library` itself (printing one line describing that directory, not
    # its contents), so the `awk` below would silently find no match and
    # `BACKTRACE_COMMIT` would be empty — which is exactly what happened
    # the first time this script was run in a fresh environment `git
    # checkout`'s tag had never been fetched into before. The empty-string
    # pathspec error `git clone --branch "$BACKTRACE_COMMIT" ...` then
    # raises downstream is a *symptom* of this, not the root cause.
    BACKTRACE_COMMIT="$(git -C "$WORK_DIR/rust-src-repo" ls-tree HEAD library/ | \
        awk '/backtrace/ {print $3}')"
    if [[ -z "$BACKTRACE_COMMIT" ]]; then
        echo "error: could not resolve library/backtrace's pinned submodule commit" >&2
        exit 1
    fi
    git clone --filter=blob:none --quiet \
        https://github.com/rust-lang/backtrace-rs.git "$WORK_DIR/backtrace-rs"
    git -C "$WORK_DIR/backtrace-rs" checkout --quiet "$BACKTRACE_COMMIT"
    rm -rf "$LIBRARY_SRC/backtrace"
    cp -r "$WORK_DIR/backtrace-rs" "$LIBRARY_SRC/backtrace"
    rm -rf "$LIBRARY_SRC/backtrace/.git"
else
    echo "== library/ source already present at $LIBRARY_SRC, skipping fetch =="
fi

if [[ ! -f "$TARGET_LIB_DIR/rsbegin.o" || ! -f "$TARGET_LIB_DIR/rsend.o" ]]; then
    echo "== Building rsbegin.o/rsend.o (Rust runtime startup objects) =="
    mkdir -p "$TARGET_LIB_DIR"
    for name in rsbegin rsend; do
        RUSTC_BOOTSTRAP=1 rustc --edition 2021 -O --target "$TARGET" \
            --crate-type rlib --emit=obj \
            -o "$TARGET_LIB_DIR/$name.o" \
            "$LIBRARY_SRC/rtstartup/$name.rs"
    done
else
    echo "== rsbegin.o/rsend.o already present, skipping =="
fi

echo "== Building workspace for $TARGET (this recompiles core/alloc/std" \
     "from source; expect it to take a minute or two the first time) =="
cd "$REPO_ROOT"
RUSTC_BOOTSTRAP=1 cargo build \
    -Z build-std=core,alloc,std,panic_abort,test \
    --target "$TARGET" --workspace --all-targets

echo "== Running rustnative-core and rustnative-windows test binaries under Wine =="
for crate in rustnative_core rustnative_windows; do
    exe="$(find "target/$TARGET/debug/deps" -maxdepth 1 -name "${crate}-*.exe" \
        ! -name "*.d" | sort | tail -1)"
    if [[ -z "$exe" ]]; then
        echo "warning: no test binary found for $crate, skipping" >&2
        continue
    fi
    echo "--- $exe ---"
    wine "$exe" --test-threads=1
done

if [[ "${1:-}" == "--run-example" ]]; then
    echo "== Launching hello-label under Xvfb for a real windowed smoke test =="
    if ! command -v Xvfb >/dev/null; then
        echo "error: Xvfb not installed (apt-get install -y xvfb)" >&2
        exit 1
    fi
    XVFB_DISPLAY=:99
    rm -f "/tmp/.X${XVFB_DISPLAY#:}-lock"
    Xvfb "$XVFB_DISPLAY" -screen 0 1280x1024x24 &
    xvfb_pid=$!
    # `Xvfb` never exits on its own, so cleanup here kills it and the app
    # explicitly rather than `wait`ing on every background job (which would
    # hang forever on `xvfb_pid`).
    trap 'kill "$xvfb_pid" 2>/dev/null || true; rm -rf "$WORK_DIR"' EXIT
    sleep 2
    DISPLAY="$XVFB_DISPLAY" timeout 8 wine \
        "target/$TARGET/debug/hello-label.exe" &
    app_pid=$!
    sleep 5
    if command -v import >/dev/null; then
        DISPLAY="$XVFB_DISPLAY" import -window root /tmp/hello-label-wine.png
        echo "Screenshot written to /tmp/hello-label-wine.png"
    fi
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
fi

echo "== Done =="
