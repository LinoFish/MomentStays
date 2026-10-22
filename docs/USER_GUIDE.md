# MomentStays User Guide

> Applies to MomentStays 1.0.1 (macOS 12 or later).
> 中文版见 [USER_GUIDE.zh-CN.md](USER_GUIDE.zh-CN.md).

MomentStays converts Android Motion Photos into Apple Live Photos on your Mac — and back again. Everything follows three rules:

1. **The cover frame stays** — the moment you picked remains the cover, aligned to the real frame on the media timeline.
2. **Nothing is re-encoded** — video and audio are remuxed only and verified sample by sample, so quality is never lost.
3. **HDR is preserved honestly** — gain maps that can be kept are kept intact; anything unsupported is reported, never faked.

All conversion and verification happen locally on your Mac. Nothing is uploaded.

---

## Contents

- [1. Quick start](#1-quick-start)
- [2. Interface and importing](#2-interface-and-importing)
- [3. Directions and format support](#3-directions-and-format-support)
- [4. How the cover frame is located](#4-how-the-cover-frame-is-located)
- [5. Remuxing and sample-by-sample verification](#5-remuxing-and-sample-by-sample-verification)
- [6. How HDR is handled](#6-how-hdr-is-handled)
- [7. The auxiliary track: the second video track most people never see](#7-the-auxiliary-track-the-second-video-track-most-people-never-see)
- [8. Photos library: recognition, import, and monitoring](#8-photos-library-recognition-import-and-monitoring)
- [9. Batches, delivery, and sharing](#9-batches-delivery-and-sharing)
- [10. Settings reference](#10-settings-reference)
- [11. Manifests and reports](#11-manifests-and-reports)
- [12. Command-line tool (CLI)](#12-command-line-tool-cli)
- [13. Troubleshooting](#13-troubleshooting)
- [14. Known limitations](#14-known-limitations)

---

## 1. Quick start

1. Open MomentStays and click **Choose Photos**, drag files into the queue, or pick **Scan Photos Library…** from the source menu.
2. Confirm the direction: **Android → Apple Live Photo** (default) or **Apple Live Photo → Android**.
3. Open **Export Settings** and check the destination (folder / Photos library / sharing), target format, and HDR options.
4. Click **Start** (⌘Return). Settings are snapshotted when a task starts and locked while it runs.
5. Review per-photo results in the report. **Recent Results…** lets you retry delivery, reveal files, or open the output folder.

Each photo produces one result folder (see §9 for naming), not loose files.

**First launch:** the distributed app is ad-hoc signed and not notarized. If macOS blocks the first launch, allow it under System Settings → Privacy & Security, or Control-click the app in Finder and choose Open. Once installed, the app needs no Python, FFmpeg, ExifTool, or Homebrew.

## 2. Interface and importing

### 2.1 The queue

Each row is one photo. On wide windows the columns are **Filename | Format/HDR | Cover time | Status**; below 640pt the row wraps to two lines, the second showing "HDR · Cover xx". Selecting a row shows vendor and vendor-format details, the relative path, the detected cover time, and HDR status.

### 2.2 Four ways in

| Source | Notes |
|---|---|
| Choose files | Multi-select file picker |
| Choose folder | Read recursively |
| Drag & drop | Drop files onto the queue |
| Scan Photos Library | Enumerates library assets via PhotoKit and identifies formats; can be scoped to one album (see §8) |

### 2.3 Keyboard shortcuts

| Shortcut | Action |
|---|---|
| ⌘O | Choose photos |
| ⌘R | Rescan |
| ⌘Return | Start conversion |
| ⌘. | Stop |
| ⌘, | Export settings |

### 2.4 Task rules

- Starting a task snapshots all settings; they stay locked until it ends.
- Stopping signals cancellation and waits for the engine to clean up before a new task can start; stale callbacks from an old task never overwrite a new task's state.
- Reports are titled "Conversion report" or "Stopped task" accordingly.

## 3. Directions and format support

> Vendor support means "the layouts the parser can validate", not a promise for every device or firmware. Photos that were edited or forwarded through chat apps may already have lost their embedded video — lost video cannot be reconstructed from a still image.

### 3.1 Android → Apple

| Input | Notes |
|---|---|
| Google-compatible Motion Photo (JPEG) | Main XMP, Extended XMP, and fully verified cover-metadata recovery |
| Huawei / Honor (`LIVE_` trailer) | Including the raw cover-frame-index path (see §4) |
| Samsung SEF | Including the SEF trailer structure |
| Samsung HEIC | Only validated mpvd/SEF layouts; original HEVC image items are kept, never transcoded to JPEG |
| OPPO / OnePlus | OPPO container layout |
| vivo V2 | vivo V2 container layout |

Output is a Live Photo pair: a still (JPEG/HEIC) plus a separate MOV, paired by ContentIdentifier. The still output contains only verified still-image payloads and required extensions; motion and audio live only in the MOV.

### 3.2 Apple → Android

Stills and MOVs are matched by Apple ContentIdentifier, and the `still-image-time` timed marker is read and aligned to the actual video frame.

| Target | Vendors |
|---|---|
| JPEG | Google/generic, Xiaomi, Samsung (writes an SEF trailer), Huawei, Honor, OPPO, vivo V2 |
| HEIC | Google, Xiaomi, Samsung only; existing image, thumbnail, and gain-map encodings and properties are retained. JPEG is never re-encoded into HEIC for this |

## 4. How the cover frame is located

The **cover timestamp** is the instant in the motion video chosen as the cover. It is *not* the capture date — the two are separate metadata.

MomentStays reads the cover time recorded in the source, maps it along the real media timeline to the actual frame, and writes the Live Photo `still-image-time` marker. Converting back reads the marker and restores the Android cover position.

Rules that protect your cover:

- **Half-frame tolerance.** Frame alignment is checked with a default tolerance of half a frame; the snap error is recorded in the manifest (`snap_error_seconds`).
- **No guessing.** Missing, ambiguous, or out-of-range timing is reported — the app never silently substitutes the video midpoint, the capture date, or numbers from the file tail. Using the midpoint requires opting in under Advanced settings.
- **Two timelines (time domains).** A cover time may be recorded on the sample (media) timeline or the presentation timeline (edit list). Auto mode checks both; if they point to different frames, conversion stops and asks you to choose a time domain (see §10). Negative-PTS media needs explicit timeline handling, otherwise the original is kept and an error is reported.
- **Huawei/Honor frame index.** Some Huawei/Honor files record a raw cover *frame index* instead of a timestamp; the engine locates the frame by index and errors if it is out of range.
- **Recovery from video metadata.** If the main XMP is missing but the embedded video carries cover-time metadata, the engine can recover the original cover time from it and warns you about the source.
- **Read-back verification.** After the MOV is written, it is re-read to confirm the cover marker survived and the cover duration equals the actual frame duration; otherwise nothing is published.

## 5. Remuxing and sample-by-sample verification

- Video and audio move as their **original compressed samples** — remuxed, never decoded or re-encoded. Outputs are verified sample by sample for **hash, size, count, and presentation timestamps**, plus track-count equality and timeline-shift tolerance.
- Stills carry a **coding hash** over the encoded data only (metadata excluded); it must be identical before and after, and exported stills are asserted to contain no embedded media.
- The ContentIdentifier in the still's EXIF, the identifier inside the MOV, and the manifest UUID must all agree.

**Publishing and source protection:**

- **Atomic publish.** Results are written to a staging area and published only after every check passes; replacements swap atomically after the new result verifies.
- **Source fingerprint.** The source file's SHA-256 is compared before and after; if another program modifies the source mid-conversion, the result is not published.
- **Output location rule.** The output folder must be outside the source folder (enforced in both GUI and CLI).

**When an output already exists:**

- Same source, same settings, and the existing manifest verifies → the item is marked `resumed` and skipped.
- Different content or settings → an error, never a silent overwrite; pick a new output folder or explicitly choose Overwrite in the dialog.
- An older result that lacks Honor private-HDR still extensions is refused for resume and must be overwritten.

Batch item states: `converted`, `resumed`, `skipped`, `failed`.

## 6. How HDR is handled

| Input | Handling |
|---|---|
| Existing Ultra HDR (JPEG gain map) | Gain map and parameters preserved as-is; main image keeps its original encoding |
| Legacy Apple gain map | Converted to an Ultra HDR gain map using Apple's published Rec.709 brightness model. Only the gain map is re-encoded — the SDR base image never is. Valid headroom (stops) range: (0, 16] |
| ISO gain data (ISO/TS 21496-1) | When parameters are complete and interpretable, the encoded image is kept and only metadata is added; output can be promoted to Ultra HDR + ISO |
| HEIC passthrough | Encoded image items, auxiliary data, and supported properties retained. tmap-style variants stay inside HEIC and are never guessed into a legacy gain model |
| Honor private EHDR | Still extensions preserved as opaque data with byte-level verification. Preservation does not mean Apple devices can render the private model |
| SDR | Stays SDR; no gain map is invented |
| Unknown/private HDR (unknown tmap, PQ/HLG, complex HEIF…) | Never silently converted to SDR and never faked; reported clearly. Keeping the original HEIC encoding is an option |

More detail:

- Ultra HDR output needs an ICC profile: if none is present, an sRGB profile is added only when the EXIF ColorSpace explicitly says sRGB. Otherwise conversion errors instead of guessing.
- The **"Try compatible conversion for non-Ultra HDR"** switch (on by default) controls legacy compatibility paths. For Apple → Android, JPEG output uses Ultra HDR; plain SDR sources get no gain map.
- **HEIC → JPEG limit:** if a HEIC carries HDR that cannot be extracted safely on macOS 12 (some tmap/gain tracks), conversion fails explicitly and suggests keeping the HEIC encoding or providing a supported Apple HDR gain map.
- **Edited versions** may already have lost their gain map; reports mark them "edited version has no HDR gain map", and the app never borrows the original's gain map for an edit.

## 7. The auxiliary track: the second video track most people never see

### 7.1 What it is

Some Motion Photos carry **two** video tracks in their embedded video: the **primary motion track** — the continuous clip that matches the cover — and a **secondary (auxiliary) track**. In measured samples the auxiliary track can be a single frame at *higher* resolution (e.g. 2048px wide): more like a high-resolution key frame than a video. The engine's rule: at most one auxiliary track per photo.

Do not confuse it with two other things: the MPF auxiliary *images* inside a JPEG (gain maps etc. — that's what the manifest field `auxiliary_sha256` refers to) and HEIC auxiliary data. "Auxiliary track" here always means the **video** track.

### 7.2 Three export modes

Under Export Settings → Output content (Android → Apple only):

| Mode | Behavior |
|---|---|
| Live Photo only (default) | Build the Live Photo from the selected track |
| Live Photo + auxiliary track | Additionally saves the auxiliary track as a separate file, `<name>.auxiliary.mov`. Keep it apart from the Live Photo pair |
| Auxiliary track only | Exports just the auxiliary track as a standalone MOV (result folder suffix `.auxiliary-export`). No Live Photo, no audio; destination is forced to folder |

If the source has no auxiliary track, these operations report that clearly instead of skipping silently.

### 7.3 Using the auxiliary track as the motion video — strict conditions

The **Video track** setting can switch between the primary and auxiliary tracks, but:

- Fewer than 2 frames is an immediate refusal: "The auxiliary track has only X frame(s) and cannot be used as the motion video." **A single-frame auxiliary track is never stretched into fabricated motion.**
- The cover time must map onto the auxiliary track's *own* timeline. If it cannot, conversion errors out — it never falls back to the primary track and never moves the cover.
- Multi-frame auxiliary tracks convert normally; the manifest records `selected_video_role = "secondary"`.

The UI says it plainly: "The auxiliary track may contain only one frame and cannot replace continuous motion video."

### 7.4 Standalone export specification

- Precondition: the source must have exactly 2 video tracks.
- **Video only, no audio** (`audio_included: false` in the manifest).
- Samples pass through untouched, then verify: exactly 1 output video track, with compressed-data hash, sample sizes, and PTS counts matching the source.
- The result detail reads: "Auxiliary track exported separately: W × H, N frames, D seconds; no audio, not re-encoded."
- **Important:** an independent auxiliary MOV is an ordinary video. Do not import it into the library together with a Live Photo pair.

### 7.5 Advanced: repairing zero-duration auxiliary tracks

Some auxiliary tracks declare a duration of 0 for their only frame, so the system cannot read a valid sample and export fails. The Advanced switch **"Allow repair of zero-duration auxiliary tracks"** (off by default) handles this:

- It edits only the **exported copy's** duration fields (borrowing the primary track's shortest positive frame duration). Compressed sample bytes and sample timestamps are completely unchanged.
- If the layout cannot be strictly validated, the repair is refused.
- The repair is recorded in the manifest (`duration_repair`: method, original and repaired durations, `sample_pts_unchanged`) and surfaced as a warning in the results.
- It only affects standalone auxiliary exports; the switch is disabled in Live-Photo-only mode.

## 8. Photos library: recognition, import, and monitoring

MomentStays talks to the Photos library only through the public PhotoKit API — it never reads or writes the library database directly. Library permission is requested the first time you enter a library feature, not at app launch.

### 8.1 Scanning and recognition

- Scanning can be scoped to a chosen album. Progress distinguishes scanning assets, reading originals, and identifying formats.
- Identification parses the actual originals (vendor markers, XMP, container structure, video boundaries) — never just the file extension, filename, or an HDR flag. Plain stills are skipped without counting as failures; items marked as motion photos whose video is gone show a specific reason instead of a guess.
- Results are annotated with vendor, motion type, original/edited status, HDR status, and whether the original must be downloaded from iCloud.
- Assets previously created by MomentStays are excluded by local identifiers, so nothing loops through conversion twice.

### 8.2 Originals vs edited versions

The Library version setting defaults to **Unmodified original (keeps original HDR)**:

- Original: the paired original photo + paired video resources.
- Edited: "Current edited version (may lack HDR)" — only the matching full-size edited pair is used. If either half is missing, the reason is shown; halves are **never mixed**, and there is **no silent fallback** to the original.

### 8.3 Direct import

- Import is available for **Android → Apple** only. The still and paired video are created as one Live Photo in a single library request, then associated with the target album — default **"MomentStays Conversion Results"**, or another user album you pick.
- Before import, pairing identifiers, the cover timing marker, and "no embedded media in the still" are verified again. **Independent auxiliary MOVs are never attached as the paired video**, and JSON manifests are never imported as photo resources.
- Original filenames, capture dates, and locations are preserved (library values win; file metadata is the fallback). After writing, the new asset is read back and checked — Live Photo properties, filename, date, location, album membership — before it is shown as "Imported".
- **Stage-scoped retries:** an import failure retries only the import (verified files are kept); an album-association failure retries only the association. Assets are never created twice.
- **De-duplication records** live in `~/Library/Application Support/Motion-Live/` (the internal data-folder name intentionally keeps the legacy name): `imports.json` and friends record source assets, settings, verification summaries, and target assets. If a record file is unreadable, writing stops with an error rather than risking duplicate imports. Assets you deleted in Photos are marked missing and are not silently re-created; anything unconfirmable is marked "to verify" instead of being blindly retried.

### 8.4 iCloud and incremental monitoring

- Library resources may exist only in iCloud; scans flag "original needs download". **Downloads and monitoring are opt-in**, chosen in the library picker, and stop when you leave.
- Incremental monitoring (second-stage feature) watches the chosen albums and direction **while the app is running**; the status bar shows "Monitoring the selected library scope" with a Stop button. There is no background daemon: quitting stops monitoring, and after a restart the app re-enumerates and reconciles with local records (macOS 12 has no persistent change token).

## 9. Batches, delivery, and sharing

### 9.1 Destinations

| Destination | Android → Apple | Apple → Android |
|---|---|---|
| Save to folder | ✓ | ✓ |
| Import into Photos library | ✓ | — |
| Folder + import | ✓ | — |
| Share directly | — | ✓ |
| Folder + share | — | ✓ |

Auxiliary-only mode is folder-only. The main button label follows the destination ("Convert and Import", "Export Auxiliary Track", …).

### 9.2 Output naming

One result folder per source (stills/MOV default to `photo.jpg` / `photo.mov` inside; the original filename is recorded in the manifest's `original_filename`, and output modification times inherit the source's):

| Case | Folder |
|---|---|
| Android → Apple | `<relative path>.livephoto` (still + MOV, plus manifest when enabled) |
| Auxiliary only | `<relative path>.auxiliary-export` |
| Apple → Android | `<relative path>.motionphoto` |

### 9.3 System sharing (Apple → Android)

- When a batch finishes, the system share menu is presented **once** for the successful files, passing the final file URLs (`.MP.jpg` / `.MP.heic`) — never thumbnails or NSImage objects that the system might re-encode.
- Manifests, result folders, and paired MOVs are not shared by default. Partial success shares only the successes and reports the failure count; total failure or a stopped task does not pop the menu.
- Cancelling the share sheet does not change "conversion succeeded" status; status records only "handed to the sharing app", never "received".
- Third-party services may still modify files in transit; verify byte/HDR/motion/cover integrity at the receiving end when it matters.

### 9.4 Reports and recent results

- Reports filter by **All / Converted / Resumed / Skipped / Failed**; each entry offers the full reason, copyable details, and Reveal in Finder. Reports remain viewable in-app even when manifest export is off.
- **Recent Results…** can retry delivery, view files, open the output folder, and clear the **app's own task cache**. Cleanup never deletes files in your output folders or library assets, and is disabled while a share is in progress.

## 10. Settings reference

Open via the top-bar **Export Settings** (an inspector on windows ≥960pt wide, otherwise a sheet). Items marked * are Advanced and collapsed by default.

| Group | Setting | Options / default |
|---|---|---|
| Direction | Conversion direction | Android → Apple Live Photo (default) / Apple Live Photo → Android |
| Output | Destination | See §9.1; enabled options depend on direction and mode |
| Output | Target album name | Default "MomentStays Conversion Results"; empty falls back to default |
| Output | Library version | Unmodified original (default) / Current edited version |
| Content (→Apple) | Export mode | Live Photo only (default) / + auxiliary track / auxiliary only |
| Content (→Apple) | Video track | Primary motion track (default) / Auxiliary track |
| HDR (→Apple) | Try compatible conversion for non-Ultra HDR | On by default |
| Target (→Android) | Target vendor | Google/generic (default), Xiaomi, Samsung, Huawei, Honor, OPPO, vivo V2 |
| Target (→Android) | Photo format | JPEG · Ultra HDR / HEIC · keep original encoding (HEIC only for Google, Xiaomi, Samsung; other vendors force JPEG) |
| Image quality (→Android) | Keep original JPEG encoding | On by default (no recompression) |
| Image quality (→Android) | Main image quality / gain map quality | Sliders 1%–100%, both default 95% |
| Verification | Export manifest & JSON report | Off by default (see §11) |
| Advanced * | When cover time is not recorded | Report an error (default, recommended) / Use video midpoint |
| Advanced * | Cover time meaning (time domain) | Auto-check (default, recommended) / media / presentation |
| Advanced * | Allow repair of zero-duration auxiliary tracks | Off by default (see §7.5) |

As the Advanced section warns: auto mode checks the cover position and reports ambiguity as an error — change these only if you understand the source's timeline.

## 11. Manifests and reports

Each result folder includes a `manifest.json` when **Export manifest & JSON report** is enabled. The switch is **off by default**: with it off, only media is exported, but internal verification still runs, the in-app report is still available, and library import does not depend on the files. Key fields:

| Field | Meaning |
|---|---|
| `source` / `source_sha256` | Source path and fingerprint |
| `identifier` | The pairing ContentIdentifier UUID |
| `source_format` | Detected source format (e.g. `samsung-sef`, `vivo-v2`) |
| `source_timestamp_us` / `source_timestamp_tag` | Source cover time and where it came from |
| `timing` | Timeline report: time domain, cover seconds, `snap_error_seconds`, timing policy (source XMP / video metadata / Huawei frame / midpoint), selected track and role, frame count, dimensions |
| `hdr` | Input/output HDR status and reason |
| `primary_coding_sha256` | Coding hash of the main still |
| `auxiliary_sha256` | Per-image hashes of MPF auxiliary images inside the still (gain maps etc.) — image data, not the §7 video track |
| `auxiliary_video` | Auxiliary-track export info: track id, frames, dimensions, duration, hash, audio flag, duration-repair record |
| `output_sha256` / `warnings` | Output file hashes and all warnings |

Batch reports are named `report-<date>-<time>-<random>.json`, with counts and per-file results. History reports are not deleted automatically.

**Privacy:** reports and manifests can contain source paths, EXIF, dates, and hashes. Keep them local; MomentStays itself uploads nothing.

## 12. Command-line tool (CLI)

The repository ships a separate Python CLI that shares parts of the native engine. **Scope:** the CLI covers the Google Motion Photo (JPEG) workflow only — no HEIC, vendor containers, auxiliary tracks, or library features.

Requirements: Python 3.9+, macOS 12+, Xcode Command Line Tools. The first run compiles the native helper automatically (with a notice).

```bash
# from the repository root
./motion-live --help
./motion-live --version        # 0.1.0

# 1) Read-only scan: cover time, HDR, format problems (file or folder, recursive)
./motion-live scan ~/Photos/incoming

# 2) Batch convert (output dir is required and must be outside the input dir)
./motion-live convert ~/Photos/incoming -o ~/Photos/converted

# 3) Verify an existing result against its recorded checksums
./motion-live verify ~/Photos/converted/IMG_0001.jpg.livephoto
```

`convert` options:

| Option | Default | Meaning |
|---|---|---|
| `--missing-time {error,midpoint}` | `error` | Error when cover time is missing; `midpoint` uses the video midpoint and must be chosen explicitly |
| `--legacy-hdr {auto,preserve}` | `auto` | `auto` tries converting an existing legacy Apple gain map; plain SDR never gains HDR. `preserve` keeps it untouched |
| `--time-domain {auto,media,presentation}` | `auto` | Checks sample and presentation timelines and errors on ambiguity; can be pinned explicitly |

Behavior: inputs are `.jpg/.jpeg` only (symlinks skipped). Plain still JPEGs are `skipped`; corrupt motion photos are `failed` and make the exit code 1. Each result is a `<name>.livephoto/` folder (photo.jpg + photo.mov + manifest.json) plus a batch report in the output root. Existing results with matching fingerprint and options are `resumed`; mismatches error out and ask for a different output directory. Publishing is atomic and non-overwriting, and output timestamps inherit the source's access/modification times.

Parsing is strict by design: Motion Photo detection relies on GCamera XMP and container structure, the video must sit at the file tail and pass full box-structure validation — never signature sniffing — and an Ultra HDR claim without an MPF auxiliary image is an error.

## 13. Troubleshooting

| Symptom / message | Cause and what to do |
|---|---|
| Converted photo doesn't move on iPhone | Usually the file was copied without conversion, or pairing/cover markers are incomplete. Convert with MomentStays and deliver via library import or as a pair; long-press in Photos to play |
| "Cover time missing" | The source has no verifiable cover time. Use an unedited original; if you must proceed, choose "Use video midpoint" under Advanced (the strategy is recorded) |
| "Ambiguous cover time (media vs presentation)" | The two timelines disagree. Pin a time domain under Advanced, or use the unedited original |
| "Auxiliary track has only 1 frame" | Expected. Use the primary track, or export the auxiliary track on its own as an ordinary video |
| "Source has no auxiliary video track" | That photo has a single video track; auxiliary modes don't apply |
| "Output exists with different content or settings" | The app refuses to overwrite a mismatched result. Choose a new output folder or explicitly Overwrite in the dialog |
| "Library de-duplication record unreadable; writing stopped" | The dedup record is corrupt; writing stops to prevent duplicate imports. Inspect the records under `~/Library/Application Support/Motion-Live/` or contact the developer |
| HEIC → JPEG says HDR cannot be extracted safely | That HEIC's HDR structure is unsupported. Choose "HEIC · keep original encoding" instead |
| HDR missing after conversion | You may have used the library's edited version (edits can drop the gain map) — switch to the unmodified original. SDR or unknown private HDR sources are reported as such |
| macOS blocks first launch | The app is ad-hoc signed. Allow it in System Settings → Privacy & Security, or Control-click → Open in Finder |
| Library scan shows "download needed" | The original is only in iCloud. Enable downloads in the library picker and retry |

## 14. Known limitations

- **Platform:** macOS 12+ only. Validation is primarily on Intel / macOS 12; Apple Silicon validation is in progress. No iOS release.
- **Signing:** ad-hoc signature, no Developer ID, no notarization (see the first-launch note in §1).
- **Builds:** source builds default to Intel; `--arm64` / `--universal` must be requested explicitly.
- **Vendor formats:** only parser-validated layouts; not every device, firmware, or edited image is covered. Motion Photos stripped by editors/sharing may have lost their video irrecoverably.
- **Real-library behavior:** real-world playback after import, re-export, HDR display, permission revocation, and similar system behaviors are under ongoing real-environment validation; automated regression uses synthetic fixtures and does not equal every real device.
- **Monitoring scope:** library monitoring runs only while the app is open and only for the chosen scope; no background daemon.
- **Sharing scope:** third-party share targets may still modify files; verify at the receiving end.
- **CLI scope:** Google Motion Photo JPEG workflow only (see §12).

---

*This guide is based on the MomentStays 1.0.1 product, design, and validation documents and on the application's source behavior. If the guide and an in-app message disagree, the in-app message and the result report win.*
