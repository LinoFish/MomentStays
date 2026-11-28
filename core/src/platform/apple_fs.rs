//! File-system facts and Foundation path semantics for the input-selection
//! decision (`engine::decisions::selection`), read the way Foundation reads
//! them: CoreFoundation resource values and the CFURL directory enumerator
//! (what `FileManager.enumerator(at:options:)` is built on), Finder string
//! ordering, and `getattrlist(ATTR_CMN_FULLPATH)` for symlink resolution.
use crate::engine::decisions::selection::{
    foundation_normalized, is_extender, Entry, Facts, Fs, Url, Walk,
};
use std::cmp::Ordering;
use std::ffi::{c_void, CString};
use std::ptr;

type Ref = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

#[repr(C)]
struct ArrayCallBacks {
    version: isize,
    retain: Ref,
    release: Ref,
    copy_description: Ref,
    equal: Ref,
}

const UTF8: u32 = 0x0800_0100;
const POSIX_STYLE: isize = 0;
// CFURLEnumeratorOptions
const DESCEND_RECURSIVELY: usize = 1 << 0;
const SKIP_INVISIBLES: usize = 1 << 1;
const SKIP_PACKAGE_CONTENTS: usize = 1 << 3;
// CFURLEnumeratorResult
const ENUM_SUCCESS: isize = 1;
const ENUM_END: isize = 2;
const ENUM_ERROR: isize = 3;
// CFStringCompareFlags of -[NSString localizedStandardCompare:]
const CASE_INSENSITIVE: usize = 1;
const NUMERICALLY: usize = 64;
const WIDTH_INSENSITIVE: usize = 256;
const FORCED_ORDERING: usize = 512;
// kCFCharacterSetNonBase
const NON_BASE: isize = 8;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFURLIsDirectoryKey: Ref;
    static kCFURLIsRegularFileKey: Ref;
    static kCFURLIsSymbolicLinkKey: Ref;
    static kCFTypeArrayCallBacks: ArrayCallBacks;
    fn CFRelease(v: Ref);
    fn CFStringCreateWithBytes(a: Ref, p: *const u8, n: isize, enc: u32, ext: u8) -> Ref;
    fn CFStringGetLength(s: Ref) -> isize;
    fn CFStringGetBytes(
        s: Ref,
        range: CFRange,
        enc: u32,
        loss: u8,
        external: u8,
        buf: *mut u8,
        max: isize,
        used: *mut isize,
    ) -> isize;
    fn CFStringCompareWithOptionsAndLocale(
        a: Ref,
        b: Ref,
        range: CFRange,
        options: usize,
        locale: Ref,
    ) -> isize;
    fn CFLocaleCopyCurrent() -> Ref;
    fn CFURLCreateWithFileSystemPath(a: Ref, path: Ref, style: isize, dir: u8) -> Ref;
    fn CFURLCopyFileSystemPath(url: Ref, style: isize) -> Ref;
    fn CFURLCopyResourcePropertyForKey(url: Ref, key: Ref, value: *mut Ref, error: *mut Ref) -> u8;
    fn CFURLEnumeratorCreateForDirectoryURL(a: Ref, url: Ref, options: usize, keys: Ref) -> Ref;
    fn CFURLEnumeratorGetNextURL(e: Ref, url: *mut Ref, error: *mut Ref) -> isize;
    fn CFArrayCreate(a: Ref, values: *const Ref, n: isize, callbacks: *const c_void) -> Ref;
    fn CFBooleanGetValue(b: Ref) -> u8;
    fn CFErrorCopyDescription(e: Ref) -> Ref;
    fn CFCharacterSetGetPredefined(id: isize) -> Ref;
    fn CFCharacterSetIsLongCharacterMember(set: Ref, c: u32) -> u8;
}

/// An owned (Create/Copy rule) CoreFoundation reference.
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}

fn cf_string(s: &str) -> Owned {
    Owned(unsafe { CFStringCreateWithBytes(ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0) })
}

unsafe fn rust_string(s: Ref) -> String {
    if s.is_null() {
        return String::new();
    }
    let len = CFStringGetLength(s);
    let range = CFRange {
        location: 0,
        length: len,
    };
    let mut used = 0isize;
    CFStringGetBytes(s, range, UTF8, 0, 0, ptr::null_mut(), 0, &mut used);
    let mut buf = vec![0u8; used.max(0) as usize];
    CFStringGetBytes(s, range, UTF8, 0, 0, buf.as_mut_ptr(), used, &mut used);
    buf.truncate(used.max(0) as usize);
    String::from_utf8(buf).unwrap_or_default()
}

unsafe fn error_text(e: Ref) -> String {
    if e.is_null() {
        return String::new();
    }
    let d = Owned(CFErrorCopyDescription(e));
    rust_string(d.0)
}

fn file_url(path: &str, dir: bool) -> Owned {
    let s = cf_string(path);
    Owned(unsafe { CFURLCreateWithFileSystemPath(ptr::null(), s.0, POSIX_STYLE, u8::from(dir)) })
}

/// One boolean resource value; Err = the error's description.
unsafe fn flag(url: Ref, key: Ref) -> std::result::Result<bool, String> {
    let mut value: Ref = ptr::null();
    let mut error: Ref = ptr::null();
    if CFURLCopyResourcePropertyForKey(url, key, &mut value, &mut error) == 0 {
        let text = error_text(error);
        if !error.is_null() {
            CFRelease(error);
        }
        return Err(text);
    }
    if value.is_null() {
        return Ok(false);
    }
    let v = CFBooleanGetValue(value) != 0;
    CFRelease(value);
    Ok(v)
}

/// `_resolvingSymlinksInPath` for an absolute path: the full path the
/// kernel reports (`getattrlist(ATTR_CMN_FULLPATH)`, following symlinks).
fn full_path(path: &str) -> Option<String> {
    if !path.starts_with('/') {
        return None;
    }
    let c = CString::new(path).ok()?;
    #[repr(C)]
    struct Buffer {
        length: u32,
        attr: libc::attrreference_t,
        path: [u8; libc::PATH_MAX as usize],
    }
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_FULLPATH,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buf: Buffer = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::getattrlist(
            c.as_ptr(),
            (&mut list as *mut libc::attrlist).cast(),
            (&mut buf as *mut Buffer).cast(),
            std::mem::size_of::<Buffer>(),
            0,
        )
    };
    if rc != 0 || buf.attr.attr_length == 0 {
        return None;
    }
    let end = buf
        .path
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(buf.path.len());
    String::from_utf8(buf.path[..end].to_vec()).ok()
}

fn exists(path: &str) -> bool {
    let Ok(c) = CString::new(path) else {
        return false;
    };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    unsafe { libc::stat(c.as_ptr(), &mut st) == 0 }
}

pub struct AppleFs;

impl Fs for AppleFs {
    fn facts(&self, path: &str) -> std::result::Result<Facts, String> {
        let url = file_url(path, false);
        if url.0.is_null() {
            return Err(format!("invalid path {path}"));
        }
        unsafe {
            let is_dir = flag(url.0, kCFURLIsDirectoryKey)?;
            let is_regular = flag(url.0, kCFURLIsRegularFileKey)?;
            Ok(Facts { is_dir, is_regular })
        }
    }

    fn walk(&self, root: &Url) -> Option<Walk> {
        let url = file_url(&root.path, root.dir);
        if url.0.is_null() {
            return None;
        }
        unsafe {
            let keys = [kCFURLIsRegularFileKey, kCFURLIsSymbolicLinkKey];
            let keys = Owned(CFArrayCreate(
                ptr::null(),
                keys.as_ptr(),
                keys.len() as isize,
                (&raw const kCFTypeArrayCallBacks).cast(),
            ));
            let e = Owned(CFURLEnumeratorCreateForDirectoryURL(
                ptr::null(),
                url.0,
                DESCEND_RECURSIVELY | SKIP_INVISIBLES | SKIP_PACKAGE_CONTENTS,
                keys.0,
            ));
            if e.0.is_null() {
                return None;
            }
            let mut walk = Walk::default();
            loop {
                let mut next: Ref = ptr::null();
                let mut error: Ref = ptr::null();
                match CFURLEnumeratorGetNextURL(e.0, &mut next, &mut error) {
                    ENUM_END => break,
                    ENUM_ERROR => walk.errors.push(error_text(error)),
                    ENUM_SUCCESS if !next.is_null() => {
                        let p = Owned(CFURLCopyFileSystemPath(next, POSIX_STYLE));
                        let path = rust_string(p.0);
                        let entry = flag(next, kCFURLIsRegularFileKey).and_then(|is_regular| {
                            Ok(Entry {
                                path,
                                is_regular,
                                is_symlink: flag(next, kCFURLIsSymbolicLinkKey)?,
                            })
                        });
                        walk.entries.push(entry);
                    }
                    _ => {}
                }
            }
            Some(walk)
        }
    }

    fn compare(&self, a: &str, b: &str) -> Ordering {
        let (x, y) = (cf_string(a), cf_string(b));
        unsafe {
            let locale = Owned(CFLocaleCopyCurrent());
            compare_cf(x.0, y.0, locale.0)
        }
    }

    fn sort(&self, v: &mut Vec<Url>) {
        unsafe {
            let locale = Owned(CFLocaleCopyCurrent());
            let mut keyed: Vec<(Owned, Url)> =
                v.drain(..).map(|u| (cf_string(&u.path), u)).collect();
            keyed.sort_by(|a, b| compare_cf(a.0 .0, b.0 .0, locale.0));
            v.extend(keyed.into_iter().map(|(_, u)| u));
        }
    }

    fn normalized(&self, url: &Url) -> Url {
        foundation_normalized(url, &full_path, &exists)
    }

    fn extends_cluster(&self, c: char) -> bool {
        is_extender(c)
            || unsafe {
                CFCharacterSetIsLongCharacterMember(CFCharacterSetGetPredefined(NON_BASE), c as u32)
                    != 0
            }
    }
}

unsafe fn compare_cf(a: Ref, b: Ref, locale: Ref) -> Ordering {
    let range = CFRange {
        location: 0,
        length: CFStringGetLength(a),
    };
    match CFStringCompareWithOptionsAndLocale(
        a,
        b,
        range,
        CASE_INSENSITIVE | NUMERICALLY | WIDTH_INSENSITIVE | FORCED_ORDERING,
        locale,
    ) {
        r if r < 0 => Ordering::Less,
        0 => Ordering::Equal,
        _ => Ordering::Greater,
    }
}
