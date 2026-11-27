//! libheif: HEIC primary-image decode and the Apple gain-map auxiliary image.
//! Only C functions present since libheif 1.16 are used.
use crate::{Error, ErrorKind};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

type Ctx = *mut c_void;
type HandlePtr = *mut c_void;
type Image = *mut c_void;
type ItemId = u32;
#[repr(C)]
struct HeifError {
    code: c_int,
    subcode: c_int,
    message: *const c_char,
}
extern "C" {
    fn heif_init(params: *mut c_void) -> HeifError;
    fn heif_context_alloc() -> Ctx;
    fn heif_context_free(ctx: Ctx);
    fn heif_context_read_from_memory_without_copy(
        ctx: Ctx,
        mem: *const c_void,
        size: usize,
        options: *const c_void,
    ) -> HeifError;
    fn heif_context_get_primary_image_handle(ctx: Ctx, out: *mut HandlePtr) -> HeifError;
    fn heif_image_handle_release(h: HandlePtr);
    fn heif_image_handle_get_luma_bits_per_pixel(h: HandlePtr) -> c_int;
    fn heif_image_handle_get_width(h: HandlePtr) -> c_int;
    fn heif_image_handle_get_height(h: HandlePtr) -> c_int;
    fn heif_image_handle_get_number_of_auxiliary_images(h: HandlePtr, filter: c_int) -> c_int;
    fn heif_image_handle_get_list_of_auxiliary_image_IDs(
        h: HandlePtr,
        filter: c_int,
        ids: *mut ItemId,
        count: c_int,
    ) -> c_int;
    fn heif_image_handle_get_auxiliary_image_handle(
        h: HandlePtr,
        id: ItemId,
        out: *mut HandlePtr,
    ) -> HeifError;
    fn heif_image_handle_get_auxiliary_type(h: HandlePtr, out: *mut *const c_char) -> HeifError;
    fn heif_image_handle_release_auxiliary_type(h: HandlePtr, out: *mut *const c_char);
    fn heif_image_handle_get_number_of_metadata_blocks(
        h: HandlePtr,
        filter: *const c_char,
    ) -> c_int;
    fn heif_image_handle_get_list_of_metadata_block_IDs(
        h: HandlePtr,
        filter: *const c_char,
        ids: *mut ItemId,
        count: c_int,
    ) -> c_int;
    fn heif_image_handle_get_metadata_content_type(h: HandlePtr, id: ItemId) -> *const c_char;
    fn heif_image_handle_get_metadata_size(h: HandlePtr, id: ItemId) -> usize;
    fn heif_image_handle_get_metadata(h: HandlePtr, id: ItemId, out: *mut c_void) -> HeifError;
    fn heif_image_handle_get_raw_color_profile_size(h: HandlePtr) -> usize;
    fn heif_image_handle_get_raw_color_profile(h: HandlePtr, out: *mut c_void) -> HeifError;
    fn heif_decoding_options_alloc() -> *mut u8;
    fn heif_decoding_options_free(o: *mut u8);
    fn heif_decode_image(
        h: HandlePtr,
        out: *mut Image,
        colorspace: c_int,
        chroma: c_int,
        options: *const u8,
    ) -> HeifError;
    fn heif_image_get_plane_readonly(img: Image, channel: c_int, stride: *mut c_int) -> *const u8;
    fn heif_image_get_width(img: Image, channel: c_int) -> c_int;
    fn heif_image_get_height(img: Image, channel: c_int) -> c_int;
    fn heif_image_get_bits_per_pixel_range(img: Image, channel: c_int) -> c_int;
    fn heif_image_release(img: Image);
}

const ERR_UNSUPPORTED_FEATURE: c_int = 4;
const ERR_DECODER_PLUGIN: c_int = 7;
const ERR_PLUGIN_LOADING: c_int = 11;
const SUB_UNSUPPORTED_CODEC: c_int = 3000;
const COLORSPACE_RGB: c_int = 1;
const COLORSPACE_MONOCHROME: c_int = 2;
const CHROMA_MONOCHROME: c_int = 0;
const CHROMA_INTERLEAVED_RGB: c_int = 10;
const CHANNEL_Y: c_int = 0;
const CHANNEL_INTERLEAVED: c_int = 10;
const MAX_SIDE: usize = 16384;
const MAX_BYTES: usize = 256 * 1024 * 1024;

/// A HEIF failure, classified so callers can tell "no decoder on this
/// system" (e.g. HEVC without codecs-extra) from damaged data.
pub enum Failure {
    MissingDecoder(String),
    Invalid(String),
}
impl Failure {
    pub fn into_error(self) -> Error {
        match self {
            Failure::MissingDecoder(m) => Error::with_kind(
                ErrorKind::Unsupported,
                format!("系统缺少 HEIF/HEVC 解码器：{m}"),
            ),
            Failure::Invalid(m) => Error::new(format!("HEIC 解码失败：{m}")),
        }
    }
}

fn check(e: HeifError) -> std::result::Result<(), Failure> {
    if e.code == 0 {
        return Ok(());
    }
    let message = if e.message.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(e.message) }
            .to_string_lossy()
            .into_owned()
    };
    let missing = (e.code == ERR_UNSUPPORTED_FEATURE && e.subcode == SUB_UNSUPPORTED_CODEC)
        || e.code == ERR_DECODER_PLUGIN
        || e.code == ERR_PLUGIN_LOADING;
    Err(if missing {
        Failure::MissingDecoder(message)
    } else {
        Failure::Invalid(message)
    })
}

static INIT: std::sync::Once = std::sync::Once::new();

/// A parsed file; borrows the input bytes (`without_copy`).
pub struct Context<'a> {
    ctx: Ctx,
    _data: std::marker::PhantomData<&'a [u8]>,
}
impl Drop for Context<'_> {
    fn drop(&mut self) {
        unsafe { heif_context_free(self.ctx) }
    }
}
pub struct Handle<'c> {
    h: HandlePtr,
    _ctx: std::marker::PhantomData<&'c ()>,
}
impl Drop for Handle<'_> {
    fn drop(&mut self) {
        unsafe { heif_image_handle_release(self.h) }
    }
}
struct Decoded(Image);
impl Drop for Decoded {
    fn drop(&mut self) {
        unsafe { heif_image_release(self.0) }
    }
}

impl<'a> Context<'a> {
    pub fn read(data: &'a [u8]) -> std::result::Result<Self, Failure> {
        INIT.call_once(|| unsafe {
            // Loads decoder plugins (libheif >= 1.14). Errors surface at decode.
            let _ = heif_init(ptr::null_mut());
        });
        let ctx = unsafe { heif_context_alloc() };
        if ctx.is_null() {
            return Err(Failure::Invalid("无法分配 HEIF 上下文".into()));
        }
        let context = Context {
            ctx,
            _data: std::marker::PhantomData,
        };
        check(unsafe {
            heif_context_read_from_memory_without_copy(
                ctx,
                data.as_ptr().cast(),
                data.len(),
                ptr::null(),
            )
        })?;
        Ok(context)
    }
    pub fn primary(&self) -> std::result::Result<Handle<'_>, Failure> {
        let mut h = ptr::null_mut();
        check(unsafe { heif_context_get_primary_image_handle(self.ctx, &mut h) })?;
        Ok(Handle {
            h,
            _ctx: std::marker::PhantomData,
        })
    }
}

/// Owned 8-bit plane.
pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixels: Vec<u8>,
}

impl Handle<'_> {
    /// Auxiliary images of this handle whose type is `urn`.
    pub fn auxiliary(&self, urn: &str) -> std::result::Result<Vec<Handle<'_>>, Failure> {
        let n = unsafe { heif_image_handle_get_number_of_auxiliary_images(self.h, 0) };
        if n <= 0 {
            return Ok(Vec::new());
        }
        let mut ids = vec![0 as ItemId; n as usize];
        let n = unsafe {
            heif_image_handle_get_list_of_auxiliary_image_IDs(self.h, 0, ids.as_mut_ptr(), n)
        };
        let mut out = Vec::new();
        for id in ids.into_iter().take(n.max(0) as usize) {
            let mut aux = ptr::null_mut();
            check(unsafe { heif_image_handle_get_auxiliary_image_handle(self.h, id, &mut aux) })?;
            let aux = Handle {
                h: aux,
                _ctx: std::marker::PhantomData,
            };
            let mut ty: *const c_char = ptr::null();
            check(unsafe { heif_image_handle_get_auxiliary_type(aux.h, &mut ty) })?;
            let matches =
                !ty.is_null() && unsafe { CStr::from_ptr(ty) }.to_bytes() == urn.as_bytes();
            unsafe { heif_image_handle_release_auxiliary_type(aux.h, &mut ty) };
            if matches {
                out.push(aux);
            }
        }
        Ok(out)
    }

    /// Displayed size (after `irot`), like `ispe` for unrotated images.
    pub fn size(&self) -> (usize, usize) {
        unsafe {
            (
                heif_image_handle_get_width(self.h).max(0) as usize,
                heif_image_handle_get_height(self.h).max(0) as usize,
            )
        }
    }

    pub fn luma_bits(&self) -> i32 {
        unsafe { heif_image_handle_get_luma_bits_per_pixel(self.h) }
    }

    /// Metadata items (`filter` = item type such as `"Exif"` or `"mime"`),
    /// with their content type.
    pub fn metadata(&self, filter: &str) -> Vec<(String, Vec<u8>)> {
        let filter = CString::new(filter).unwrap();
        let n = unsafe { heif_image_handle_get_number_of_metadata_blocks(self.h, filter.as_ptr()) };
        if n <= 0 || n > 1024 {
            return Vec::new();
        }
        let mut ids = vec![0 as ItemId; n as usize];
        let n = unsafe {
            heif_image_handle_get_list_of_metadata_block_IDs(
                self.h,
                filter.as_ptr(),
                ids.as_mut_ptr(),
                n,
            )
        };
        let mut out = Vec::new();
        for id in ids.into_iter().take(n.max(0) as usize) {
            let size = unsafe { heif_image_handle_get_metadata_size(self.h, id) };
            if size == 0 || size > 16 << 20 {
                continue;
            }
            let mut data = vec![0u8; size];
            if check(unsafe {
                heif_image_handle_get_metadata(self.h, id, data.as_mut_ptr().cast())
            })
            .is_err()
            {
                continue;
            }
            let ct = unsafe { heif_image_handle_get_metadata_content_type(self.h, id) };
            let ct = if ct.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(ct) }.to_string_lossy().into_owned()
            };
            out.push((ct, data));
        }
        out
    }

    /// The embedded ICC profile (`colr` `prof`/`rICC`), if any.
    pub fn icc(&self) -> Option<Vec<u8>> {
        let size = unsafe { heif_image_handle_get_raw_color_profile_size(self.h) };
        if size == 0 || size > 16 << 20 {
            return None;
        }
        let mut out = vec![0u8; size];
        check(unsafe { heif_image_handle_get_raw_color_profile(self.h, out.as_mut_ptr().cast()) })
            .ok()?;
        Some(out)
    }

    /// Decode with libheif's transforms (`clap` crop, `irot`, `imir`).
    /// Callers undo `irot`/`imir` (see `coded`) so pixels end up in coded
    /// orientation with the crop applied, as ImageIO returns them.
    fn decode(&self, colorspace: c_int, chroma: c_int) -> std::result::Result<Decoded, Failure> {
        unsafe {
            let options = heif_decoding_options_alloc();
            if options.is_null() {
                return Err(Failure::Invalid("无法分配解码选项".into()));
            }
            let mut img = ptr::null_mut();
            let e = heif_decode_image(self.h, &mut img, colorspace, chroma, options);
            heif_decoding_options_free(options);
            check(e)?;
            if img.is_null() {
                return Err(Failure::Invalid("解码结果为空".into()));
            }
            Ok(Decoded(img))
        }
    }

    fn plane(
        img: &Decoded,
        channel: c_int,
        bytes_per_pixel: usize,
    ) -> std::result::Result<Plane, Failure> {
        unsafe {
            if heif_image_get_bits_per_pixel_range(img.0, channel) != 8 {
                return Err(Failure::Invalid("不是 8 位像素".into()));
            }
            let w = heif_image_get_width(img.0, channel).max(0) as usize;
            let h = heif_image_get_height(img.0, channel).max(0) as usize;
            let mut stride: c_int = 0;
            let p = heif_image_get_plane_readonly(img.0, channel, &mut stride);
            let stride = stride.max(0) as usize;
            let size = stride.checked_mul(h).filter(|&n| n <= MAX_BYTES);
            if p.is_null()
                || w == 0
                || h == 0
                || w > MAX_SIDE
                || h > MAX_SIDE
                || stride < w * bytes_per_pixel
                || size.is_none()
            {
                return Err(Failure::Invalid("图像尺寸或行跨度无效".into()));
            }
            Ok(Plane {
                width: w,
                height: h,
                stride,
                pixels: std::slice::from_raw_parts(p, size.unwrap()).to_vec(),
            })
        }
    }

    /// Full decode check, no colour conversion.
    pub fn decodes(&self) -> std::result::Result<(), Failure> {
        // 99/99 = heif_colorspace_undefined / heif_chroma_undefined.
        self.decode(99, 99).map(drop)
    }

    pub fn rgb(&self) -> std::result::Result<Plane, Failure> {
        let img = self.decode(COLORSPACE_RGB, CHROMA_INTERLEAVED_RGB)?;
        Self::plane(&img, CHANNEL_INTERLEAVED, 3)
    }

    pub fn gray(&self) -> std::result::Result<Plane, Failure> {
        let img = self.decode(COLORSPACE_MONOCHROME, CHROMA_MONOCHROME)?;
        Self::plane(&img, CHANNEL_Y, 1)
    }
}
