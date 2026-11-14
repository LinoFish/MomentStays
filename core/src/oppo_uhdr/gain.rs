//! Software-only JPEG decode / HEVC PCM encode. No imported HDR conversion code.
//! RGB samples are carried as G/B/R planes with identity matrix coefficients;
//! no chroma subsampling, transfer conversion or colour management is applied.
use super::Source;
use crate::{
    fail,
    mp4::{bx, full},
    Error, Result,
};
use oxideav_h265::encoder::{
    intra::VideoSignal,
    pcm::{encode_idr_pcm_au_wide, PcmAuOptions, PcmLayout},
};

const TILE: usize = 512;

pub(super) fn graph(source: &Source<'_>) -> Result<Vec<u8>> {
    let padded =
        source.gain_width.div_ceil(TILE) * source.gain_height.div_ceil(TILE) * TILE * TILE * 3;
    if padded.saturating_add(source.base.len()) > 240 << 20 {
        return fail("OPPO PCM 增益图与主图预计超过 240 MiB 输出上限");
    }
    let mut decoder = jpeg_decoder::Decoder::new(source.gain);
    decoder.set_max_decoding_buffer_size(256 << 20);
    let pixels = decoder
        .decode()
        .map_err(|e| Error::new(format!("OPPO 增益 JPEG 解码失败：{e}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| Error::new("OPPO 增益 JPEG 缺少尺寸"))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => 3,
        jpeg_decoder::PixelFormat::L8 => 1,
        _ => return fail("OPPO 增益 JPEG 像素格式不受支持"),
    };
    if (w, h, channels as u8) != (source.gain_width, source.gain_height, source.gain_channels)
        || pixels.len() != w * h * channels
    {
        return fail("OPPO 增益 JPEG 解码尺寸不一致");
    }
    let (cols, rows) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut samples = Vec::new();
    let mut config = None;
    for row in 0..rows {
        for col in 0..cols {
            let mut planes = [
                vec![0u16; TILE * TILE],
                vec![0u16; TILE * TILE],
                vec![0u16; TILE * TILE],
            ];
            for y in 0..TILE {
                for x in 0..TILE {
                    let at =
                        ((row * TILE + y).min(h - 1) * w + (col * TILE + x).min(w - 1)) * channels;
                    let rgb = if channels == 1 {
                        [pixels[at]; 3]
                    } else {
                        [pixels[at], pixels[at + 1], pixels[at + 2]]
                    };
                    let index = y * TILE + x;
                    for (plane, value) in planes.iter_mut().zip([rgb[1], rgb[2], rgb[0]]) {
                        plane[index] = u16::from(value);
                    }
                }
            }
            let options = PcmAuOptions {
                layout: PcmLayout {
                    chroma_format_idc: 3,
                    bit_depth: 8,
                },
                video_signal: Some(VideoSignal {
                    full_range: true,
                    colour: Some((2, 2, 0)),
                }),
                ..Default::default()
            };
            let encoded =
                encode_idr_pcm_au_wide(&planes[0], &planes[1], &planes[2], TILE, TILE, options)
                    .map_err(|e| Error::new(format!("OPPO 增益 HEVC 软件编码失败：{e:?}")))?;
            let (cfg, sample) = sample(&encoded)?;
            if config.as_ref().is_some_and(|previous| previous != &cfg) {
                return fail("增益 HEVC 分块配置不一致");
            }
            config = Some(cfg);
            samples.push(sample);
        }
    }
    container(w, h, rows, cols, &config.unwrap(), &samples)
}

// This codec's PCM entry point emits four-byte Annex B prefixes, one VPS,
// SPS, PPS, then one IDR. Refuse a changed encoder contract explicitly.
fn sample(encoded: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let starts: Vec<_> = encoded
        .windows(4)
        .enumerate()
        .filter_map(|(n, w)| (w == [0, 0, 0, 1]).then_some(n))
        .collect();
    if starts.len() != 4 || starts[0] != 0 {
        return fail("HEVC PCM NAL 布局不受支持");
    }
    let nals: Vec<_> = starts
        .iter()
        .enumerate()
        .map(|(n, &s)| &encoded[s + 4..starts.get(n + 1).copied().unwrap_or(encoded.len())])
        .collect();
    for (nal, ty) in nals.iter().zip([32, 33, 34, 20]) {
        if nal.len() < 3 || (nal[0] >> 1) & 63 != ty || nal[0] & 0x81 != 0 || nal[1] != 1 {
            return fail("HEVC PCM NAL 类型不受支持");
        }
    }
    let mut rbsp = Vec::new();
    let mut zeros = 0;
    for &v in &nals[1][2..] {
        if zeros >= 2 && v == 3 {
            zeros = 0;
            continue;
        }
        rbsp.push(v);
        zeros = if v == 0 { zeros + 1 } else { 0 };
    }
    if rbsp.len() < 13 || rbsp[0] & 15 != 1 {
        return fail("HEVC PCM SPS 时域配置无效");
    }
    let mut cfg = vec![1];
    cfg.extend(&rbsp[1..13]); // profile_tier_level, one temporal layer
    cfg.extend([0xf0, 0, 0xfc, 0xff, 0xf8, 0xf8, 0, 0, 0x0f, 3]);
    for nal in &nals[..3] {
        if nal.len() > u16::MAX as usize {
            return fail("HEVC 参数集超限");
        }
        cfg.push(0x80 | ((nal[0] >> 1) & 63));
        cfg.extend(1u16.to_be_bytes());
        cfg.extend((nal.len() as u16).to_be_bytes());
        cfg.extend(*nal);
    }
    let mut sample = (nals[3].len() as u32).to_be_bytes().to_vec();
    sample.extend(nals[3]);
    Ok((cfg, sample))
}

fn container(
    w: usize,
    h: usize,
    rows: usize,
    cols: usize,
    cfg: &[u8],
    tiles: &[Vec<u8>],
) -> Result<Vec<u8>> {
    let mut out = bx(b"ftyp", b"heix\0\0\0\0heixmif1");
    let mut grid = vec![0, 0, (rows - 1) as u8, (cols - 1) as u8];
    grid.extend((w as u16).to_be_bytes());
    grid.extend((h as u16).to_be_bytes());
    let mut payloads = vec![grid];
    payloads.extend_from_slice(tiles);
    let mut loc = vec![0x44, 0];
    loc.extend((payloads.len() as u16).to_be_bytes());
    let mut mdat = Vec::new();
    let mut iinf = (payloads.len() as u16).to_be_bytes().to_vec();
    for (n, p) in payloads.iter().enumerate() {
        let id = (n + 1) as u16;
        loc.extend(id.to_be_bytes());
        loc.extend([0, 0, 0, 1]);
        loc.extend(((out.len() + 8 + mdat.len()) as u32).to_be_bytes());
        loc.extend((p.len() as u32).to_be_bytes());
        mdat.extend(p);
        let mut info = id.to_be_bytes().to_vec();
        info.extend([0, 0]);
        info.extend(if n == 0 { b"grid" } else { b"hvc1" });
        info.extend(b"Gain\0");
        // The donor's primary grid must be visible for ImageIO to enumerate
        // and decode it. Composition hides the entire gain graph afterwards.
        iinf.extend(full(b"infe", 2, u32::from(n != 0), &info));
    }
    out.extend(bx(b"mdat", &mdat));
    let size = |w: usize, h: usize| {
        let mut p = (w as u32).to_be_bytes().to_vec();
        p.extend((h as u32).to_be_bytes());
        full(b"ispe", 0, 0, &p)
    };
    let mut properties = size(w, h);
    properties.extend(size(TILE, TILE));
    properties.extend(bx(b"hvcC", cfg));
    properties.extend(full(b"pixi", 0, 0, &[3, 8, 8, 8]));
    properties.extend(bx(b"colr", b"nclx\0\x02\0\x02\0\0\x80"));
    let mut ipma = (payloads.len() as u32).to_be_bytes().to_vec();
    ipma.extend([0, 1, 3, 1, 4, 5]);
    for n in 0..tiles.len() {
        ipma.extend(((n + 2) as u16).to_be_bytes());
        ipma.extend([4, 2, 0x83, 4, 5]);
    }
    let mut iprp = bx(b"ipco", &properties);
    iprp.extend(full(b"ipma", 0, 0, &ipma));
    let mut dimg = 1u16.to_be_bytes().to_vec();
    dimg.extend((tiles.len() as u16).to_be_bytes());
    for n in 0..tiles.len() {
        dimg.extend(((n + 2) as u16).to_be_bytes());
    }
    let mut handler = vec![0; 8];
    handler.extend(b"pict");
    handler.extend([0; 4]);
    let mut meta = bx(b"hdlr", &handler);
    meta.extend(full(b"pitm", 0, 0, &1u16.to_be_bytes()));
    meta.extend(full(b"iinf", 0, 0, &iinf));
    meta.extend(full(b"iloc", 0, 0, &loc));
    meta.extend(full(b"iref", 0, 0, &bx(b"dimg", &dimg)));
    meta.extend(bx(b"iprp", &iprp));
    out.extend(full(b"meta", 0, 0, &meta));
    crate::heif::layout(&out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires separately licensed local OPPO fixtures and Apple ImageIO"]
    fn real_gain_software_codec() {
        use crate::engine::Native;
        let root = std::path::PathBuf::from(std::env::var_os("OPPO_UHDR_FIXTURE_DIR").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("OPPO_UHDR_OUTPUT_DIR").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        for name in ["uhdr-master-v1-01.heic", "uhdr-master-v2-01.heic"] {
            let bytes = std::fs::read(root.join(name)).unwrap();
            let source = crate::oppo_uhdr::read(&bytes).unwrap().unwrap();
            let donor = super::graph(&source).unwrap();
            std::fs::write(output.join(format!("gain-{name}")), &donor).unwrap();
            assert_eq!(
                crate::platform::SystemNative.image_decodes(&donor),
                Some(true),
                "{name}"
            );
        }
    }
}
