//! AVFoundation frame decoding via the Objective-C runtime, confined to Apple.
//! No application callbacks: Rust owns timing input and the returned CGImage.
use super::*;
use std::ffi::{c_char, CString};
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Time {
    value: i64,
    scale: i32,
    flags: u32,
    epoch: i64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Range {
    start: Time,
    duration: Time,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Mapping {
    source: Range,
    target: Range,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Transform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}
#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Ref;
    fn sel_registerName(name: *const c_char) -> Ref;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}
#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMTimeMapTimeFromRangeToRange(t: Time, from: Range, to: Range) -> Time;
    fn CMTimeSubtract(a: Time, b: Time) -> Time;
    fn CMTimeRangeContainsTime(range: Range, t: Time) -> u8;
    fn CMSampleBufferGetNumSamples(sample: Ref) -> isize;
}
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVMediaTypeVideo: Ref;
    static AVMediaTypeAudio: Ref;
    static AVFormatIDKey: Ref;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        a: Ref,
        p: *const u8,
        n: isize,
        directory: u8,
    ) -> Ref;
}
struct Pool(*mut c_void);
impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}
unsafe fn selector(name: &str) -> Ref {
    sel_registerName(CString::new(name).unwrap().as_ptr())
}
unsafe fn class(name: &str) -> Ref {
    objc_getClass(CString::new(name).unwrap().as_ptr())
}
unsafe fn object(obj: Ref, name: &str) -> Ref {
    let send: unsafe extern "C" fn(Ref, Ref) -> Ref =
        std::mem::transmute(objc_msgSend as *const ());
    send(obj, selector(name))
}
unsafe fn count(obj: Ref) -> usize {
    let send: unsafe extern "C" fn(Ref, Ref) -> usize =
        std::mem::transmute(objc_msgSend as *const ());
    send(obj, selector("count"))
}
unsafe fn at(obj: Ref, index: usize) -> Ref {
    let send: unsafe extern "C" fn(Ref, Ref, usize) -> Ref =
        std::mem::transmute(objc_msgSend as *const ());
    send(obj, selector("objectAtIndex:"), index)
}
unsafe fn structure<T: Default>(obj: Ref, name: &str) -> T {
    // All structs used here exceed 16 bytes. Darwin x86_64 returns them
    // indirectly; arm64 uses the normal message-send entry point.
    #[cfg(target_arch = "x86_64")]
    {
        let mut out = T::default();
        let send: unsafe extern "C" fn(*mut T, Ref, Ref) =
            std::mem::transmute(objc_msgSend_stret as *const ());
        send(&mut out, obj, selector(name));
        out
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let send: unsafe extern "C" fn(Ref, Ref) -> T =
            std::mem::transmute(objc_msgSend as *const ());
        send(obj, selector(name))
    }
}
unsafe fn error(error: Ref, fallback: &str) -> Error {
    if !error.is_null() {
        let description = object(error, "localizedDescription");
        let p = object(description, "UTF8String") as *const c_char;
        if !p.is_null() {
            return Error::new(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned());
        }
    }
    Error::new(fallback)
}

/// Stream decoded PCM without playing or retaining it. Track presence alone
/// does not prove that AVFoundation can open the QuickTime audio description.
pub(super) fn audio_decodes(path: &Path, expected_tracks: usize) -> Result<bool> {
    unsafe {
        let _pool = Pool(objc_autoreleasePoolPush());
        use std::os::unix::ffi::OsStrExt;
        let p = path.as_os_str().as_bytes();
        let url = Owned::new(CFURLCreateFromFileSystemRepresentation(
            ptr::null(), p.as_ptr(), p.len() as isize, 0,
        ), "音频校验 URL")?;
        let one: unsafe extern "C" fn(Ref, Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        let two: unsafe extern "C" fn(Ref, Ref, Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        let yes: unsafe extern "C" fn(Ref, Ref) -> u8 =
            std::mem::transmute(objc_msgSend as *const ());
        let status: unsafe extern "C" fn(Ref, Ref) -> isize =
            std::mem::transmute(objc_msgSend as *const ());
        let command: unsafe extern "C" fn(Ref, Ref) =
            std::mem::transmute(objc_msgSend as *const ());
        let add_output: unsafe extern "C" fn(Ref, Ref, Ref) =
            std::mem::transmute(objc_msgSend as *const ());
        let asset = two(class("AVURLAsset"), selector("URLAssetWithURL:options:"), url.0, ptr::null());
        if asset.is_null() { return Err(Error::new("音频校验无法打开 MOV")); }
        let tracks = one(asset, selector("tracksWithMediaType:"), AVMediaTypeAudio);
        if count(tracks) != expected_tracks { return Ok(false); }
        if count(tracks) > 32 { return Err(Error::new("音频校验轨道数量超过限制")); }
        let format = u32::from_be_bytes(*b"lpcm") as i64;
        let number = Owned::new(CFNumberCreate(ptr::null(), 4, (&format as *const i64).cast()), "PCM 格式")?;
        let settings = two(class("NSDictionary"), selector("dictionaryWithObject:forKey:"), number.0, AVFormatIDKey);
        for i in 0..count(tracks) {
            let mut error_ref: Ref = ptr::null();
            let init: unsafe extern "C" fn(Ref, Ref, Ref, *mut Ref) -> Ref =
                std::mem::transmute(objc_msgSend as *const ());
            let reader = init(object(class("AVAssetReader"), "alloc"), selector("initWithAsset:error:"), asset, &mut error_ref);
            if reader.is_null() { return Err(error(error_ref, "无法创建音频解码器")); }
            object(reader, "autorelease");
            let output = two(object(class("AVAssetReaderTrackOutput"), "alloc"), selector("initWithTrack:outputSettings:"), at(tracks, i), settings);
            if output.is_null() { return Err(Error::new("无法创建 PCM 解码输出")); }
            object(output, "autorelease");
            let can_add: unsafe extern "C" fn(Ref, Ref, Ref) -> u8 =
                std::mem::transmute(objc_msgSend as *const ());
            if can_add(reader, selector("canAddOutput:"), output) == 0 { return Ok(false); }
            add_output(reader, selector("addOutput:"), output);
            if yes(reader, selector("startReading")) == 0 { return Ok(false); }
            let started = std::time::Instant::now();
            let mut frames = 0u64;
            loop {
                let sample = object(output, "copyNextSampleBuffer");
                if sample.is_null() { break; }
                let n = CMSampleBufferGetNumSamples(sample);
                CFRelease(sample);
                if n < 0 { return Ok(false); }
                frames = frames.saturating_add(n as u64);
                if frames > 100_000_000 || started.elapsed().as_secs() > 30 {
                    command(reader, selector("cancelReading"));
                    return Err(Error::new("原生音频校验超过资源限制，未发布结果"));
                }
            }
            if status(reader, selector("status")) != 2 || frames == 0 { return Ok(false); }
        }
        Ok(true)
    }
}
pub(super) fn frame(path: &Path, index: usize, seconds: f64) -> Result<(Vec<u8>, u32, u32)> {
    if !seconds.is_finite() || seconds.abs() > i64::MAX as f64 / 1e9 {
        return Err(Error::new("视频帧时间无效"));
    }
    unsafe {
        let _pool = Pool(objc_autoreleasePoolPush());
        use std::os::unix::ffi::OsStrExt;
        let p = path.as_os_str().as_bytes();
        let url = Owned::new(
            CFURLCreateFromFileSystemRepresentation(ptr::null(), p.as_ptr(), p.len() as isize, 0),
            "视频 URL",
        )?;
        let asset_send: unsafe extern "C" fn(Ref, Ref, Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        let asset = asset_send(
            class("AVURLAsset"),
            selector("URLAssetWithURL:options:"),
            url.0,
            ptr::null(),
        );
        if asset.is_null() {
            return Err(Error::new("无法打开视频"));
        }
        let one: unsafe extern "C" fn(Ref, Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        let tracks = one(asset, selector("tracksWithMediaType:"), AVMediaTypeVideo);
        if index >= count(tracks) {
            return Err(Error::new("视频轨不存在"));
        }
        let track = at(tracks, index);
        let range: Range = structure(track, "timeRange");
        let media = Time {
            value: (seconds * 1e9).round() as i64,
            scale: 1_000_000_000,
            flags: 1,
            epoch: 0,
        };
        let segments = object(track, "segments");
        let mut presented = media;
        let mut hits = 0;
        for i in 0..count(segments) {
            let segment = at(segments, i);
            let boolean: unsafe extern "C" fn(Ref, Ref) -> i8 =
                std::mem::transmute(objc_msgSend as *const ());
            if boolean(segment, selector("isEmpty")) != 0 {
                continue;
            }
            let m: Mapping = structure(segment, "timeMapping");
            if CMTimeRangeContainsTime(m.source, media) != 0 {
                presented = CMTimeMapTimeFromRangeToRange(media, m.source, m.target);
                hits += 1;
            }
        }
        if count(segments) > 0 && hits != 1 {
            return Err(Error::new("视频帧时间无法唯一定位"));
        }
        let composition = object(class("AVMutableComposition"), "composition");
        let add: unsafe extern "C" fn(Ref, Ref, Ref, i32) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        let output = add(
            composition,
            selector("addMutableTrackWithMediaType:preferredTrackID:"),
            AVMediaTypeVideo,
            0,
        );
        if output.is_null() {
            return Err(Error::new("无法创建视频解码轨"));
        }
        let zero = Time {
            value: 0,
            scale: 1,
            flags: 1,
            epoch: 0,
        };
        let mut err = ptr::null();
        let insert: unsafe extern "C" fn(Ref, Ref, Range, Ref, Time, *mut Ref) -> i8 =
            std::mem::transmute(objc_msgSend as *const ());
        if insert(
            output,
            selector("insertTimeRange:ofTrack:atTime:error:"),
            range,
            track,
            zero,
            &mut err,
        ) == 0
        {
            return Err(error(err, "视频轨读取失败"));
        }
        let transform: Transform = structure(track, "preferredTransform");
        let set_transform: unsafe extern "C" fn(Ref, Ref, Transform) =
            std::mem::transmute(objc_msgSend as *const ());
        set_transform(output, selector("setPreferredTransform:"), transform);
        let generator = one(
            class("AVAssetImageGenerator"),
            selector("assetImageGeneratorWithAsset:"),
            composition,
        );
        if generator.is_null() {
            return Err(Error::new("无法创建视频帧解码器"));
        }
        let set_bool: unsafe extern "C" fn(Ref, Ref, i8) =
            std::mem::transmute(objc_msgSend as *const ());
        set_bool(generator, selector("setAppliesPreferredTrackTransform:"), 1);
        let set_time: unsafe extern "C" fn(Ref, Ref, Time) =
            std::mem::transmute(objc_msgSend as *const ());
        set_time(
            generator,
            selector("setRequestedTimeToleranceBefore:"),
            zero,
        );
        set_time(generator, selector("setRequestedTimeToleranceAfter:"), zero);
        let copy: unsafe extern "C" fn(Ref, Ref, Time, *mut Time, *mut Ref) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        let img = copy(
            generator,
            selector("copyCGImageAtTime:actualTime:error:"),
            CMTimeSubtract(presented, range.start),
            ptr::null_mut(),
            &mut err,
        );
        if img.is_null() {
            return Err(error(err, "视频帧解码失败"));
        }
        let image = Owned(img);
        let (w, h) = (CGImageGetWidth(image.0), CGImageGetHeight(image.0));
        dimensions(w, h, w)?;
        Ok((jpeg(image.0, 1.0, ptr::null())?, w as u32, h as u32))
    }
}
