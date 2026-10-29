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
A1. [x] Neutral model: `MotionModel` { still parts (primary, gain maps, vendor tails), video range,
       cover: Time(us) | Frame(n), source vendor, preserved vendor XMP } built by every reader.
       Done: `model.rs` built from `format::parse` for every reader (JPEG all vendors,
       HEIC Google/Samsung); HEIC writers done for Google/Samsung
       (`heifwrite.rs`, public samples verified locally 2026-10-28,
       see item 3 and `tests/SOURCES.md`).
A2. [ ] Android build: CI cross-compiles aarch64-linux-android / armv7 / x86_64 with cargo-ndk;
       `cdylib` + UniFFI (or JNI) Kotlin bindings, smoke test calling parse from Kotlin.
A3. [x] Writers: Google Motion Photo v2 (Container directory, MotionPhotoPresentationTimestampUs),
       Samsung SEF (JPEG + HEIC mpvd), Huawei/Honor LIVE_ tail, OPPO, vivo; video bytes never re-encoded.
       Done: JPEG writers for all five vendors; HEIC mpvd writer for Google/Samsung
       (`heifwrite::assemble_heic`: XMP rewrite per Swift `HEIFMetadata.write`, mpvd +
       Samsung SEF, parse-back validation of video bytes / coding hash / cover).
       Other HEIC targets are refused by design (Apple-to-Android HEIC
       targets are Samsung only per product fact). 6 tests (synthetic
       fixtures + largesize mpvd regression) + 1 CLI test; public-sample
       verification done locally 2026-10-28 (Samsung S22 mpvd + Google
       libmphoto largesize mpvd parse and writer round-trip, see
       `tests/SOURCES.md`); gallery recognition still Needs Mac check.
A4. [x] Cover conversion between time and frame index (Huawei uses frames) via the MP4 sample-table
       reader (item 7); refuse rather than guess when timing is ambiguous.
       Done: `cover.rs` (frame = presentation-sorted index, same as Swift `pair()`;
       half-duration + 2 us tolerance, ties refused, single video track required;
       raw stbl times, edit list not applied). 5 tests incl. B-frame order and
       the real H.264 file.
A5. [x] Round-trip matrix tests: every vendor → every vendor → parse back, cover/video/still coding hash equal.
       Done: JPEG matrix (10 tests in `tests/android.rs`: five vendors, Ultra HDR gain map,
       Extended XMP keys, cross-vendor chain); HEIC round-trips (Google/Samsung
       assemble + vendor CLI test, parse-back: video bytes, coding hash, cover);
       MOV-remux round-trips (MP4->MOV->MP4, per-track count/sizes/bytes/
       presentation, synthetic video+audio and real H.264 file).
3. [x] HEIF fixtures: Samsung HEIC with `mpvd` + SEF, Google HEIC, coding hash stable under Exif edits,
       iloc v0/v1/v2, idat storage. Done: `tests/heif_fixtures.rs` (9 tests,
       parameterized `build_still_spec` in `tests/common/mod.rs`): iloc v1
       8-byte widths + multi-extent mdat, iloc v2 4-byte base offset,
       iloc v1 idat-only, mixed mdat/idat, nonzero extent index
       rejected, hand-built Google mpvd and Samsung mpvd+SEF fixtures
       parsing with video bytes + timestamp, sefd-shape fixture located
       via `vendor::locate` and parsed (sefd acceptance landed
       2026-10-29, see Log), coding hash stable under Exif content /
       length / removal and XMP changes. Public samples located and
       verified locally 2026-10-28 (see `tests/SOURCES.md`); not
       committed to the repo (1.6-6.5 MB each). Full suite 121 tests
       pass, `check_rust_core.py` 16 inputs 0 mismatches.
4. [x] Scan parity: port `PhotoVendor.identify` labels and HDR classification
       (honorPrivate / ISO 21496 / legacy Apple) → `momentstays-convert scan`
       matching NativeDriver `scan` rows (relative/motion/hdr/cover/
       manufacturer/manufacturer_detail, plus detail/confirmed_static/
       source). `momentstays scan` untouched (Python CLI parity).
       13 lib tests + 2 CLI tests. Needs Mac check: row-by-row
       comparison against NativeDriver output on identical files, and
       the `CGImageSource` decode half of `confirmedStatic`
       (approximated here by header dimensions).
5. [ ] Fuzzing: cargo-fuzz targets for `parse`, `jpeg::segments`, `heif::layout`; guarantee no panics
       (replace remaining slicing that can panic on hostile input with checked access).
6. [x] `live_still` for JPEG (XMP rewrite, Apple MakerNote / Exif identifier) with parity defined on
       coding hash + semantic XMP fields. Done: `pymotion::live_still_with_identifier`
       (static-only clean per Swift `liveStill(updateIdentifier:true)`, then MakerNote /
       Exif identifier inject); parity = coding bytes unchanged + dynamic XMP fields
       cleared + MakerNote tag 17 reads back the uuid; HEIC refused. 3 tests.
       HEIC done 2026-10-29: `heifwrite::live_still_heic` (same order:
       static truncation of motion input, motion XMP strip with the
       `live_still_clean` key sets, MakerNote tag 17 inject via the same
       TIFF surgery; Exif item created with new `infe` + `cdsc` when
       absent; repack shared with `write_xmp` via a common `repack`
       helper, coding-hash guard kept). 5 tests in
       `tests/live_still_heic.rs` (strip + inject, replace keeps tag 23
       and coding, motion-input truncation, Exif creation, non-HEIC /
       multi-Exif refusal). Gallery recognition still Needs Mac check.
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
   validation. `mett_cover_time` reads the cover from the marker track's
   sample timing through the edit list (accepting Swift `mett` entries and
   native `mebx` entries identified by their still-image-time key; sample
   value NOT checked — native files write -1, Swift `movie()` writes 0).
   HEIC stills now handled by `heifwrite::live_still_heic` (see item 6).
   8 assembly tests.
9. [x] Motion Photo → Live Photo remux step (a): sample-copy MP4 -> MOV (`remux.rs`):
       chosen video track + all audio tracks, stts/stsc/stsz/ctts/stss copied verbatim,
       stco/co64 rebuilt, edit list rescaled to the 1_000_000 movie timescale, ftyp `qt  `;
       validated by parse-back `check_remux` + ffprobe (real H.264 file: pts/dts/sizes exact,
       decodes clean). 5 tests.
   [x] step (b1): movie-level content identifier
       (`com.apple.quicktime.content.identifier` in moov/udta/meta/ilst, UTF-8
       `----` item): `remux_to_mov_with_identifier` + `read_content_identifier`;
       verified by parse-back and by ffprobe format tags. 3 tests.
   [x] step (b2): still-image-time track, in the native iPhone form
       (`remux_to_mov_with_identifier_and_mett`, CLI flag
       `--cover-time-us`): handler `meta`, sample entry `mebx` with a
       `keys` box carrying `mdta/com.apple.quicktime.still-image-time`,
       one 9-byte sample (value -1), mdhd timescale 600, edit list with
       an empty edit up to the cover point. Correction to the old plan:
       no verified public sample contains a `mett` entry at all — every
       real iPhone file uses `mebx` — so the AVAssetWriter `mett` byte
       form is NOT replicated (it would be a guess); the writer copies
       the native `mebx` stsd/sample bytes verbatim from MIT-licensed
       public samples (see `tests/SOURCES.md`). Reader extended to
       accept `mebx` entries identified by their still-image-time key
       (other native `mebx` tracks — video-orientation, live-photo-info
       — are ignored). 5 tests (writer round-trip incl. cover 0 and
       negative-cover refusal, native 1-key/3-key reader forms,
       other-meta-track disambiguation) + 1 CLI test; `mett_cover_time`
       verified against the original public iPhone files (covers
       1,233,333 us and 1,260,000 us). Remaining Mac items: Photos
       recognition of the synthetic track, and comparison with Swift
       `movie()` output (see Needs Mac check).
10. [x] C ABI: `src/capi.rs` (`inspect` over FFI: version / inspect /
    free-string, panics caught at the boundary so none unwind across
    FFI), committed header `core/include/momentstays_core.h`, staticlib
    (`crate-type` already included it; `cargo build --release --lib`),
    C smoke program `tests/cabi_smoke.c` + Rust test `tests/cabi.rs`
    (builds the staticlib on demand when missing or stale, links it,
    runs version + real Motion Photo fixture + invalid bytes). 1 test.
    Swift bridge behind a flag still needs a Mac run.
11. [x] Cross-platform `convert` CLI (`momentstays-convert` binary, no Swift
    helper): `vendor` (Google/Samsung/Huawei/OPPO/vivo, xiaomi/honor as
    Google-layout aliases, Time<->Frame auto-conversion via sample
    tables; HEIC inputs for Google/Samsung targets via `heifwrite`),
    `to-live` (MOV remux + content identifier, `--cover-time-us` for the
    still-image-time track, warning only when that flag is absent),
    `to-motion` (Live pair assembly, cover from --cover-time/--cover-frame/
    --cover-from-mett). Existing `momentstays` Python-compatible binary
    untouched (its tests still pass). 7 CLI tests. Release binaries via
    CI remain open (no CI in this repo yet).

## Needs Mac check
- Parse layer (M1): verified by the owner on Mac 2026-10-28 and merged into the
  main program locally; that merge is NOT pushed yet, so remote main does not
  have it. Do not claim it until the push lands.
- MP4->MOV remux (item 9a): container validity verified on Linux (ffprobe +
  decode); needs a Mac run to confirm the MOV pairs with its still as a Live
  Photo once the mett track (item 9b) is added, and to compare against Swift
  `movie()` output sample-for-sample.
- still-image-time track (item 9b2): stsd/sample bytes are now pinned by
  MIT-licensed public samples (see `tests/SOURCES.md`) and the writer
  emits the native `mebx` form. Still needs a Mac run: confirm Photos
  recognizes the synthetic track as a Live Photo marker, and compare
  against Swift `movie()` output — whether Photos treats the native
  `mebx` form and the AVAssetWriter `mett` form equivalently is still
  unproven (no public `mett` sample exists to compare against).
- Live->Motion assembly (item 8): container-level validity verified on
  Linux (ffprobe + decode, sample-for-sample vs source); needs a Mac run
  on a real Live Photo pair (JPEG still + MOV) to confirm the output opens
  as a Motion Photo in vendor galleries and matches Swift `convert`
  byte-for-byte on the still and video portions.
- `model` writers: verify against real vendor samples (especially Samsung SEF on-device
  recognition and Huawei frame playback) and the MOV->MP4 remux path (not implemented).
- HEIC writers (A3): public samples verified locally (parse + writer
  round-trip, see `tests/SOURCES.md`); still needs a Mac run to confirm
  vendor galleries recognize the HEIC mpvd outputs (especially the
  Swift-style XMP on Samsung, whose camera files use different
  Padding/Length semantics), and the Samsung sefd form is not parsed.
- C ABI Swift bridge (item 10): the C header exists and the C smoke test
  passes on Linux; importing the header and calling it from the Swift
  app needs a Mac run.
- Scan rows (item 4): the Rust rows match Swift semantics as ported
  from source, but no Mac run has compared `momentstays-convert scan`
  output against NativeDriver `scan` rows on identical files; the
  `CGImageSource` decode half of `confirmedStatic` is approximated by
  header dimensions (see `scan.rs` docs).

## Log
- 2026-10-29: HEIC `live_still` landed (`heifwrite::live_still_heic`,
  ROADMAP item 6 HEIC half): same order as the JPEG version — motion
  input truncated to its still part, motion XMP stripped with the
  `live_still_clean` key sets, MakerNote tag 17 injected via the same
  TIFF surgery (tag 23 kept, Exif item created with new `infe` +
  `cdsc` when absent). `write_xmp` refactored onto a shared `repack`
  helper (iloc v2 rebuild, coding-hash guard); full suite confirms no
  regression. 5 tests in `tests/live_still_heic.rs`. Full suite 126
  tests pass, `check_rust_core.py` 16 inputs 0 mismatches.
- 2026-10-29: Samsung sefd-form HEIC parsing landed (`format::parse_heif`):
  top-level `sefd` (no mpvd) accepted when it runs to EOF and the SEF
  `MotionPhoto_Data` record lies inside the sefd payload; the SEF
  record is the motion declaration, so the XMP MotionPhoto==1 gate is
  skipped and XMP only supplies an optional timestamp (the real S20
  file has no mime item, timestamp None). mpvd path untouched.
  Verified on the re-fetched MIT S20 sample (SHA-256 matches
  `tests/SOURCES.md`): format `samsung-heic-sef`, video
  2749488..5755287, still 2749402 bytes, ffprobe reads a valid
  video+audio MP4. Synthetic sefd test flipped to acceptance.
  Full suite 121 tests pass, `check_rust_core.py` 16 inputs 0 mismatches.
- 2026-10-29: HEIF fixtures landed (ROADMAP item 3): `tests/heif_fixtures.rs`
  (9 tests) + parameterized `build_still_spec` / `sef_tail_mpv2` /
  `sefd_box` in `tests/common/mod.rs` (iloc v0/v1/v2, mdat/idat,
  multi-extent, base offsets, mpvd+SEF and sefd shapes, coding-hash
  stability). One test-side lesson recorded: iloc v0 has no method
  field, so idat fixtures must use v1+ (the builder now asserts this).
  Full suite 121 tests pass, `check_rust_core.py` 16 inputs 0 mismatches.
  A1/A3/A5 checkboxes corrected from [~] to [x]: HEIC writers
  (Google/Samsung, the only HEIC targets) and MOV round-trips had
  already landed; other HEIC targets remain refused by design.
- 2026-10-28: Scan parity landed (ROADMAP item 4): `src/scan.rs`
  (`identify` brand table + format fallback + HEIF Exif recursion,
  HDR chain HEIC/Ultra/Honor-private/ISO-21496/legacy-Apple/SDR,
  cover text, detail text, `confirmed_static`) + `momentstays-convert
  scan <file|dir>` emitting NativeDriver-shaped rows. `momentstays
  scan` untouched (Python CLI parity). 13 tests + 2 CLI tests. Full
  suite 112 tests pass, `check_rust_core.py` 16 inputs 0 mismatches.
- 2026-10-28: HEIC public-sample verification (ROADMAP A3 / item 3):
  Samsung S22 Ultra mpvd HEIC (MIT, sm_motion_photo) and Google
  libmphoto largesize-mpvd HEIC (Apache-2.0) parse and writer-round-trip
  clean (video bytes, coding hash, timestamp identical); Apple iPhone
  15 Pro Max still (CC0, Wikimedia Commons) passes `live_still_with_identifier`
  (tag 17 replaced, tag 23 kept, coding bytes unchanged). Samples
  registered in `tests/SOURCES.md`, not committed (size). The samples
  forced three fixes: largesize mpvd headers in `format.rs` (video
  start was hard-coded to mpvd + 8), Exif optional for HEIC motion
  photos (the Google fixture's still has none), and LONG8/IFD8 field
  sizes in `pyjpeg.rs` `maker_note` (real MakerNotes use type 16).
  Samsung sefd-form HEIC (S20) is documented as unsupported, not
  parsed. 2 regression tests. Full suite 97 tests pass,
  `check_rust_core.py` 16 inputs 0 mismatches.
- 2026-10-28: C ABI landed (ROADMAP item 10): `src/capi.rs` (`inspect`
  over FFI: version / inspect / free-string, panics caught at the
  boundary), cbindgen header `core/include/momentstays_core.h` (config
  `core/cbindgen.toml`, three-function whitelist; two remux constants
  made private after cbindgen first leaked them into the header), C
  smoke program + Rust test (staticlib built on demand when missing or
  stale). 1 test. Full suite 95 tests pass, `check_rust_core.py`
  16 inputs 0 mismatches. Swift bridge still Needs Mac check.
- 2026-10-28: MOV-remux round-trip landed (ROADMAP A5): MP4->MOV->MP4
  parse-back comparison per track (count, sizes, bytes, presentation
  times) on synthetic video+audio and the real H.264 file. 2 tests.
- 2026-10-28: Still-image-time track landed (`remux.rs`, ROADMAP item 9b2):
  native iPhone form (`mebx` + still-image-time key, 9-byte sample value -1,
  mdhd 600, empty-edit cover mapping) in `remux_to_mov_with_identifier_and_mett`
  and CLI `--cover-time-us`; `mett_cover_time` now accepts native `mebx`
  tracks (key-identified) as well as Swift `mett` tracks. 5 tests + 1 CLI
  test; reader verified against the original MIT-licensed public iPhone
  files (covers 1,233,333 us / 1,260,000 us); samples registered in
  `tests/SOURCES.md`. Full suite 92 tests pass, `check_rust_core.py`
  16 inputs 0 mismatches. Needs Mac check: Photos recognition of the
  synthetic track and comparison with Swift `movie()` output.
- 2026-10-28: `live_still` with identifier landed (`pymotion.rs`, ROADMAP
  item 6): `live_still_with_identifier` (static-only clean, then Apple
  MakerNote/Exif identifier inject per Swift `liveStill(updateIdentifier:true)`);
  parity = coding bytes unchanged + dynamic XMP fields cleared + MakerNote
  tag 17 reads back the uuid; HEIC refused. 3 tests.
- 2026-10-28: HEIC writers landed (`heifwrite.rs`, ROADMAP A3): Google/Samsung
  HEIC assembly (XMP rewrite per Swift `HEIFMetadata.write`, mpvd + Samsung
  SEF, parse-back validation); `vendor` routes HEIC inputs for Google/Samsung
  targets, other targets refused. 5 tests (synthetic fixtures) + 1 CLI test.
  Public-sample verification still pending (item 3, `tests/SOURCES.md`).
- 2026-10-28: Cross-platform convert CLI landed (`momentstays-convert`
  binary, ROADMAP item 11): `vendor` / `to-live` / `to-motion` on the Rust
  core only, no Swift helper; Time<->Frame auto-conversion via sample
  tables; `to-live` warns every run that the still-image-time marker is
  still missing (9b2). 5 CLI tests (real binary runs); existing
  `momentstays` Python-compatible behavior untouched.
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
