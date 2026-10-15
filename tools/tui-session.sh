#!/usr/bin/env bash
# Runs the terminal backend in real Unix terminals (`docs/tui.md`,
# "Verified on"; docs/superpowers/plans/2026-10-13-terminal-milestone-38.md).
#
#   tools/tui-session.sh tests   the real-terminal tests, under `script` (a
#                                pty of a known size), so every test runs
#                                with a controlling terminal
#   tools/tui-session.sh tmux    examples/tui-demo inside tmux: typed with
#                                `send-keys`, read with `capture-pane`, and
#                                the shell's screen checked after it exits
#   tools/tui-session.sh ssh     the real-terminal tests with the child run
#                                over SSH to this machine (needs `sshd`; the
#                                script starts a private one on port 2222)
#
# Exit status 0 when every check passed.
set -u
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/rustnative-target}"
failures=0
check() { # check <description> <condition...>
    local what=$1; shift
    if "$@"; then echo "ok   $what"; else echo "FAIL $what"; failures=$((failures + 1)); fi
}

case "${1:-tests}" in
tests)
    # 100 × 30, set on the pty `script` makes.
    script -qec "stty cols 100 rows 30; cargo test -p rustnative-tui --test real_terminal -- --include-ignored --skip over_ssh" /dev/null
    exit $?
    ;;
tmux)
    cargo build -q -p tui-demo || exit 1
    demo="$CARGO_TARGET_DIR/debug/tui-demo"
    socket=rustnative-tui-check
    tmux -L "$socket" kill-server 2>/dev/null
    tmux -L "$socket" -f /dev/null new-session -d -x 100 -y 30 -s demo "TERM=xterm-256color sh"
    pane() { tmux -L "$socket" capture-pane -p -t demo; }
    keys() { tmux -L "$socket" send-keys -t demo "$@"; sleep 0.4; }
    keys "printf 'shell before\\n'" Enter
    keys "$demo" Enter
    sleep 1.5
    check "the demo draws in tmux" grep -q "1 of 3 done" <(pane)
    check "on tmux's alternate screen" test "$(tmux -L "$socket" display -p -t demo '#{alternate_on}')" = 1
    for _ in 1 2 3 4; do keys Tab; done
    keys "Buy milk"
    keys Enter
    check "typed and added" grep -q "1 of 4 done" <(pane)
    check "the new task is listed" grep -q "Buy milk" <(pane)
    keys BTab
    keys BTab
    keys Up
    check "Up reached the list" grep -q "Done: Resize the terminal" <(pane)
    keys Tab
    keys Space
    check "Space checked it" grep -q "2 of 4 done" <(pane)
    tmux -L "$socket" resize-window -t demo -x 120 -y 34 2>/dev/null
    sleep 0.6
    check "a resize is followed" grep -q "2 of 4 done" <(pane)
    keys C-c
    sleep 0.6
    check "Ctrl+C gave the terminal back" test "$(tmux -L "$socket" display -p -t demo '#{alternate_on}')" = 0
    check "the shell's screen is back" grep -q "shell before" <(pane)
    check "the cursor is shown" test "$(tmux -L "$socket" display -p -t demo '#{cursor_flag}')" = 1
    check "mouse reporting is off" test "$(tmux -L "$socket" display -p -t demo '#{mouse_any_flag}#{mouse_button_flag}#{mouse_standard_flag}')" = 000
    keys "stty -a > /tmp/rustnative-tui-stty.txt" Enter
    check "the terminal is in cooked mode" sh -c \
        "grep -qw icanon /tmp/rustnative-tui-stty.txt && ! grep -q -- -icanon /tmp/rustnative-tui-stty.txt"
    tmux -L "$socket" kill-server
    ;;
ssh)
    command -v sshd >/dev/null || [ -x /usr/sbin/sshd ] || { echo "no sshd"; exit 2; }
    state="$CARGO_TARGET_DIR/tui-ssh"
    mkdir -p "$state"
    [ -f "$state/host_ed25519" ] || ssh-keygen -q -t ed25519 -N "" -f "$state/host_ed25519"
    [ -f "$state/client_ed25519" ] || ssh-keygen -q -t ed25519 -N "" -f "$state/client_ed25519"
    cp "$state/client_ed25519.pub" "$state/authorized_keys"
    chmod 600 "$state/authorized_keys"
    cat > "$state/sshd_config" <<CONFIG
Port 2222
ListenAddress 127.0.0.1
HostKey $state/host_ed25519
AuthorizedKeysFile $state/authorized_keys
PasswordAuthentication no
PermitRootLogin prohibit-password
StrictModes no
UsePAM no
PidFile $state/sshd.pid
CONFIG
    # sshd's privilege-separation directory, which a distribution's
    # service would make.
    [ "$(id -u)" = 0 ] && mkdir -p /run/sshd
    [ -f "$state/sshd.pid" ] && kill "$(cat "$state/sshd.pid")" 2>/dev/null
    "$(command -v sshd || echo /usr/sbin/sshd)" -f "$state/sshd_config" -E "$state/sshd.log" || exit 1
    sleep 0.5
    export RUSTNATIVE_TUI_SSH="$(id -un)@127.0.0.1:2222:$state/client_ed25519"
    cargo test -p rustnative-tui --test real_terminal -- --include-ignored over_ssh
    status=$?
    kill "$(cat "$state/sshd.pid")" 2>/dev/null
    exit $status
    ;;
*)
    echo "usage: $0 [tests|tmux|ssh]"; exit 2 ;;
esac
[ "$failures" = 0 ]
