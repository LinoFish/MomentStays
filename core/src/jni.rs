//! Minimal JNI bindings (ROADMAP A2) for the Android app: thin shims
//! over the C ABI, with no new dependency (no jni crate).
//!
//! Kotlin side (`core/android/.../Core.kt`):
//!   object Core { external fun inspect(data: ByteArray): String? }
//! which maps to `Java_com_linofish_momentstays_Core_inspect`
//! (JNIEnv, jobject, jbyteArray) -> jstring. A NULL return means
//! failure; the native side never throws across FFI (panics are
//! caught, like in the C ABI).
//!
//! JNI function-table indices below are read from the NDK's `jni.h`
//! (NDK r27c, `struct JNINativeInterface`: 4 reserved entries, then
//! GetVersion at 4). They are stable across NDK releases:
//! NewStringUTF 167, GetArrayLength 171, GetByteArrayElements 184,
//! ReleaseByteArrayElements 192. The host smoke test
//! (`tests/jni_smoke.rs`) drives this shim through a fake table.

use std::ffi::{c_char, c_void};

pub type JniInt = i32;
pub type JniBoolean = u8;

pub const JNI_VERSION_1_6: JniInt = 0x0001_0006;
pub const JNI_ABORT: JniInt = 2;

/// Indices into `JNINativeInterface` (see module docs).
pub const IDX_NEW_STRING_UTF: usize = 167;
pub const IDX_GET_ARRAY_LENGTH: usize = 171;
pub const IDX_GET_BYTE_ARRAY_ELEMENTS: usize = 184;
pub const IDX_RELEASE_BYTE_ARRAY_ELEMENTS: usize = 192;

pub type Jobject = *mut c_void;
pub type Jstring = *mut c_void;
pub type JbyteArray = *mut c_void;
pub type Jbyte = i8;
pub type Jsize = i32;

/// Mirrors `_JNIEnv`: a pointer to the function table. The JVM passes
/// a pointer to this struct; the host smoke test builds the same
/// layout by hand.
#[repr(C)]
pub struct JNIEnv {
    pub functions: *const *const c_void,
}

type NewStringUtfFn = unsafe extern "C" fn(*mut JNIEnv, *const c_char) -> Jstring;
type GetArrayLengthFn = unsafe extern "C" fn(*mut JNIEnv, Jobject) -> Jsize;
type GetByteArrayElementsFn =
    unsafe extern "C" fn(*mut JNIEnv, JbyteArray, *mut JniBoolean) -> *mut Jbyte;
type ReleaseByteArrayElementsFn =
    unsafe extern "C" fn(*mut JNIEnv, JbyteArray, *mut Jbyte, JniInt);

/// Read one function pointer out of the table, typed by the caller.
/// SAFETY: `idx` must name the matching function (see IDX_* above);
/// the JVM guarantees the table.
unsafe fn table_fn<T>(env: *mut JNIEnv, idx: usize) -> T {
    let entry = *(*env).functions.add(idx);
    std::mem::transmute_copy(&entry)
}

unsafe fn new_string_utf(env: *mut JNIEnv, s: *const c_char) -> Jstring {
    let f: NewStringUtfFn = table_fn(env, IDX_NEW_STRING_UTF);
    f(env, s)
}

unsafe fn get_array_length(env: *mut JNIEnv, array: Jobject) -> Jsize {
    let f: GetArrayLengthFn = table_fn(env, IDX_GET_ARRAY_LENGTH);
    f(env, array)
}

unsafe fn get_byte_array_elements(env: *mut JNIEnv, array: JbyteArray) -> *mut Jbyte {
    let f: GetByteArrayElementsFn = table_fn(env, IDX_GET_BYTE_ARRAY_ELEMENTS);
    f(env, array, std::ptr::null_mut())
}

unsafe fn release_byte_array_elements(env: *mut JNIEnv, array: JbyteArray, elems: *mut Jbyte, mode: JniInt) {
    let f: ReleaseByteArrayElementsFn = table_fn(env, IDX_RELEASE_BYTE_ARRAY_ELEMENTS);
    f(env, array, elems, mode)
}

/// Kotlin `Core.inspect(data: ByteArray): String?`.
#[no_mangle]
pub extern "C" fn Java_com_linofish_momentstays_Core_inspect(
    env: *mut JNIEnv,
    _this: Jobject,
    data: JbyteArray,
) -> Jstring {
    if env.is_null() || data.is_null() {
        return std::ptr::null_mut();
    }
    // A panic must never unwind across FFI.
    match std::panic::catch_unwind(|| unsafe { inspect_via_env(env, data) }) {
        Ok(s) => s,
        Err(_) => std::ptr::null_mut(),
    }
}

/// The actual shim body (panics caught by the caller).
///
/// SAFETY: `env` must be a live JNI environment and `data` a live
/// byte array; lengths and pointers come from the JVM.
unsafe fn inspect_via_env(env: *mut JNIEnv, data: JbyteArray) -> Jstring {
    let len = get_array_length(env, data);
    if len < 0 {
        return std::ptr::null_mut();
    }
    let elems = get_byte_array_elements(env, data);
    if elems.is_null() {
        return std::ptr::null_mut();
    }
    let bytes = std::slice::from_raw_parts(elems as *const u8, len as usize);
    let mut out: *mut c_char = std::ptr::null_mut();
    let rc = crate::capi::momentstays_inspect(bytes.as_ptr(), bytes.len(), &mut out);
    release_byte_array_elements(env, data, elems, JNI_ABORT);
    if rc != 0 || out.is_null() {
        if !out.is_null() {
            crate::capi::momentstays_free_string(out);
        }
        return std::ptr::null_mut();
    }
    let s = new_string_utf(env, out);
    crate::capi::momentstays_free_string(out);
    s
}

/// Standard JNI entry point: report the JNI version we implement.
#[no_mangle]
pub extern "C" fn JNI_OnLoad(_vm: *mut c_void, _reserved: *mut c_void) -> JniInt {
    JNI_VERSION_1_6
}
