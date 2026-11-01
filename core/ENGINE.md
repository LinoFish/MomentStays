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

## Host callbacks (`Native` / `MsNative`)

| Callback | Swift code it replaces |
|---|---|
| `image_decodes` | `CGImageSource` complete-decode check in `confirmedStatic` and `AndroidTransfer.inspect` |
| `srgb_icc` | `CGColorSpace(name: .sRGB).copyICCData()` in `legacyGain` |
| `decode_gray_jpeg` / `encode_gray_jpeg` | gain-map decode/encode in `legacyGain` and `AndroidHDR.convert` (the lookup table runs in Rust) |
| `reencode_jpeg` | `AndroidHDR.jpeg(CGImageSourceCreateImageAtIndex…)` when the still is re-encoded |
| `heic_gain_map` | `CGImageSourceCopyAuxiliaryDataInfoAtIndex(…HDRGainMap)` + its version/headroom tags |
| `frame_jpeg` | `AVAssetImageGenerator` frame of the secondary track in `CoverCandidateSession.writeJPEG` |
| `progress`, `existing_output`, `rows_ready` | `BatchProgress`, the existing-output dialog latch, `rowsReady` |

A missing callback makes that step fail with `需要原生处理：…`. Batch and
scan work runs on several Rust threads, so codec callbacks must be
thread-safe. `existing_output` may block while the user decides.

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
apple/macos/scripts/build_gui_tests.sh             # Swift driver, still used for `fixture`
apple/macos/scripts/run_checks_with_rust.sh        # all NativeDriver checks on Rust
apple/macos/scripts/run_checks_with_rust.sh check_vendors.py check_reverse.py
```

The runner swaps `.build/gui/native-tests` for a shim that sends `fixture`
requests to the Swift driver and everything else to `momentstays-driver`,
then restores it. `.build/rust-shim.log` records which engine answered each
request and whether Rust asked for host codecs. The standalone driver has
no codecs, so checks that need them (legacy Apple HDR conversion, HEIC →
JPEG, cover candidates) are expected to fail there until the app's
callbacks are wired.

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
- **Scheduling.** No thermal-state check; the memory budget is a fixed
  1 GiB instead of physical memory / 8.
- **Folder walk.** Finder ordering (`localizedStandardCompare`) and package
  detection (`.skipsPackageDescendants`) are approximated: case-insensitive
  natural sort, and a fixed list of package extensions.
- **Error texts** from parsing and I/O are the Rust core's (parsing has
  gone through Rust since `ms_core` op 0).
