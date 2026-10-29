package com.linofish.momentstays

/**
 * Thin Kotlin binding over the Rust core's JNI exports (see
 * `core/src/jni.rs`). The native library is built by
 * `core/android/build.sh` into `jniLibs/<abi>/libmomentstays_core.so`.
 *
 * `inspect` takes the raw bytes of one file (JPEG Motion Photo, HEIC
 * still, MP4/MOV video) and returns the same JSON summary the
 * `momentstays-core inspect` CLI prints, or null when the input
 * cannot be inspected (the native side never throws across FFI).
 *
 * The JSON shape is the `inspect` output: format, video range,
 * timestamp, HDR parts and warnings. Parsing it is the app's job;
 * this object deliberately does no JSON work so the binding stays
 * stable while the summary evolves.
 */
object Core {
    init {
        System.loadLibrary("momentstays_core")
    }

    /**
     * Inspect one file's bytes. Returns null on failure (including a
     * null native string, which the JNI side maps to null here).
     */
    external fun inspect(data: ByteArray): String?
}
