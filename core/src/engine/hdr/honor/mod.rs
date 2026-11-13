//! Honor resource decoding and reversible Ultra HDR packaging.
//! The empirical formula and capture gates are isolated in `profile`.
pub mod profile;
use crate::engine::{failure, Native};
use crate::{hdr as framing, jpeg, tiff, Error, Result};
pub use profile::{map_luma, OFFSET, PROFILE, STOPS};

fn unsupported<T>(reason: &str) -> Result<T> {
    failure(format!("荣耀 EHDR 实验转换未执行：{reason}"))
}

/// Read SOF dimensions before asking the native decoder to allocate pixels.
fn frame(data: &[u8]) -> Result<(usize, usize, u8)> {
    let segments = jpeg::segments(data)?;
    let frames: Vec<_> = segments
        .iter()
        .filter(|s| matches!(s.marker, 0xc0 | 0xc2))
        .collect();
    if frames.len() != 1 {
        return unsupported("JPEG 帧结构不受支持");
    }
    let p = frames[0].payload(data);
    if p.len() < 6 || p[0] != 8 || p.len() != 6 + 3 * p[5] as usize {
        return unsupported("JPEG 位深或分量结构不受支持");
    }
    Ok((
        u16::from_be_bytes([p[3], p[4]]) as usize,
        u16::from_be_bytes([p[1], p[2]]) as usize,
        p[5],
    ))
}

fn ascii(t: &tiff::Tiff<'_>, fields: &[tiff::Field], tag: usize) -> Result<String> {
    let f = tiff::find(fields, tag).ok_or_else(|| Error::new("缺少荣耀设备信息"))?;
    if f.ty != 2 || f.count == 0 || f.count > 64 {
        return failure("荣耀设备信息类型无效");
    }
    let bytes = if f.count <= 4 {
        &f.value[..f.count]
    } else {
        let at = f.offset(t.little)?;
        t.data
            .get(at..at + f.count)
            .ok_or_else(|| Error::new("荣耀设备信息越界"))?
    };
    Ok(std::str::from_utf8(bytes)
        .map_err(|_| Error::new("荣耀设备信息编码无效"))?
        .trim_end_matches('\0')
        .to_string())
}

fn supported_camera(data: &[u8]) -> Result<bool> {
    let segs = jpeg::segments(data)?;
    // The calibrated source declares sRGB in EXIF and has no ICC profile.
    // Do not silently apply that colour assumption to another profile.
    if segs
        .iter()
        .any(|s| s.marker == 0xe2 && s.payload(data).starts_with(b"ICC_PROFILE\0"))
    {
        return Ok(false);
    }
    let exifs: Vec<_> = segs
        .iter()
        .filter(|s| s.marker == 0xe1 && s.payload(data).starts_with(jpeg::EXIF))
        .collect();
    if exifs.len() != 1 {
        return Ok(false);
    }
    let t = tiff::Tiff::new(&exifs[0].payload(data)[6..])?;
    let (root, _) = t.root()?;
    if ascii(&t, &root, 0x10f)? != "HONOR" || ascii(&t, &root, 0x110)? != "ELP-AN00" {
        return Ok(false);
    }
    let Some(ptr) = tiff::find(&root, 0x8769) else {
        return Ok(false);
    };
    let (exif, _) = t.directory(ptr.offset(t.little)?)?;
    let srgb = tiff::find(&exif, 0xa001).is_some_and(|f| {
        f.ty == 3
            && f.count == 1
            && if t.little {
                u16::from_le_bytes([f.value[0], f.value[1]]) == 1
            } else {
                u16::from_be_bytes([f.value[0], f.value[1]]) == 1
            }
    });
    Ok(srgb
        && segs.iter().any(|s| {
            s.marker == 0xe9
                && s.payload(data)
                    .windows(profile::FIRMWARE.len())
                    .any(|w| w == profile::FIRMWARE)
        }))
}

/// No reference data at runtime. A refusal leaves preservation to the caller.
pub fn convert(native: &dyn Native, image: &[u8]) -> Result<Vec<u8>> {
    let end = jpeg::image_end(image)?;
    let Some(honor) = framing::honor_ehdr(image, end)? else {
        return unsupported("未找到 EHDR 扩展");
    };
    if honor.luma.is_empty() {
        return unsupported("原片不含亮度 JPEG");
    }
    if jpeg::image_ranges(image)?.len() != 1 {
        return unsupported("已存在辅助图，不对其叠加私有 HDR 重建");
    }
    if !supported_camera(image)? {
        return unsupported("机型、固件或色彩声明不属于当前标定配置");
    }
    if !profile::supports_header(honor.header) {
        return unsupported("EHDR 参数头未命中四组标定配置（含 ISO/曝光字段）；无法确认私有亮度图对应的标准增益参数，未直接封装为 Ultra HDR");
    }
    let (width, height, channels) = frame(image)?;
    if channels != 3
        || !profile::supports_dimensions(width, height)
        || frame(honor.luma)? != (width, height, 1)
    {
        return unsupported("主图与亮度图尺寸不属于当前标定配置");
    }
    let Some(luma) = native.decode_gray_jpeg(honor.luma)? else {
        return unsupported("无法读取单通道亮度图");
    };
    let Some(base) = native.decode_srgb_jpeg(&image[..end])? else {
        return unsupported("无法读取 sRGB 主图像素");
    };
    if (luma.width, luma.height) != (width, height) || (base.width, base.height) != (width, height)
    {
        return unsupported("主图或亮度图解码尺寸不一致");
    }
    let mapped = map_luma(&luma, &base)?;
    let gain = native.encode_gray_jpeg(&mapped, 1.0)?;
    if frame(&gain)? != (width / 4, height / 4, 1) || native.image_decodes(&gain) == Some(false) {
        return failure("荣耀 EHDR 增益图编码校验失败");
    }
    let tagged = super::color_profile(image, &native.srgb_icc()?)?;
    let mut output = super::ultra_with_offset(&tagged, &gain, STOPS, OFFSET)?;
    output.extend_from_slice(honor.extension);
    if jpeg::coding_hash(&output)? != jpeg::coding_hash(image)?
        || framing::honor_private(&output, jpeg::image_end(&output)?)? != Some(honor.extension)
        || jpeg::image_ranges(&output)?.len() != 2
        || jpeg::static_only(&output)? != output
        || native.image_decodes(&output) == Some(false)
    {
        return failure("荣耀 EHDR 转换后主图或私有扩展校验失败");
    }
    Ok(output)
}
