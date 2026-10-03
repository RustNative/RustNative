#!/usr/bin/env bash
# Runs a command the way the Linux backend's tests need it run
# (`docs/superpowers/plans/2026-09-29-linux-milestone-34.md`, "Verification
# gate"):
#
#   tools/linux-session.sh <wayland|x11|xvfb> <command> [args...]
#
# - a private D-Bus session bus, so the AT-SPI2 accessibility bus, fake
#   portals, and fake notification servers a test registers never reach the
#   person's own session;
# - the chosen display server: the session's own (WSLg provides both a
#   Wayland compositor and Xwayland) when it is reachable, otherwise a
#   headless Weston or an Xvfb started for this run and stopped after it;
# - `GDK_BACKEND` pinned, so GTK cannot quietly pick the other one.
set -euo pipefail

server="${1:-}"
shift || true
if [[ "$server" != "wayland" && "$server" != "x11" && "$server" != "xvfb" ]] || [[ $# -eq 0 ]]; then
    echo "usage: $0 <wayland|x11|xvfb> <command> [args...]" >&2
    exit 2
fi

cleanup_pids=()
cleanup() {
    for pid in "${cleanup_pids[@]:-}"; do
        [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
    done
}
trap cleanup EXIT

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [[ ! -d "$XDG_RUNTIME_DIR" ]]; then
    XDG_RUNTIME_DIR="$(mktemp -d)"
    export XDG_RUNTIME_DIR
fi

wayland_reachable() {
    local socket="${WAYLAND_DISPLAY:-}"
    [[ -n "$socket" ]] || return 1
    [[ "$socket" = /* ]] || socket="$XDG_RUNTIME_DIR/$socket"
    [[ -S "$socket" ]]
}

x11_reachable() {
    [[ -n "${DISPLAY:-}" ]] && xdotool getdisplaygeometry >/dev/null 2>&1
}

case "$server" in
wayland)
    if ! wayland_reachable; then
        # WSLg keeps its socket under /mnt/wslg/runtime-dir.
        if [[ -S /mnt/wslg/runtime-dir/wayland-0 ]]; then
            ln -sf /mnt/wslg/runtime-dir/wayland-0 "$XDG_RUNTIME_DIR/wayland-0" 2>/dev/null || true
            export WAYLAND_DISPLAY=wayland-0
        fi
    fi
    # A cold WSL start creates WSLg's socket before its compositor accepts
    # connections: wait for it to answer.
    if wayland_reachable && command -v nc >/dev/null; then
        target="$WAYLAND_DISPLAY"
        [[ "$target" = /* ]] || target="$XDG_RUNTIME_DIR/$target"
        for _ in $(seq 1 100); do
            nc -zU "$target" 2>/dev/null && break
            sleep 0.1
        done
    fi
    if ! wayland_reachable; then
        socket="rustnative-test-$$"
        weston --backend=headless --socket="$socket" --idle-time=0 >/dev/null 2>&1 &
        cleanup_pids+=("$!")
        for _ in $(seq 1 100); do
            [[ -S "$XDG_RUNTIME_DIR/$socket" ]] && break
            sleep 0.05
        done
        export WAYLAND_DISPLAY="$socket"
    fi
    export GDK_BACKEND=wayland
    ;;
x11 | xvfb)
    # `xvfb` always starts a private X server: the one place a test can
    # inject real input (XTest) and have it land, because no compositor
    # decides focus there. `x11` prefers the session's own server.
    if [[ "$server" == "xvfb" ]] || ! x11_reachable; then
        display=":$((90 + RANDOM % 9))"
        Xvfb "$display" -screen 0 1600x1000x24 -nolisten tcp >/dev/null 2>&1 &
        cleanup_pids+=("$!")
        export DISPLAY="$display"
        for _ in $(seq 1 100); do
            x11_reachable && break
            sleep 0.05
        done
    fi
    export GDK_BACKEND=x11
    if [[ "$server" == "xvfb" ]]; then
        # Tests that inject input through XTest run only here.
        export RUSTNATIVE_PRIVATE_X=1
    fi
    ;;
esac

# GTK's own accessibility goes to the AT-SPI2 bus, which the private session
# activates on demand (`org.a11y.Bus`).
export GTK_A11Y=atspi
export NO_AT_BRIDGE=0
exec dbus-run-session -- "$@"
