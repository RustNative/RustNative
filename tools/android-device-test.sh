#!/usr/bin/env bash
# Runs the Android backend's device suite on every attached device or
# emulator (`docs/android.md`, "Testing"): builds `examples/android-device-tests`
# for the device's ABI with the instrumentation, installs it, keeps the screen
# on while it runs, and prints one line per test and a summary. Exits
# non-zero when any test fails or none ran.
#
# Usage: tools/android-device-test.sh [filter]   (a substring of test names)
set -uo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=tools/android-env.sh
source tools/android-env.sh
# Git Bash would rewrite the device's paths (`/data/local/tmp`) otherwise.
export MSYS_NO_PATHCONV=1

filter="${1:-}"
package="dev.rustnative.devicetests"
runner="$package/dev.rustnative.android.RnInstrumentation"
target_dir="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | sed 's/\\\\/\//g')"
apk="$target_dir/rustnative/android/android-device-tests/app/build/outputs/apk/debug/app-debug.apk"

serials="${ANDROID_SERIAL:-$(adb devices | awk 'NR > 1 && $2 == "device" { print $1 }')}"
if [ -z "$serials" ]; then
    echo "android-device-test: no device or emulator is attached (adb devices)" >&2
    exit 1
fi

failed=0
for serial in $serials; do
    abi="$(adb -s "$serial" shell getprop ro.product.cpu.abi | tr -d '\r')"
    model="$(adb -s "$serial" shell getprop ro.product.model | tr -d '\r')"
    api="$(adb -s "$serial" shell getprop ro.build.version.sdk | tr -d '\r')"
    echo "== $model ($serial, API $api, $abi)"
    if ! (cd examples/android-device-tests && cargo run -q --manifest-path ../../Cargo.toml -p rustnative-cli -- build android --abi "$abi" --instrumentation); then
        echo "android-device-test: the build failed" >&2
        exit 1
    fi
    if ! adb -s "$serial" install -r -t "$apk"; then
        echo "android-device-test: the install failed (on some devices, confirm the install on the device)" >&2
        exit 1
    fi
    # Keep the screen on while plugged in for the run; afterwards the
    # device gets back whatever setting it had.
    stay_on="$(adb -s "$serial" shell settings get global stay_on_while_plugged_in | tr -d '\r')"
    services="$(adb -s "$serial" shell settings get secure enabled_accessibility_services | tr -d '\r')"
    accessibility="$(adb -s "$serial" shell settings get secure accessibility_enabled | tr -d '\r')"
    adb -s "$serial" shell svc power stayon usb
    log="$(mktemp)"
    adb -s "$serial" shell am instrument -r -w -e filter "'$filter'" "$runner" > "$log" 2>&1
    adb -s "$serial" shell settings put global stay_on_while_plugged_in "${stay_on:-0}"
    # The accessibility suite starts TalkBack and restores it; a run that
    # crashed partway cannot, so the services the device had are put back.
    if [ "$services" = "null" ] || [ -z "$services" ]; then
        adb -s "$serial" shell settings delete secure enabled_accessibility_services > /dev/null
    else
        adb -s "$serial" shell settings put secure enabled_accessibility_services "$services"
    fi
    adb -s "$serial" shell settings put secure accessibility_enabled "${accessibility:-0}"
    # Each test is a block of `INSTRUMENTATION_STATUS: key=value` lines
    # closed by `INSTRUMENTATION_STATUS_CODE: 1` (started), `0` (passed), or
    # `-2` (failed).
    awk '
        /^INSTRUMENTATION_STATUS: test=/ { test = substr($0, index($0, "=") + 1) }
        /^INSTRUMENTATION_STATUS: stack=/ { stack = substr($0, index($0, "=") + 1); collecting = 1; next }
        /^INSTRUMENTATION_STATUS/ { collecting = 0 }
        collecting { stack = stack "\n    " $0 }
        /^INSTRUMENTATION_STATUS_CODE: 0/ { print "ok      " test; passed++; stack = "" }
        /^INSTRUMENTATION_STATUS_CODE: -2/ { print "FAILED  " test "\n    " stack; failures++; stack = "" }
        /^INSTRUMENTATION_RESULT: shortMsg=/ { print "crashed: " substr($0, index($0, "=") + 1); failures++ }
        /^INSTRUMENTATION_FAILED/ { print "the instrumentation did not run: " $0; failures++ }
        END {
            printf "%d passed, %d failed\n", passed, failures
            if (failures > 0 || passed == 0) exit 1
        }
    ' "$log" || failed=1
    if [ "$failed" -ne 0 ]; then
        echo "-- raw output: $log"
    else
        rm -f "$log"
    fi
done
exit "$failed"
