#!/usr/bin/env bash
# Exports what a shell needs to build and test the Android backend
# (`docs/android.md`): the JDK, the SDK and NDK, and Cargo's linker and C
# compiler for each Android target, pointed at the NDK's LLVM toolchain for
# the minimum API level the backend supports. `rustnative build android`
# performs the same discovery, so the gate and the CLI agree.
#
# Usage: `source tools/android-env.sh`
#
# Overrides: ANDROID_HOME (or ANDROID_SDK_ROOT), ANDROID_NDK_HOME,
# JAVA_HOME, RUSTNATIVE_ANDROID_API (default 26).

android_api="${RUSTNATIVE_ANDROID_API:-26}"

: "${ANDROID_HOME:=${ANDROID_SDK_ROOT:-}}"
if [ -z "$ANDROID_HOME" ]; then
    for candidate in "$LOCALAPPDATA/Android/Sdk" "$HOME/Android/Sdk" "$HOME/Library/Android/sdk"; do
        if [ -d "$candidate" ]; then ANDROID_HOME="$candidate"; break; fi
    done
fi
if [ -z "$ANDROID_HOME" ] || [ ! -d "$ANDROID_HOME" ]; then
    echo "android-env: no Android SDK (set ANDROID_HOME)" >&2
    return 1 2>/dev/null || exit 1
fi
ANDROID_HOME="$(cd "$ANDROID_HOME" && pwd)"
# Native Windows programs (Cargo, the linker, Gradle, `rustnative`) need
# `E:/…` paths, not Git Bash's `/e/…`.
native_path() {
    if command -v cygpath > /dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}
ANDROID_HOME="$(native_path "$ANDROID_HOME")"
export ANDROID_HOME ANDROID_SDK_ROOT="$ANDROID_HOME"

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    # The newest NDK the SDK manager installed.
    ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/*/ 2>/dev/null | sort -V | tail -1)"
    ANDROID_NDK_HOME="${ANDROID_NDK_HOME%/}"
fi
if [ -z "$ANDROID_NDK_HOME" ] || [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo "android-env: no NDK under $ANDROID_HOME/ndk (sdkmanager ndk/<version>)" >&2
    return 1 2>/dev/null || exit 1
fi
ANDROID_NDK_HOME="$(native_path "$ANDROID_NDK_HOME")"
export ANDROID_NDK_HOME ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"

if [ -z "${JAVA_HOME:-}" ]; then
    for candidate in "/e/Program Files/Android/Android Studio/jbr" \
        "/c/Program Files/Android/Android Studio/jbr" \
        "/opt/android-studio/jbr" "/Applications/Android Studio.app/Contents/jbr/Contents/Home"; do
        if [ -x "$candidate/bin/javac" ] || [ -x "$candidate/bin/javac.exe" ]; then
            JAVA_HOME="$candidate"; break
        fi
    done
fi
if [ -n "${JAVA_HOME:-}" ]; then
    export PATH="$JAVA_HOME/bin:$PATH"
    JAVA_HOME="$(native_path "$JAVA_HOME")"
    export JAVA_HOME
fi

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) host=windows-x86_64; ext=".cmd"; exe=".exe" ;;
    Darwin) host=darwin-x86_64; ext=""; exe="" ;;
    *) host=linux-x86_64; ext=""; exe="" ;;
esac
llvm="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$host/bin"
posix_path() {
    if command -v cygpath > /dev/null 2>&1; then cygpath -u "$1"; else printf '%s' "$1"; fi
}
sdk_posix="$(posix_path "$ANDROID_HOME")"
export PATH="$sdk_posix/platform-tools:$sdk_posix/cmdline-tools/latest/bin:$PATH"

for pair in aarch64-linux-android:aarch64-linux-android \
    armv7-linux-androideabi:armv7a-linux-androideabi \
    x86_64-linux-android:x86_64-linux-android \
    i686-linux-android:i686-linux-android; do
    target="${pair%%:*}"
    clang="${pair##*:}${android_api}-clang"
    upper="$(echo "$target" | tr 'a-z-' 'A-Z_')"
    lower="$(echo "$target" | tr '-' '_')"
    export "CARGO_TARGET_${upper}_LINKER=$llvm/$clang$ext"
    export "CC_${lower}=$llvm/$clang$ext"
    export "CXX_${lower}=$llvm/$clang++$ext"
    export "AR_${lower}=$llvm/llvm-ar$exe"
done
export RUSTNATIVE_ANDROID_API="$android_api"
