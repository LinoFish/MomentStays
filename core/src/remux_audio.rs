//! AAC sample-description adaptation. Compressed samples and ESDS bytes stay intact.
use super::*;

fn unsupported<T>() -> Result<T> {
    fail_as(ErrorKind::Unsupported, "AAC 音频描述不受支持，未丢弃音轨或重新编码")
}

fn descriptor(data: &[u8], tag: u8) -> Result<&[u8]> {
    let mut at = 0;
    let mut found = None;
    while at < data.len() {
        let kind = data[at];
        at += 1;
        let mut size = 0usize;
        let mut ended = false;
        for _ in 0..4 {
            let Some(&b) = data.get(at) else { return unsupported() };
            at += 1;
            size = (size << 7) | (b & 127) as usize;
            if b & 128 == 0 { ended = true; break; }
        }
        if !ended || size > data.len() - at { return unsupported(); }
        if kind == tag {
            if found.is_some() { return unsupported(); }
            found = Some(&data[at..at + size]);
        }
        at += size;
    }
    found.ok_or_else(|| crate::Error::new("AAC 缺少解码配置"))
}

fn packet_frames(esds: &[u8]) -> Result<u32> {
    let es = descriptor(esds.get(4..).ok_or_else(|| crate::Error::new("esds 被截断"))?, 3)?;
    if es.len() < 3 { return unsupported(); }
    let flags = es[2];
    let mut at = 3 + usize::from(flags & 128 != 0) * 2;
    if flags & 64 != 0 {
        let Some(&len) = es.get(at) else { return unsupported() };
        at += 1 + len as usize;
    }
    at += usize::from(flags & 32 != 0) * 2;
    let config = descriptor(es.get(at..).ok_or_else(|| crate::Error::new("ES 描述被截断"))?, 4)?;
    if config.len() < 13 || config[0] != 0x40 || config[1] >> 2 != 5 { return unsupported(); }
    let asc = descriptor(&config[13..], 5)?;
    let mut bit = 0usize;
    let mut read = |n: usize| -> Result<u32> {
        if bit + n > asc.len() * 8 { return unsupported(); }
        let mut v = 0;
        for i in bit..bit + n { v = (v << 1) | ((asc[i / 8] >> (7 - i % 8)) & 1) as u32; }
        bit += n;
        Ok(v)
    };
    // MPEG-4 GA AAC Main/LC/SSR/LTP: use the original frameLengthFlag,
    // never infer packet size from a possibly trimmed last sample duration.
    if !matches!(read(5)?, 1..=4) { return unsupported(); }
    let frequency = read(4)?;
    if frequency == 15 { let _ = read(24)?; } else if frequency > 12 { return unsupported(); }
    let channels = read(4)?;
    let frames = if read(1)? == 0 { 1024 } else { 960 };
    if channels == 0 || channels > 7 || read(1)? != 0 || read(1)? != 0 {
        return unsupported();
    }
    // A sync extension can change the output frame size (implicit SBR).
    // Preserve and refuse such an unimplemented configuration, never guess.
    while bit < asc.len() * 8 {
        if ((asc[bit / 8] >> (7 - bit % 8)) & 1) != 0 { return unsupported(); }
        bit += 1;
    }
    Ok(frames)
}

fn children_at(raw: &[u8]) -> Result<usize> {
    if raw.len() < 36 { return unsupported(); }
    match crate::binary::be(raw, 16, 2)? {
        0 => Ok(36),
        1 if raw.len() >= 52 => Ok(52),
        _ => unsupported(),
    }
}

fn quicktime_entry(raw: &[u8]) -> Result<Vec<u8>> {
    if raw.get(4..8) != Some(b"mp4a") { return Ok(raw.to_vec()); }
    let at = children_at(raw)?;
    let children = mp4::boxes(raw, at, raw.len())?;
    let waves: Vec<_> = children.iter().filter(|b| b.is(b"wave")).collect();
    let configs: Vec<_> = children.iter().filter(|b| b.is(b"esds")).collect();
    if !waves.is_empty() {
        if waves.len() != 1 || !configs.is_empty() || at != 52 { return unsupported(); }
        return Ok(raw.to_vec()); // Already a QuickTime sound description.
    }
    if configs.len() != 1 || at != 36 { return unsupported(); }
    let config = configs[0];
    let frames = packet_frames(&raw[config.payload..config.end])?;
    let mut body = raw[8..36].to_vec();
    body[8..10].copy_from_slice(&1u16.to_be_bytes());
    body[20..22].copy_from_slice(&(-2i16).to_be_bytes());
    body.extend(frames.to_be_bytes());
    body.extend([0; 8]); // variable bytes per packet/frame
    body.extend(2u32.to_be_bytes());
    let mut wave = bx(b"frma", b"mp4a");
    wave.extend(bx(b"mp4a", &[0; 4]));
    wave.extend_from_slice(&raw[config.start..config.end]);
    wave.extend(bx(&[0; 4], &[]));
    body.extend(bx(b"wave", &wave));
    for c in children.iter().filter(|c| !c.is(b"esds")) {
        body.extend_from_slice(&raw[c.start..c.end]);
    }
    Ok(bx(b"mp4a", &body))
}

pub(super) fn quicktime_descriptions(table: &[u8]) -> Result<Vec<u8>> {
    let entries = mp4::boxes(table, 16, table.len())?;
    if crate::binary::be(table, 12, 4)? != entries.len() { return unsupported(); }
    let mut body = u32b(entries.len() as u32).to_vec();
    for e in entries { body.extend(quicktime_entry(&table[e.start..e.end])?); }
    Ok(full(b"stsd", 0, 0, &body))
}

pub(super) fn iso_description(raw: &[u8]) -> Result<Vec<u8>> {
    let at = children_at(raw)?;
    if at == 36 { return Ok(raw.to_vec()); }
    let children = mp4::boxes(raw, at, raw.len())?;
    let waves: Vec<_> = children.iter().filter(|c| c.is(b"wave")).collect();
    if waves.is_empty() { return Ok(raw.to_vec()); }
    if waves.len() != 1 { return unsupported(); }
    let wave = waves[0];
    let items = mp4::boxes(raw, wave.payload, wave.end)?;
    let configs: Vec<_> = items.iter().filter(|c| c.is(b"esds")).collect();
    if configs.len() != 1 || items.iter().any(|c| ![b"frma", b"mp4a", b"esds", &[0;4]].iter().any(|t| c.is(t))) { return unsupported(); }
    let mut body = raw[8..36].to_vec();
    body[8..10].fill(0);
    body[20..24].fill(0);
    body.extend_from_slice(&raw[configs[0].start..configs[0].end]);
    for c in children.iter().filter(|c| !c.is(b"wave")) { body.extend_from_slice(&raw[c.start..c.end]); }
    Ok(bx(b"mp4a", &body))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(asc: [u8; 2]) -> Vec<u8> {
        let mut es = vec![3,25,0,0,0,4,17,0x40,0x15,0,3,0,0,3,0xe8,0,0,3,0xe8,0,5,2];
        es.extend(asc);
        es.extend([6,1,2]);
        let mut p = vec![0; 28];
        p[7] = 1;
        p[17] = 2;
        p[19] = 16;
        p[24..].copy_from_slice(&(48000u32 << 16).to_be_bytes());
        p.extend(full(b"esds", 0, 0, &es));
        bx(b"mp4a", &p)
    }
    #[test]
    fn aac_description_roundtrip_retains_original_decoder_configuration() {
        for (asc, frames) in [([0x11,0x90],1024u32),([0x11,0x94],960)] {
            let original = entry(asc);
            let qt = quicktime_entry(&original).unwrap();
            assert_eq!(&qt[16..18], &1u16.to_be_bytes());
            assert_eq!(&qt[36..40], &frames.to_be_bytes());
            assert_eq!(quicktime_entry(&qt).unwrap(), qt);
            assert_eq!(iso_description(&qt).unwrap(), original);
        }
    }
    #[test]
    fn malformed_or_unknown_aac_configuration_is_not_silently_copied() {
        let original = entry([0x11,0x90]);
        for n in 8..original.len() { assert!(quicktime_entry(&original[..n]).is_err()); }
        assert!(quicktime_entry(&entry([0xf9,0x90])).is_err());
        let mut qt = quicktime_entry(&original).unwrap();
        let at = qt.windows(4).position(|w| w == b"frma").unwrap();
        qt[at..at + 4].copy_from_slice(b"oops");
        assert!(iso_description(&qt).is_err());
    }
    #[test]
    fn non_aac_sound_description_stays_byte_identical() {
        let raw = bx(b"lpcm", &[0; 64]);
        assert_eq!(quicktime_entry(&raw).unwrap(), raw);
    }
}
