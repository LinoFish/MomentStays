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
A1. [~] Neutral model: `MotionModel` { still parts (primary, gain maps, vendor tails), video range,
       cover: Time(us) | Frame(n), source vendor, preserved vendor XMP } built by every reader.
       JPEG path done (`model.rs`, built from `format::parse`); HEIC writers pending.
A2. [ ] Android build: CI cross-compiles aarch64-linux-android / armv7 / x86_64 with cargo-ndk;
       `cdylib` + UniFFI (or JNI) Kotlin bindings, smoke test calling parse from Kotlin.
A3. [~] Writers: Google Motion Photo v2 (Container directory, MotionPhotoPresentationTimestampUs),
       Samsung SEF (JPEG + HEIC mpvd), Huawei/Honor LIVE_ tail, OPPO, vivo; video bytes never re-encoded.
       JPEG writers done (all five vendors); HEIC mpvd writer pending.
A4. [x] Cover conversion between time and frame index (Huawei uses frames) via the MP4 sample-table
       reader (item 7); refuse rather than guess when timing is ambiguous.
       Done: `cover.rs` (frame = presentation-sorted index, same as Swift `pair()`;
       half-duration + 2 us tolerance, ties refused, single video track required;
       raw stbl times, edit list not applied). 5 tests incl. B-frame order and
       the real H.264 file.
A5. [~] Round-trip matrix tests: every vendor → every vendor → parse back, cover/video/still coding hash equal.
       JPEG matrix done (10 tests in `tests/android.rs`: five vendors, Ultra HDR gain map,
       Extended XMP keys, cross-vendor chain); HEIC and MOV-remux round-trips pending.
3. [ ] HEIF fixtures: Samsung HEIC with `mpvd` + SEF, Google HEIC, coding hash stable under Exif edits,
       iloc v0/v1/v2, idat storage.
4. [ ] Scan parity: port `PhotoVendor.identify` labels and HDR classification
       (honorPrivate / ISO 21496 / legacy Apple) → `scan` subcommand matching NativeDriver `scan` rows.
5. [ ] Fuzzing: cargo-fuzz targets for `parse`, `jpeg::segments`, `heif::layout`; guarantee no panics
       (replace remaining slicing that can panic on hostile input with checked access).
6. [ ] `live_still` for JPEG (XMP rewrite, Apple MakerNote / Exif identifier) with parity defined on
       coding hash + semantic XMP fields.
7. [x] MP4/MOV sample-table reader (stts/stsz/stco/co64/ctts) and per-track sample hash matching
       Swift `trackInfo` (no AVFoundation). Reader done: `mp4::sample_tables` (per-sample
       offset/size/decode/presentation/sync, stss + ctts v0/v1); 6 tests incl. a real
       H.264 file cross-checked against ffprobe (positions/sizes/sync exact; times
       match up to the uniform edit-list shift). Per-track sample hash matching
       against Swift `trackInfo` still needs a Mac run.
8. [x] Live Photo → Motion Photo assembly (AndroidExport) at byte level
   (`live2motion.rs`): MOV->MP4 sample-copy remux (`remux_to_mp4`, exactly
   one video + at most one audio track like Swift, mett dropped, ftyp
   `mp42`, edit list copied), JPEG still cleaning
   (`pymotion::live_still_clean`: Swift `liveStill(uuid:"", updateIdentifier:
   false)` — static-only truncation, motion XMP strip for GCamera + OPPO/
   vivo keys and MotionPhoto directory items, EXIF untouched, gain-map
   bytes verified), then existing `model::write` with its parse-back
   validation. `mett_cover_time` reads the cover from the mett track's
   sample timing through the edit list (sample value NOT checked: same
   open question as 9b2). HEIC stills refused (need HEIC writers). 8 tests.
9. [x] Motion Photo → Live Photo remux step (a): sample-copy MP4 -> MOV (`remux.rs`):
       chosen video track + all audio tracks, stts/stsc/stsz/ctts/stss copied verbatim,
       stco/co64 rebuilt, edit list rescaled to the 1_000_000 movie timescale, ftyp `qt  `;
       validated by parse-back `check_remux` + ffprobe (real H.264 file: pts/dts/sizes exact,
       decodes clean). 5 tests.
   [x] step (b1): movie-level content identifier
       (`com.apple.quicktime.content.identifier` in moov/udta/meta/ilst, UTF-8
       `----` item): `remux_to_mov_with_identifier` + `read_content_identifier`;
       verified by parse-back and by ffprobe format tags. 3 tests.
   [ ] step (b2): still-image-time mett track. NOT done — the boxed sample
       bytes are not pinned by any verifiable source and were NOT guessed.
       Known from Swift `movie()`: format description identifier
       `mdta/com.apple.quicktime.still-image-time`, data type
       `com.apple.metadata.datatype.int8`, value 0, mediaTimeScale 1_000_000,
       one group at frameTime/frameDuration; reader requires exactly one
       marker with value in {-1, 0, 255}. Unknown: the exact sample bytes
       AVAssetWriter writes for a boxed int8 item, and the mett stsd entry
       children. Closing step (needs a Mac): run Swift `movie()` on a sample,
       hex-dump the mett track's stsd entry and mdat sample, then encode
       those bytes. Until then the remux output is a valid MOV with a content
       identifier but not yet a Live Photo.
10. [ ] C ABI (`cbindgen` header), universal static lib build script, optional Swift bridge behind a flag.
11. [ ] Cross-platform `convert` CLI and release binaries via CI.

## Needs Mac check
- Parse layer (M1): verified by the owner on Mac 2026-10-28 and merged into the
  main program locally; that merge is NOT pushed yet, so remote main does not
  have it. Do not claim it until the push lands.
- MP4->MOV remux (item 9a): container validity verified on Linux (ffprobe +
  decode); needs a Mac run to confirm the MOV pairs with its still as a Live
  Photo once the mett track (item 9b) is added, and to compare against Swift
  `movie()` output sample-for-sample.
- still-image-time mett track (item 9b2): needs a Mac run of Swift `movie()`
  on a sample file, then a hex dump of the mett track's stsd entry and mdat
  sample — those bytes are the ground truth for the Rust encoder. Do not
  guess them; the 2012 QuickTime spec predates mett tracks and ffmpeg's
  demuxer does not parse still-image-time samples.
- Live->Motion assembly (item 8): container-level validity verified on
  Linux (ffprobe + decode, sample-for-sample vs source); needs a Mac run
  on a real Live Photo pair (JPEG still + MOV) to confirm the output opens
  as a Motion Photo in vendor galleries and matches Swift `convert`
  byte-for-byte on the still and video portions.
- `model` writers: verify against real vendor samples (especially Samsung SEF on-device
  recognition and Huawei frame playback) and the MOV->MP4 remux path (not implemented).

## Log
- 2026-10-28: Live->Motion assembly landed (`live2motion.rs`, ROADMAP
  item 8): `remux_to_mp4` (strict 1-video + <=1-audio layout, mett dropped,
  ftyp `mp42`), `live_still_clean` (Swift `liveStill` with
  updateIdentifier:false: static-only truncation, GCamera + OPPO/vivo
  motion XMP strip, EXIF untouched, gain-map check; HEIC refused), then
  `model::write`; plus `mett_cover_time` (edit-list-mapped mett timing,
  value unchecked per 9b2). 8 tests (synthetic MOV incl. elst-mapped mett
  cover, hand-built motion-XMP JPEG cleaning, Google + Huawei end-to-end
  assembly); ffprobe on the real-file MP4 shows pts/dts/sizes identical
  to source and it decodes cleanly.
- 2026-10-28: Content identifier landed (`remux.rs`, ROADMAP item 9b first
  half): `remux_to_mov_with_identifier` writes moov/udta/meta/ilst with a
  UTF-8 `----` item (mean `com.apple.quicktime.metadata`), `read_content_
  identifier` reads it back; 3 tests. ffprobe independently reports the tag
  (`TAG:com.apple.quicktime.content.identifier=<uuid>`). The still-image-
  time mett track (9b second half) is deliberately NOT written: its boxed
  sample bytes are not pinned by any verifiable source (QTFF 2012 predates
  mett tracks, ffmpeg does not parse still-image-time samples, web searches
  surfaced no byte-level reference) and were not guessed; the closing step
  is a Mac hex dump of Swift `movie()` output (see Needs Mac check).
- 2026-10-28: MP4->MOV sample-copy remux landed (`remux.rs`, ROADMAP item 9a):
  chosen video track + all audio tracks, verbatim sample bytes and timing tables,
  rebuilt chunk offsets, edit list rescaled to the 1_000_000 movie timescale;
  negative presentation times refused. 5 tests (synthetic video+audio, mett
  drop, negative-PTS refusal, second video selection, real H.264 file); ffprobe
  on the real-file output shows pts/dts/sizes identical to source and it
  decodes cleanly. Item 9b (content identifier + still-image-time mett track)
  remains: without it the output is a valid MOV but not yet a Live Photo.
- 2026-10-28: Cover time<->frame conversion landed (`cover.rs`, ROADMAP A4): frame is the
  presentation-sorted index (Swift `pair()` convention), half-duration + 2 us tolerance,
  ties/out-of-range refused; 5 tests (B-frame order, real H.264 file, strict video-track
  selection). Carries the owner's Mac parse-layer verification note (see Needs Mac check).
- 2026-10-28: MP4/MOV sample-table reader landed (`mp4::sample_tables`, ROADMAP item 7):
  stts/stsz/stsc/stco/co64/ctts/stss enumeration with strict count/bounds checks; 6 tests
  (synthetic tracks + real ffmpeg H.264 file cross-checked against ffprobe). This unblocks
  A4 (cover time<->frame) and item 9 (remux). Pushed as a1c20813.
- 2026-10-28: Android writers landed (`model.rs`): neutral `MotionModel` from `format::parse`,
  JPEG writers for Google/Samsung SEF/Huawei LIVE_/OPPO/vivo with parse-back validation
  (video bytes, still coding hash, cover); Time<->Frame conversion refused, not guessed.
  10 round-trip tests pass (five vendors, Ultra HDR gain map, Extended XMP keys,
  cross-vendor chain); full suite 45 tests pass, `check_rust_core.py` 16 inputs 0 mismatches.
  Found and fixed while testing: SEF mpv2 pointer/length are big-endian while the rest
  of the SEF directory is little-endian. Pushed as fabf38dc.
- 2026-10-28: Python CLI port landed as `momentstays` binary (pyxml/pyjpeg/pymotion + CLI): scan/verify cross-platform, convert via the macOS Swift helper like Python. 6 parity tests vs the Python reference (parse, live_still/ultra/legacy bytes, full scan JSON); full suite 35 tests pass, `check_rust_core.py` 16 inputs 0 mismatches. Needs Mac check: `convert` end-to-end and the Swift helper cache path.
- 2026-10-27: P1 bench CLI + synthetic generator, P3 parallel `inspect -j N <file|dir>` (JSON Lines, input order), mmap inputs, memchr JPEG scan.
- 2026-10-27: items 1-2 done (22 fixture tests: Ultra HDR, Extended XMP, Honor EHDR, OPPO, vivo, legacy, recovered cover). Added performance track P1-P4 and Android track A1-A5.
- 2026-10-27: branch created, M1 landed.
