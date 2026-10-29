//! Host smoke test for the JNI shim (ROADMAP A2): drives
//! `Java_com_linofish_momentstays_Core_inspect` through a fake
//! JNIEnv table, verifying the shim logic end to end (array in,
//! JSON string out, release with JNI_ABORT, NULL on failure paths).
//! Table indices come from the lib (verified against NDK r27c
//! `jni.h` when the shim was written); the `.so` symbol check
//! happens in the Android build step, not here.
//!
//! One test function with sequential steps: the fake-table state
//! lives in statics, so steps must not run concurrently.

mod common;

use base64::Engine;
use common::build_still;
use momentstays_core::heifwrite::assemble_heic;
use momentstays_core::jni::{
    Java_com_linofish_momentstays_Core_inspect, JNI_OnLoad, JNIEnv, Jbyte, JbyteArray, Jobject,
    Jsize, Jstring, IDX_GET_ARRAY_LENGTH, IDX_GET_BYTE_ARRAY_ELEMENTS, IDX_NEW_STRING_UTF,
    IDX_RELEASE_BYTE_ARRAY_ELEMENTS, JNI_ABORT, JNI_VERSION_1_6,
};
use momentstays_core::model::Vendor;
use std::ffi::c_void;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;

static FAKE_DATA: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static FAKE_LEN: AtomicUsize = AtomicUsize::new(0);
static FAKE_ELEMS_NULL: AtomicBool = AtomicBool::new(false);
static CAPTURED: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static RELEASE_CALLED: AtomicBool = AtomicBool::new(false);
static RELEASE_MODE: AtomicI32 = AtomicI32::new(-1);
static NEW_STRING_CALLS: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" fn fake_get_array_length(_env: *mut JNIEnv, _array: Jobject) -> Jsize {
    FAKE_LEN.load(Ordering::SeqCst) as Jsize
}

unsafe extern "C" fn fake_get_byte_array_elements(
    _env: *mut JNIEnv,
    _array: JbyteArray,
    _is_copy: *mut u8,
) -> *mut Jbyte {
    if FAKE_ELEMS_NULL.load(Ordering::SeqCst) {
        std::ptr::null_mut()
    } else {
        FAKE_DATA.load(Ordering::SeqCst) as *mut Jbyte
    }
}

unsafe extern "C" fn fake_release(
    _env: *mut JNIEnv,
    _array: JbyteArray,
    _elems: *mut Jbyte,
    mode: i32,
) {
    RELEASE_CALLED.store(true, Ordering::SeqCst);
    RELEASE_MODE.store(mode, Ordering::SeqCst);
}

unsafe extern "C" fn fake_new_string_utf(_env: *mut JNIEnv, s: *const std::ffi::c_char) -> Jstring {
    NEW_STRING_CALLS.fetch_add(1, Ordering::SeqCst);
    let mut v = Vec::new();
    let mut p = s;
    while *p != 0 {
        v.push(*p as u8);
        p = p.add(1);
    }
    *CAPTURED.lock().unwrap() = v;
    0x1234 as Jstring
}

fn real_mp4() -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures-samples/real.mp4.b64");
    let text = fs::read_to_string(path).unwrap();
    base64::engine::general_purpose::STANDARD.decode(text.trim_ascii()).unwrap()
}

#[test]
fn jni_shim_roundtrip_and_null_paths() {
    let mut table: [*const c_void; 256] = [std::ptr::null(); 256];
    table[IDX_NEW_STRING_UTF] = fake_new_string_utf as *const c_void;
    table[IDX_GET_ARRAY_LENGTH] = fake_get_array_length as *const c_void;
    table[IDX_GET_BYTE_ARRAY_ELEMENTS] = fake_get_byte_array_elements as *const c_void;
    table[IDX_RELEASE_BYTE_ARRAY_ELEMENTS] = fake_release as *const c_void;
    let mut env = JNIEnv { functions: table.as_ptr() };
    let env_ptr: *mut JNIEnv = &mut env;
    let dummy_array = 0x5678 as JbyteArray;

    // 1. Valid motion HEIC -> JSON string mentioning google-heic.
    let still = build_still(true, true);
    let video = real_mp4();
    let motion = assemble_heic(&still, &video, Vendor::Google, 1_420_000).unwrap();
    FAKE_DATA.store(motion.as_ptr() as *mut u8, Ordering::SeqCst);
    FAKE_LEN.store(motion.len(), Ordering::SeqCst);
    FAKE_ELEMS_NULL.store(false, Ordering::SeqCst);
    RELEASE_CALLED.store(false, Ordering::SeqCst);
    let out = unsafe {
        Java_com_linofish_momentstays_Core_inspect(env_ptr, std::ptr::null_mut(), dummy_array)
    };
    assert!(!out.is_null(), "non-null jstring");
    assert_eq!(out as usize, 0x1234, "fake NewStringUTF return passes through");
    assert!(RELEASE_CALLED.load(Ordering::SeqCst), "array released");
    assert_eq!(RELEASE_MODE.load(Ordering::SeqCst), JNI_ABORT, "released with JNI_ABORT");
    let json = String::from_utf8(CAPTURED.lock().unwrap().clone()).unwrap();
    assert!(json.contains("google-heic"), "inspect JSON mentions format: {json}");
    assert!(json.contains("1420000"), "cover time in JSON: {json}");
    FAKE_DATA.store(std::ptr::null_mut(), Ordering::SeqCst);

    // 2. Null array -> null, no NewStringUTF call.
    let calls_before = NEW_STRING_CALLS.load(Ordering::SeqCst);
    let out = unsafe {
        Java_com_linofish_momentstays_Core_inspect(env_ptr, std::ptr::null_mut(), std::ptr::null_mut())
    };
    assert!(out.is_null());
    assert_eq!(NEW_STRING_CALLS.load(Ordering::SeqCst), calls_before);

    // 3. Null env -> null.
    let out = unsafe {
        Java_com_linofish_momentstays_Core_inspect(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dummy_array,
        )
    };
    assert!(out.is_null());

    // 4. Elements unavailable -> null, no NewStringUTF call.
    FAKE_ELEMS_NULL.store(true, Ordering::SeqCst);
    let out = unsafe {
        Java_com_linofish_momentstays_Core_inspect(env_ptr, std::ptr::null_mut(), dummy_array)
    };
    assert!(out.is_null());
    assert_eq!(NEW_STRING_CALLS.load(Ordering::SeqCst), calls_before);

    // 5. JNI_OnLoad reports 1.6.
    assert_eq!(
        unsafe { JNI_OnLoad(std::ptr::null_mut(), std::ptr::null_mut()) },
        JNI_VERSION_1_6
    );
}
