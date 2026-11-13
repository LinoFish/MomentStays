//! Conservative legacy Google multi-file bursts. A BurstID alone may describe
//! portrait variants; require the parent's strict filename protocol and EXIF Make.
use crate::{engine::failure, xmp, Error, Result};
use serde_json::{json, Value};

const CREATIONS: &str = "http://ns.google.com/photos/1.0/creations/";

fn unique(packets: &[xmp::Packet], ns: &str, name: &str) -> Result<Option<String>> {
    let values: Vec<_> = packets.iter().flat_map(|p| p.values(ns, name)).collect();
    let first = values.first().map(|s| s.trim());
    if values.iter().any(|v| Some(v.trim()) != first)
        || first.is_some_and(|v| v.is_empty() || v.len() > 256 || v.chars().any(char::is_control))
    {
        return failure(format!("Google 连拍 XMP 字段无效或冲突：{name}"));
    }
    Ok(first.map(String::from))
}

pub(super) fn validate(data: &[u8], frame: &mut Value) -> Result<()> {
    let packets = xmp::packets(data)?;
    for p in &packets {
        if ["PortraitNote", "PortraitVersion", "PortraitRequest"]
            .iter()
            .any(|name| !p.values(xmp::CAMERA, name).is_empty())
            || !p
                .values("http://ns.google.com/photos/1.0/depthmap/", "Data")
                .is_empty()
            || p.values(xmp::CAMERA, "SpecialTypeID")
                .iter()
                .any(|v| v.to_ascii_uppercase().contains("PORTRAIT"))
        {
            return failure("Google 人像或景深组合尚未验证，未作为普通连拍转换");
        }
    }
    let group = unique(&packets, xmp::CAMERA, "BurstID")?;
    let creation = unique(&packets, CREATIONS, "CameraBurstID")?;
    if creation.is_some() && creation != group {
        return failure("Google BurstID 与 CameraBurstID 不一致或缺失");
    }
    // Filename-only legacy groups are permitted. Never use an arbitrary XMP ID
    // as a path, group boundary, frame number, or proof of total frame count.
    frame["google_group"] = json!(group);
    if let Some(primary) = unique(&packets, xmp::CAMERA, "BurstPrimary")? {
        let cover = match primary.as_str() {
            "1" | "True" | "true" => true,
            "0" | "False" | "false" => false,
            _ => return Err(Error::new("Google BurstPrimary 取值尚未支持")),
        };
        if frame["source_cover"] == true && !cover {
            return failure("Google 文件名 COVER 与 BurstPrimary 冲突");
        }
        frame["source_cover"] = json!(cover);
    }
    Ok(())
}
