# Rust batch engine (`core/src/engine`)

The Rust replacement for the Swift engine in `apple/shared`. It keeps the
Swift engine's request keys, option JSON, manifest and report keys,
`implementation` revisions, output names and user-facing texts, so results
written by the Swift app are resumed, verified and delivered unchanged.

Byte and container work runs in Rust. Pixel and codec work goes through
`engine::Native`, which the app implements with ImageIO/AVFoundation.

## Swift → Rust map

| Swift | Rust |
|---|---|
| `ConversionOptions`, `AndroidOptions` (+ `json`) | `engine::options` |
| `NativeBatch.files/selection/relative/validateOutput/normalized/root` | `engine::paths` |
| `ScanRow`, `SourceVersion`, `NativeBatch.scanFile` | `engine::scanrow` |
| `NativeBatch.verify/publish/restoreNames/convert/run/scan`, `BatchScheduling` | `engine::batch` |
| `MotionLive.movie`, `exportAuxiliary`, `PhotoFormat.repairAuxiliaryDuration` | `engine::movie` |
| `PhotoFormat.liveStill`, `PhotoHDR.upgradeISO/colorProfile/ultra/packGainMap`, `legacyGain` math | `engine::still` |
| `CoverCandidateSession` | `engine::candidates` (frame decode via `Native::frame_jpeg`) |
| `AndroidExport`, `AndroidHDR.prepare` | `engine::android` |
| `AndroidTransfer` | `engine::transfer` |
| `NativeDriver.swift` modes | `engine::request` (+ `momentstays-driver`) |

Revisions written: `gui-native-1.1.5` (to Apple), `reverse-native-1.2.1`
(to Android), `google-to-honor-a-modern-1` (Google → Honor). Resume accepts
the same earlier revisions as Swift.

Besides the NativeDriver modes (`format`, `scan`, `batch`,
`android-assemble`, `android-still`, `cover-candidates`), the request layer
has the calls the UI makes outside a batch: `scan-file` (one row, with the
version used to reuse it in `batch` via `scan_rows`), `android-pair`
(Photos import, preview), `preview-cover` (cover frame and candidates for
the motion preview, replacing `sourceCoverTiming` there), `still-check`
(the import guard on stills), plus `movie` and `export-auxiliary`.

## Platform backend and host events

`platform::SystemNative` implements ImageIO and AVFoundation directly in Rust
on Apple: complete image checks, sRGB ICC, gray JPEG decode/encode, JPEG
re-encoding with source properties, HEIC gain-map reading (the existing
`heic_gain` module), and transformed video-frame JPEG extraction. Core
container, timing, HDR decisions and batch work remain platform independent.
Other platforms explicitly refuse codec operations until a backend is supplied.
The `Native` trait and optional `MsNative` codec callbacks allow another backend
without changing the engine. The macOS app supplies **no codec callbacks**.

Swift supplies only progress, existing-output decisions and scan-row delivery.
Callback contexts stay alive until the synchronous engine request has returned;
cancellation uses an owned `ms_stop_*` token. Image/video codec objects and
Objective-C autorelease pools belong to each Rust invocation.

Production macOS calls `ms_core_engine` for batch/convert, all scan projections,
Apple pairing, preview cover resolution, candidate preparation/export and the
Photos still-data guard. The UI retains its incremental source/version cache,
preview rendering, settings, Photos/system sharing services and report model.
`MOTIONLIVE_SWIFT_REFERENCE` retains the prior engine for differential checks.

## C ABI

```c
void *ms_core_engine(const MsNative *native, const uint8_t *request, size_t len, void *token);
void *ms_stop_new(void); void ms_stop_cancel(const void *); void ms_stop_free(void *);
void  ms_buffer_append(void *buffer, const uint8_t *data, size_t len);
```

The request is the driver JSON (`{"mode":"batch", …}`); the reply is read
with `ms_core_bytes(r, 0, &len)` and released with `ms_core_free`.
Declarations: `apple/shared/RustCore.h`.

## Comparing with the Swift engine (macOS)

```sh
apple/macos/scripts/build_gui_tests.sh
apple/macos/scripts/run_checks_with_rust.sh
```

The driver now calls the same `RustCore.engine` and direct platform backend as
the app. Only synthetic fixture generation remains Swift. Build with
`MOTIONLIVE_SWIFT_REFERENCE=1` for the retained reference. The runner supports
macOS Bash 3.2 empty argument arrays and never swaps the running driver.
Validation logs and limitations for the local integration are recorded outside
git in `.build/rust-engine-integration/handoff.md`.

## Known differences from the Swift engine

- **Movie writer.** The MOV is a sample-copy remux, not AVAssetWriter
  output. The content identifier is QuickTime keyed metadata
  (`moov/meta` `keys`/`ilst`); the still-image-time track uses the native
  iPhone `mebx` form with a 1/600 s marker sample, so
  `cover_duration_seconds` is 1/600 instead of the frame duration. Edit
  lists are kept. Whether Photos pairs these files has not been tested.
- **XMP serialization.** Rewritten packets are serialized with
  ElementTree-style prefixes (`ns0`, …) and without `<?xpacket?>`
  wrappers. Names, namespaces and values are unchanged; unchanged packets
  stay byte-identical.
- **Timelines.** Times are matched exactly (integer ticks); a cover exactly
  halfway between two frames is refused rather than resolved to the first.
- **Scheduling.** The macOS host supplies its thermal/CPU worker limit. Rust
  admits weighted work under physical memory / 8 (at least 256 MiB); other
  hosts use a 1 GiB fallback budget.
- **Folder walk.** Finder ordering (`localizedStandardCompare`) and package
  detection (`.skipsPackageDescendants`) are approximated: case-insensitive
  natural sort, and a fixed list of package extensions.
- **Error texts** from parsing and I/O are the Rust core's (parsing has
  gone through Rust since `ms_core` op 0).
