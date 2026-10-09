#!/usr/bin/env bash
# The Android lifecycle checks that need a real process (`docs/android/lifecycle.md`),
# on `examples/adoption-android` — a host application embedding a
# `RustNativeView` whose count is persisted state:
#
#   1. embedding: the host's own screen, with the Rust Native view inside it;
#   2. process death: pressed three times, sent to the background, killed
#      (`am kill`), relaunched cold — the count is back;
#   3. a configuration change (rotation): the same process, relaid out,
#      the count kept;
#   4. a memory trim (`am send-trim-memory`): no crash.
#
# The device's rotation settings are restored afterwards. On HyperOS, the
# first install of the example asks for a confirmation on the device.
#
# Usage: tools/android-lifecycle-test.sh
set -uo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=tools/android-env.sh
source tools/android-env.sh
export MSYS_NO_PATHCONV=1

serial="${ANDROID_SERIAL:-$(adb devices | awk 'NR > 1 && $2 == "device" { print $1; exit }')}"
[ -n "$serial" ] || { echo "android-lifecycle-test: no device attached" >&2; exit 1; }
package="dev.rustnative.adoption_android"
host="$package/dev.rustnative.adoption.HostActivity"
target_dir="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | sed 's/\\\\/\//g')"
apk="$target_dir/rustnative/android/adoption-android/app/build/outputs/apk/debug/app-debug.apk"
abi="$(adb -s "$serial" shell getprop ro.product.cpu.abi | tr -d '\r')"

(cd examples/adoption-android && cargo run -q --manifest-path ../../Cargo.toml -p rustnative-cli -- build android --abi "$abi") \
    || { echo "android-lifecycle-test: the build failed" >&2; exit 1; }
adb -s "$serial" install -r -t "$apk" > /dev/null || { echo "android-lifecycle-test: the install failed (confirm it on the device)" >&2; exit 1; }
adb -s "$serial" shell pm clear "$package" > /dev/null

failed=0
check() { if [ "$2" = "$3" ]; then echo "ok      $1"; else echo "FAILED  $1: expected '$3', got '$2'"; failed=1; fi; }
texts() {
    adb -s "$serial" shell uiautomator dump /sdcard/rn-ui.xml > /dev/null
    adb -s "$serial" shell cat /sdcard/rn-ui.xml | tr '>' '\n' | sed -n 's/.*text="\([^"]*\)".*/\1/p' | grep -v '^$'
}
count() { texts | grep '^Pressed ' | head -1; }
press() {
    local bounds
    bounds="$(adb -s "$serial" shell cat /sdcard/rn-ui.xml | tr '>' '\n' | grep 'text="Press"' | sed -n 's/.*bounds="\[\([0-9]*\),\([0-9]*\)\]\[\([0-9]*\),\([0-9]*\)\]".*/\1 \2 \3 \4/p')"
    set -- $bounds
    adb -s "$serial" shell input tap $(( ($1 + $3) / 2 )) $(( ($2 + $4) / 2 ))
}

adb -s "$serial" shell am start -W -n "$host" > /dev/null
sleep 2
screen="$(texts)"
check "embedding: the host's own screen" "$(echo "$screen" | grep -c "host application's own screen")" "1"
check "embedding: the Rust Native view inside it" "$(echo "$screen" | grep -c 'Rendered by Rust Native')" "1"

for _ in 1 2 3; do texts > /dev/null; press; sleep 0.7; done
check "input reaches the embedded view" "$(count)" "Pressed 3 times"

first="$(adb -s "$serial" shell pidof "$package" | tr -d '\r')"
adb -s "$serial" shell input keyevent KEYCODE_HOME
sleep 2
adb -s "$serial" shell am kill "$package"
sleep 1
check "process death: the process is gone" "$(adb -s "$serial" shell pidof "$package" | tr -d '\r')" ""
adb -s "$serial" shell am start -W -n "$host" > /dev/null
sleep 2
second="$(adb -s "$serial" shell pidof "$package" | tr -d '\r')"
check "process death: a new process" "$([ -n "$second" ] && [ "$second" != "$first" ] && echo new)" "new"
check "process death: the count is restored" "$(count)" "Pressed 3 times"

accelerometer="$(adb -s "$serial" shell settings get system accelerometer_rotation | tr -d '\r')"
rotation="$(adb -s "$serial" shell settings get system user_rotation | tr -d '\r')"
adb -s "$serial" shell settings put system accelerometer_rotation 0
adb -s "$serial" shell settings put system user_rotation 1
sleep 3
check "rotation: the same process" "$(adb -s "$serial" shell pidof "$package" | tr -d '\r')" "$second"
check "rotation: the count kept" "$(count)" "Pressed 3 times"
adb -s "$serial" shell settings put system user_rotation "${rotation:-0}"
adb -s "$serial" shell settings put system accelerometer_rotation "${accelerometer:-1}"
sleep 2

adb -s "$serial" shell am send-trim-memory "$package" RUNNING_LOW > /dev/null
sleep 1
check "memory trim: still running" "$(adb -s "$serial" shell pidof "$package" | tr -d '\r')" "$second"

adb -s "$serial" shell am force-stop "$package"
exit "$failed"
