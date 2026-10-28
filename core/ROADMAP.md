# rust-core roadmap

Goal: a cross-platform, high-performance core (Windows/Linux/macOS CLI first, then optional
use from the Swift app via a C ABI) that produces byte-identical results to the Swift engine.

Android target (user requirement, 2026-10-27): the same core will power a future Android app that
converts between Android vendor formats (Google / Samsung / Huawei-Honor / OPPO / vivo, both ways),
so keep the core free of Apple assumptions: one neutral model (still image ranges, video range,
cover time or frame, HDR parts, vendor extras), separate readers and writers per vendor.

Performance rules (user requirement, 2026-10-27)
- Never copy a whole file: read inputs with mmap, parse borrowed `&[u8]`, return ranges/`Cow`.
- One pass per file: parse once, reuse the result for verify + extract (no re-read/re-parse).
- Hash incrementally over ranges; no temporary buffers for hashing.
- Batch work runs in parallel across files (bounded thread pool), output order stays stable.
- Every perf change lands with a number from `cargo bench`/the bench CLI, before and after.

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

## Baseline (2026-10-27, Linux sandbox, 2 vCPU, no SHA-NI; `momentstays-core bench`)
| input | parse | inspect (parse + 3 SHA-256) |
|---|---|---|
| 4+8 MB | 9.1 → 2.7 ms | 61 → 51 ms |
| 10+40 MB | 27.3 → 5.6 ms | 218 → 184 ms |
| 12+188 MB | 29.0 → 6.8 ms | 706 → 629 ms |
Inspect is now bound by software SHA-256 here; Apple Silicon / SHA-NI CPUs use hardware SHA via `sha2`.

## Next (in order)
1. [x] Fixture tests: Ultra HDR (MPF + hdrgm:Version, in primary and in gain-map XMP),
       Extended XMP split across segments (+ missing/bad GUID), Honor EHDR tail kept by static-only,
       OPPO VideoLength / MediaDataOffset, vivo marker, MicroVideoOffset legacy.
2. [x] Google recovered path: file with XMP stripped + MP4 `mett` motionphoto-image-meta track;
       high-res candidate times; conflicting descriptors rejected.
P1. [x] Bench harness: `momentstays-core bench <dir|synthetic>` timing parse/hash/extract per file
       (synthetic 10 MB / 50 MB / 200 MB Motion Photos), report MB/s; record baseline here.
P2. [~] Zero-copy I/O: mmap inputs (memmap2) DONE; remaining: `MotionPhoto.still` as `Cow` (borrow when no MPF rewrite
       or Honor tail), extract writes slices directly; streaming SHA-256 over ranges.
P3. [x] Parallel batch: `inspect`/`extract` accept many files/dirs, rayon pool (default = cores),
       JSON Lines output in input order.
P4. [~] Hot-path tuning from P1 numbers (memchr for JPEG 0xFF scan DONE: parse 3-4x faster; fewer XML parses, no per-segment Vec).
A1. [ ] Neutral model: `MotionModel` { still parts (primary, gain maps, vendor tails), video range,
       cover: Time(us) | Frame(n), source vendor, preserved vendor XMP } built by every reader.
A2. [ ] Android build: CI cross-compiles aarch64-linux-android / armv7 / x86_64 with cargo-ndk;
       `cdylib` + UniFFI (or JNI) Kotlin bindings, smoke test calling parse from Kotlin.
A3. [ ] Writers: Google Motion Photo v2 (Container directory, MotionPhotoPresentationTimestampUs),
       Samsung SEF (JPEG + HEIC mpvd), Huawei/Honor LIVE_ tail, OPPO, vivo; video bytes never re-encoded.
A4. [ ] Cover conversion between time and frame index (Huawei uses frames) via the MP4 sample-table
       reader (item 7); refuse rather than guess when timing is ambiguous.
A5. [ ] Round-trip matrix tests: every vendor → every vendor → parse back, cover/video/still coding hash equal.
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
- 2026-10-28: Python CLI port landed as `momentstays` binary (pyxml/pyjpeg/pymotion + CLI): scan/verify cross-platform, convert via the macOS Swift helper like Python. 6 parity tests vs the Python reference (parse, live_still/ultra/legacy bytes, full scan JSON); full suite 35 tests pass, `check_rust_core.py` 16 inputs 0 mismatches. Needs Mac check: `convert` end-to-end and the Swift helper cache path.
- 2026-10-27: P1 bench CLI + synthetic generator, P3 parallel `inspect -j N <file|dir>` (JSON Lines, input order), mmap inputs, memchr JPEG scan.
- 2026-10-27: items 1-2 done (22 fixture tests: Ultra HDR, Extended XMP, Honor EHDR, OPPO, vivo, legacy, recovered cover). Added performance track P1-P4 and Android track A1-A5.
- 2026-10-27: branch created, M1 landed.
