<p align="center">
  <img src="docs/branding/github-header.svg" alt="MomentStays — The moment stays. Cover timestamps, original audio/video streams, HDR gain maps, and Photos library integration." width="100%">
</p>

<p align="center">
  <strong>English</strong> · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <strong>macOS 12+ · Native SwiftUI · Local processing · Version 1.0.1</strong>
</p>

# MomentStays — Android Motion Photo ↔ Apple Live Photo Converter for macOS

Convert Android Motion Photos to Apple Live Photos — and convert Live Photos back to Motion Photos — without re-encoding the original video or audio. MomentStays preserves the cover (key) photo timestamp, Ultra HDR gain maps, and works directly with your Apple Photos library. Supports Samsung, Xiaomi, Google Pixel, Huawei/Honor, OPPO and vivo Motion Photos, in JPEG and HEIC.

安卓动态照片 ↔ 苹果实况照片（Live Photo）互转工具：在 Mac 上把三星、小米、Google、华为/荣耀、OPPO、vivo 的动态照片转换成 iPhone 实况照片，保留封面帧、原始音视频与 Ultra HDR，不重新编码，全部本地处理，并可直接导入「照片」图库。

<p align="center">
  <img src="https://img.shields.io/badge/macOS-12%2B-black" alt="macOS 12+">
  <img src="https://img.shields.io/badge/SwiftUI-Native-orange" alt="Native SwiftUI">
  <img src="https://img.shields.io/badge/License-GPL--3.0--only-blue" alt="License GPL-3.0-only">
</p>

**Download:** get the latest DMG/ZIP from [Releases](https://github.com/LinoFish/MomentStays/releases), or build from source below.

Move Motion Photos between Android and Apple Live Photos while keeping the moment you chose as the cover. Remux the original video and audio, preserve supported image and HDR data, and bring converted Live Photos into your Photos library.

[Build & run](#build--run) · [Photos integration](#works-with-your-photos-library) · [Preservation details](#what-stays-with-your-photo) · [Documentation](docs/README.md)

**License: [GPL-3.0-only](LICENSE)** · Copyright © 2026 Lino

## The moment stays

| | What MomentStays preserves |
| --- | --- |
| **The cover you chose** | Reads the source cover timestamp, maps it to an actual video frame, and writes the Live Photo timing marker. |
| **Original video and audio** | Remuxes compressed samples without video/audio re-encoding. Checks sample data, counts, sizes, and presentation timestamps. |
| **Image and HDR detail** | Keeps JPEG/HEIC image payloads on supported passthrough paths. Preserves existing Ultra HDR gain maps and handles supported Apple/ISO gain data through explicit models. |
| **Your library workflow** | Reads complete original or matching edited resources from Photos, creates new Live Photos, and adds them to your selected album. |

MomentStays changes the container and pairing metadata. Converting HEIC to JPEG, changing JPEG quality, or converting an HDR gain map can encode image or gain-map data; the original compressed video and audio remain preserved.

## Works with your Photos library

Choose files and folders, or work directly with the system Photos library through PhotoKit.

| From | Through MomentStays | To |
| --- | --- | --- |
| Android Motion Photos in files or complete Photos originals | Extract the still image and motion clip, preserve cover timing, validate the pair | A new **Live Photo in a Photos album**, or JPEG/HEIC + MOV files |
| Apple Live Photos in Photos or complete image + MOV pairs | Read the paired resources and timed cover marker, preserve audio/video streams | An **Android Motion Photo** in a folder, or the system share menu |

The library workflow includes:

- **Album-scoped selection.** Scan the chosen library scope and recognize motion data from the actual resources.
- **Originals by default.** Optionally use a complete edited pair. The still and movie always come from the same selected version.
- **Direct Live Photo import.** Add the still and paired video in one asset-creation request, then associate the new asset with the selected album.
- **Library metadata.** Keep original resource names, capture date, and location when available from trusted source metadata.
- **Controlled iCloud access.** Downloads and monitoring during the current app session are opt-in.
- **Recoverable delivery.** Persistent task records support deduplication, import/share retries, and safe cleanup of the app's own cache.

Source files and library assets stay intact. Library changes happen only when you choose an import destination. The app checks the created asset and its paired resources; real-library playback, HDR rendering, iCloud behavior, and receiving-device compatibility still need end-to-end validation.

## What stays with your photo

### Cover timestamps, mapped to the real frame

A cover timestamp identifies the selected moment **inside the motion clip**. MomentStays reads the source timing metadata and maps it through the actual media timeline. For Live Photos, it writes the corresponding `com.apple.quicktime.still-image-time` timed marker; in the other direction, it reads that marker to restore the Android cover position.

Frame alignment is checked, with a default tolerance of half a frame. Missing, ambiguous, or out-of-range timing is reported instead of silently selecting the midpoint. Optional manifests record source timing, the mapped frame, and alignment error.

The Photos capture date is separate metadata and is carried into the new library asset when available.

### Remuxed media, verified sample by sample

Video and audio move into the destination container as their **original compressed samples**. Output verification checks sample hashes, sizes, counts, and presentation timestamps (PTS).

Static-image passthrough paths keep the original JPEG/HEIC image encoding. Full resources are used throughout—no thumbnail or video-frame substitute for the original still. New results must pass validation before replacing an existing output.

### HDR data with a defined meaning

| Source data | Behavior |
| --- | --- |
| Existing Ultra HDR JPEG | Preserve the gain map and its parameters; the default JPEG-preservation path also keeps the original base-image encoding. |
| Supported Apple Gain Map | Convert through the supported Apple luminance model to an interoperable gain map. Gain-map conversion can involve encoding. |
| Interpretable ISO gain data | Preserve the coded images and supplement metadata only when the complete parameters can be explained. |
| HEIC passthrough | Retain coded image items, auxiliary data, and supported properties. Rendering Apple-private HDR on Android depends on the receiving device. |
| SDR or unknown private HDR | Keep SDR as SDR. Report unsupported HDR rather than inventing a gain map or silently claiming conversion. |

Some validated private static extensions can be retained as opaque data. Retaining those bytes does not guarantee another device can render the private HDR format.

## Supported workflows

| Direction | Formats and targets |
| --- | --- |
| Android → Apple | Google-compatible Motion Photo JPEG, supported Huawei/Honor, Samsung SEF, OPPO and vivo V2 containers; validated Samsung HEIC layouts → JPEG/HEIC + MOV |
| Apple → Android, JPEG | Google/generic, Xiaomi, Samsung, Huawei/Honor, OPPO, vivo V2 |
| Apple → Android, HEIC | Google, Xiaomi, Samsung; retains the existing HEIC encoding |

Batch processing includes progress, cancellation, verified resume, skip/overwrite decisions, optional JSON manifests/reports, and separate auxiliary-track export. A single-frame auxiliary track is never stretched into fabricated motion.

Vendor support applies to the layouts the parser can validate, not every device or firmware. See the [format scope](docs/product/双向转换与移动布局.md) (Chinese).

## Build & run

Building requires Xcode Command Line Tools. The default build produces an Intel macOS app:

```bash
git clone https://github.com/LinoFish/MomentStays.git
cd MomentStays
bash scripts/build_app.sh
open dist/MomentStays.app
```

A built app requires no Python, FFmpeg, ExifTool, Homebrew, or additional runtime. To build for another architecture:

```bash
bash scripts/build_app.sh --arm64
bash scripts/build_app.sh --universal
```

To produce a DMG, ZIP, and checksum list from the built app:

```bash
bash scripts/make_dmg.sh
```

Builds stay in local `dist/`. Current runtime validation is based on Intel/macOS 12; ARM hardware remains to be validated. The app uses ad-hoc signing without Developer ID signing or Apple notarization, so a downloaded copy may require approval in macOS security settings.

### First conversion

1. Select the direction and add files, a folder, or a Photos-library selection.
2. Choose the format and delivery destination: folder, Photos album, or system sharing.
3. Start conversion and review the queue/report. Stop or retry delivery when needed.

When importing files manually into Photos, select the matching JPEG/HEIC and MOV together. Independent auxiliary MOV files are ordinary videos. Keep the output folder outside the source folder.

### Optional Python CLI

The CLI provides a separate Google Motion Photo workflow; it does not expose every native-app, HEIC, vendor, or library feature. It requires Python 3.9+ and Command Line Tools:

```bash
python3 -m motionlive scan "/path/to/originals"
python3 -m motionlive convert "/path/to/originals" -o "/path/to/output"
```

The `./motion-live` launcher is also available. Native components compile into `.build/` on first use.

## Local by design

Conversion and verification run locally. Personal sample photos, converted outputs, full reports, build products, and editor captures are excluded from this repository. The project artwork is code-drawn SVG and uses no personal media.

Optional reports can contain source paths, EXIF, dates, and hashes—keep them local. Run the publication audit before sharing source changes:

```bash
python3 scripts/check_publish.py
```

## Development

```bash
python3 -m unittest discover -s tests -p 'test_*.py'
bash scripts/build_gui_tests.sh
python3 scripts/check_gui_engine.py
python3 scripts/check_reverse.py
python3 scripts/check_workspace.py
```

Native layout checks use fictional data and require an available macOS desktop:

```bash
bash scripts/build_app.sh --diagnostics
python3 scripts/check_swiftui_ui.py
```

| Area | Location |
| --- | --- |
| SwiftUI, task state, Photos and sharing | `gui/` |
| Parsing, HDR, timing, conversion, verification | `native/` |
| Optional Python workflow | `motionlive/` |
| Build scripts and regression drivers | `scripts/`, `tests/` |
| App metadata and reproducible icon drawing | `packaging/` |
| Project icon and README artwork | [Brand assets](docs/branding/README.md) |

New to MomentStays? Start with the **[User Guide](docs/USER_GUIDE.md)** ([简体中文版](docs/USER_GUIDE.zh-CN.md)) — every feature explained, including auxiliary tracks, Photos library import, and the CLI.

Further technical documentation is currently in Chinese: [implementation](docs/development/SwiftUI实现说明.md), [library integration](docs/design/图库识别与自动入册规范.md), [sharing](docs/design/输出位置与系统共享规范.md), [validation scope](docs/validation/图库与共享-1.0.md), and [local fixtures/privacy](docs/development/本地样片与隐私.md).

iOS is not currently shipped. Complex timelines, unknown HEIF structures, and unsupported private HDR may be rejected explicitly. Actual Photos playback/import, iCloud, third-party sharing, phone-gallery HDR, and ARM/iOS hardware are not covered by the synthetic regression suite.

## FAQ

### How do I convert a Samsung or Xiaomi Motion Photo to an Apple Live Photo?

Add the Motion Photo file (or a folder, or a Photos-library selection) in MomentStays, choose the Android → Apple direction, and deliver the result to a Photos album or as a JPEG/HEIC + MOV pair. The cover timestamp is mapped to the real frame, and the video/audio streams are remuxed without re-encoding.

### Can I convert an iPhone Live Photo back to an Android Motion Photo?

Yes. In the Apple → Android direction, MomentStays reads the paired image + MOV and its still-image-time marker, and writes a Motion Photo for Google/generic, Xiaomi, Samsung, Huawei/Honor, OPPO, or vivo V2 layouts (JPEG), or Google, Xiaomi, and Samsung layouts (HEIC).

### Does converting a Motion Photo lose quality?

No video or audio re-encoding takes place: the original compressed samples are remuxed and verified sample by sample. Supported image paths keep the original JPEG/HEIC encoding, and existing Ultra HDR gain maps are preserved.

### Which phones and formats are supported?

Google-compatible Motion Photo JPEGs, supported Samsung (including SEF and validated HEIC layouts), Xiaomi, Huawei/Honor, OPPO, and vivo V2 containers. See the Supported workflows table above for the exact directions and formats.

### Is MomentStays free and private?

Yes. MomentStays is GPL-3.0-only open source, runs entirely on your Mac, and never uploads your photos.

## License

The original source code, documentation, and artwork are licensed under the **GNU General Public License, version 3 only**. Commercial use is allowed. When distributing covered derivative works, provide their corresponding source under the GPL and meet the license's redistribution requirements.

See [LICENSE](LICENSE), [copyright and version choice](COPYRIGHT), and [third-party/distribution notes](THIRD_PARTY_NOTICES.md). Personal input media and separately licensed system or third-party components retain their own rights and terms.
