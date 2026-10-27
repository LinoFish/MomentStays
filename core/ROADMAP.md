# rust-core roadmap

Goal: a cross-platform core (Windows/Linux/macOS CLI first, then optional use from
the Swift app via a C ABI) that produces byte-identical results to the Swift engine.

Rules for every step
- Work only on branch `rust-core`; never change `main` or the Swift app.
- One item (or one coherent slice) per step. Keep Swift error texts verbatim.
- `cargo test --release` and `python3 scripts/check_rust_core.py` must pass before pushing.
- Swift parity can only run on macOS: list anything that needs it under "Needs Mac check".

## Done
- [x] M1 Parser port: binary/TIFF, JPEG segments + coding hash, MPF ranges + static-only
      (incl. Honor EHDR tail), XMP + Extended XMP (MD5), Google/legacy/OPPO/vivo/Huawei/Samsung
      location, MP4 boxes + Google cover descriptors (primary + high-res), HEIF layout,
      Samsung/Google HEIC parse, HEIF coding hash, `inspect`/`extract` CLI,
      7 vendor tests, Python parity script (16 fixtures, 0 mismatches). (2026-10-27)

## Next (in order)
1. [ ] Fixture tests: Ultra HDR (MPF + hdrgm:Version, in primary and in gain-map XMP),
       Extended XMP split across segments (+ missing/bad GUID), Honor EHDR tail kept by static-only,
       OPPO VideoLength / MediaDataOffset, vivo marker, MicroVideoOffset legacy.
2. [ ] Google recovered path: file with XMP stripped + MP4 `mett` motionphoto-image-meta track;
       high-res candidate times; conflicting descriptors rejected.
3. [ ] HEIF fixtures: Samsung HEIC with `mpvd` + SEF, Google HEIC, coding hash stable under Exif edits,
       iloc v0/v1/v2, idat storage.
4. [ ] Scan parity: port `PhotoVendor.identify` labels and HDR classification
       (honorPrivate / ISO 21496 / legacy Apple) → `scan` subcommand matching NativeDriver `scan` rows.
5. [ ] Fuzzing: cargo-fuzz targets for `parse`, `jpeg::segments`, `heif::layout`; guarantee no panics
       (replace remaining slicing that can panic on hostile input with checked access).
6. [ ] `live_still` for JPEG (XMP rewrite, Apple MakerNote / Exif identifier) with parity defined on
       coding hash + semantic XMP fields.
7. [ ] MP4/MOV sample-table reader (stts/stsz/stco/co64/ctts) and per-track sample hash matching
       Swift `trackInfo` (no AVFoundation).
8. [ ] Live Photo → Motion Photo assembly (AndroidExport) at byte level.
9. [ ] Motion Photo → Live Photo remux without re-encoding (QuickTime metadata, content identifier,
       still-image-time) — largest item; split into sub-steps when reached.
10. [ ] C ABI (`cbindgen` header), universal static lib build script, optional Swift bridge behind a flag.
11. [ ] Cross-platform `convert` CLI and release binaries via CI.

## Needs Mac check
- Run `scripts/check_rust_core.py --swift ... --samples <real photos>` once M1 lands.

## Log
- 2026-10-27: branch created, M1 landed.
