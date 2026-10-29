# Android binding (ROADMAP A2)

The Rust core ships to Android as a `cdylib` (`libmomentstays_core.so`)
with a thin hand-written JNI layer (`core/src/jni.rs`, no jni crate)
over the C ABI (`core/src/capi.rs`). The Kotlin side is a single
object, `src/main/java/com/linofish/momentstays/Core.kt`:

```kotlin
object Core {
    init { System.loadLibrary("momentstays_core") }
    external fun inspect(data: ByteArray): String?
}
```

`inspect` takes one file's bytes and returns the same JSON summary
the `momentstays-core inspect` CLI prints, or null on failure. The
native side never throws across FFI (panics are caught, like the C
ABI) and never touches the filesystem — the app reads the bytes
(MediaStore, SAF, whatever) and hands them over.

## Building the .so files

```sh
export ANDROID_NDK_HOME=$HOME/workspace/android-ndk-r27c  # or your NDK
sh core/android/build.sh
```

This runs `cargo ndk` for `aarch64-linux-android`,
`armv7-linux-androideabi` and `x86_64-linux-android` (release) and
places the libraries under `core/android/jniLibs/<abi>/`:

```
jniLibs/arm64-v8a/libmomentstays_core.so
jniLibs/armeabi-v7a/libmomentstays_core.so
jniLibs/x86_64/libmomentstays_core.so
```

Copy `jniLibs` into the app module (or point a source set at it):

```groovy
android {
    sourceSets {
        main { jniLibs.srcDirs += ["core/android/jniLibs"] }
    }
}
```

The `.so` files are build artifacts and are NOT committed (the
GitHub push tool only takes text). CI rebuilds them from `build.sh`
(see the rust-core workflow, milestone 8).

## What was verified where

- Linux host (this repo, `tests/jni_smoke.rs`): the JNI shim logic
  end to end through a fake JNIEnv table — array in, inspect JSON
  out, release with `JNI_ABORT`, NULL on null array / null env /
  unavailable elements, `JNI_OnLoad` reports 1.6. JNI table indices
  were read from NDK r27c `jni.h` (`struct JNINativeInterface`:
  NewStringUTF 167, GetArrayLength 171, GetByteArrayElements 184,
  ReleaseByteArrayElements 192).
- Linux host (this repo, `build.sh` run): the `.so` files compile
  for all three targets and export `JNI_OnLoad`,
  `Java_com_linofish_momentstays_Core_inspect`,
  `momentstays_inspect`, `momentstays_version`,
  `momentstays_free_string` (checked with the NDK's `llvm-nm`).
- NOT verified here (needs a device or emulator): loading the
  library from a real Android app, calling `Core.inspect` from
  Kotlin, and any MediaStore / file-picker integration. The `.so`
  cannot run on a Linux host (bionic-linked), so a device run is
  the only true end-to-end check.

## API levels

cargo-ndk defaults to the NDK's platform levels; the build script
does not pin one. If the app targets an older minSdk, pass
`--platform <level>` to the `cargo ndk` invocation in `build.sh`
and keep it at or below the app's minSdk.
