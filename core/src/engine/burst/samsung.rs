//! Experimental legacy Samsung filenames plus bounded SEF burst evidence.
//! SEF field names are documented by ExifTool's Samsung.pm. Payloads remain
//! opaque evidence: neither group boundaries nor best-frame semantics are guessed.
use crate::{engine::failure, jpeg, scan, sha256_hex, vendor, Error, Result};
use serde_json::{json, Value};

pub(super) fn filename(name: &str) -> Option<Value> {
    let upper = name.to_ascii_uppercase();
    let (stem, ext) = upper.rsplit_once('.')?;
    if !matches!(ext, "JPG" | "JPEG") {
        return None;
    }
    let (label, index) = stem.rsplit_once('_')?;
    let mut frame = super::filename(&format!("IMG_{label}_BURST{index}.JPG"), "HONOR")?;
    frame["source_group"] = json!(label);
    frame["evidence"] = json!("samsung-filename-sef-v1");
    frame["source_format"] = json!("samsung-burst");
    Some(frame)
}

/// Only digit-text records are admitted initially. Unknown binary payloads may
/// contain absolute pointers and cannot safely be relocated after an EXIF edit.
pub(super) fn validate(data: &[u8], frame: &mut Value) -> Result<()> {
    let ranges = jpeg::image_ranges(data)?;
    let image_end = ranges
        .last()
        .ok_or_else(|| Error::new("三星 JPEG 图像缺失"))?
        .end;
    if !scan::confirmed_static(&data[..image_end]) {
        return failure("三星连拍实况或非静态组合尚未支持");
    }
    let sef = vendor::sef_records(data, image_end)?
        .ok_or_else(|| Error::new("三星序号文件缺少 SEF 连拍证据，未自动分组转换"))?;
    if !matches!(sef.version, 106 | 107) {
        return failure("三星 SEF 版本尚未支持，未猜测新版本");
    }
    let mut cursor = image_end;
    let mut records: Vec<_> = sef.records.iter().collect();
    records.sort_by_key(|r| r.start);
    let mut burst = None;
    let mut seen = std::collections::HashSet::new();
    for record in records {
        if record.start != cursor {
            return failure("三星 SEF 含未识别的间隙或额外资源");
        }
        cursor = record.end;
        let expected: &[u8] = match record.marker {
            0x09e0 => b"Burst_Shot_Info",
            0x09e1 => b"BurstShot_Best_Photo_Info",
            0x0a01 => b"Image_UTC_Data",
            0x0c61 => b"Camera_Capture_Mode_Info",
            _ => return failure("三星连拍包含尚未支持的 SEF 条目，原片未改写"),
        };
        if data[record.start..record.start + 2] != [0, 0]
            || record.name != expected
            || !seen.insert(record.marker)
            || record.data.is_empty()
            || record.data.len() > 32
            || !record.data.iter().all(u8::is_ascii_digit)
        {
            return failure("三星 SEF 连拍名称、重复条目或数据编码尚未支持");
        }
        if record.marker == 0x09e0 {
            burst = Some(sha256_hex(record.data));
        }
        // Best_Photo is preserved; choose a representative explicitly in the UI.
    }
    if cursor != sef.head || burst.is_none() {
        return failure("三星 SEF 缺少唯一连拍条目或包含额外资源");
    }
    frame["vendor_group"] = json!(burst);
    Ok(())
}
