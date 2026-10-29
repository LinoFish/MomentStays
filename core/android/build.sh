#!/bin/sh
# CI-style Android cross-compile for momentstays-core (ROADMAP A2).
# Builds the cdylib for aarch64 / armv7 / x86_64 and places the .so
# files under core/android/jniLibs/<abi>/, the layout Android Studio
# expects (copy that directory into the app module, or point
# sourceSets at it — see README.md).
#
# Requires: Android NDK (ANDROID_NDK_HOME), cargo-ndk, and the three
# Rust targets (installed below if missing). The .so files are build
# artifacts: they are NOT committed (the GitHub push tool only takes
# text); CI rebuilds them from this script.
set -e
cd "$(dirname "$0")/.."

if [ -z "$ANDROID_NDK_HOME" ]; then
    echo "ANDROID_NDK_HOME is not set; point it at an NDK, e.g.:" >&2
    echo "  export ANDROID_NDK_HOME=\$HOME/workspace/android-ndk-r27c" >&2
    exit 1
fi
command -v cargo-ndk >/dev/null 2>&1 || { echo "cargo-ndk is not installed (cargo install cargo-ndk)" >&2; exit 1; }

rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

cargo ndk \
    --target aarch64-linux-android \
    --target armv7-linux-androideabi \
    --target x86_64-linux-android \
    --output-dir android/jniLibs \
    build --release --lib

echo "Built:"
find android/jniLibs -name '*.so' | sort
echo "Next: copy android/jniLibs into the app module and add the Core.kt binding."
