<p align="center">
  <img src="docs/branding/github-header.svg" alt="MomentStays — The moment stays. Cover timestamps, original audio/video streams, HDR gain maps, and Photos library integration." width="100%">
</p>

<p align="center">
  <strong>English</strong> · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <strong>macOS 12+ · Native SwiftUI · Local processing · Version 1.5.1</strong>
</p>

# 留刻— Android Motion Photo ↔ Apple Live Photo Converter for macOS

Convert Android Motion Photos to Apple Live Photos — and convert Live Photos back to Motion Photos — without re-encoding the original video or audio. MomentStays preserves the cover (key) photo timestamp, Ultra HDR gain maps, and works directly with your Apple Photos library. Supports Samsung, Xiaomi, Google Pixel, Huawei/Honor, OPPO and vivo Motion Photos — JPEG, plus validated Samsung HEIC layouts.

安卓动态照片 ↔ 苹果实况照片（Live Photo）互转工具：在 Mac 上把三星、小米、Google、华为/荣耀、OPPO、vivo 的动态照片转换成 iPhone 实况照片，保留封面帧、原始音视频与 Ultra HDR，音视频不重新编码，全部本地处理，并可直接导入「照片」图库。

<p align="center">
  <img src="https://img.shields.io/badge/macOS-12%2B-black" alt="macOS 12+">
  <img src="https://img.shields.io/badge/SwiftUI-Native-orange" alt="Native SwiftUI">
  <img src="https://img.shields.io/badge/License-GPL--3.0--only-blue" alt="License GPL-3.0-only">
</p>

> **Feedback welcome:** Tried it and something failed or looks wrong? [Open an issue](https://github.com/LinoFish/MomentStays/issues) — tell us the source device and format, the conversion direction and the app version, and strip sensitive information from any sample first. See [Feedback](#feedback) below.

**Download:** get the latest DMG/ZIP from [Releases](https://github.com/LinoFish/MomentStays/releases), or build from source below.

Move Motion Photos between Android and Apple Live Photos while keeping the moment you chose as the cover. Remux the original video and audio, preserve supported image and HDR data, and bring converted Live Photos into your Photos library.

[Build & run](#build--run) · [Photos integration](#works-with-your-photos-library) · [Preservation details](#what-stays-with-your-photo) · [Documentation](docs/README.md)

**License: [GPL-3.0-only](LICENSE)** · Copyright © 2026 Lino

## Feedback

Feedback is welcome. If a conversion fails, or the result does not look right, please [open an issue](https://github.com/LinoFish/MomentStays/issues) and describe what happened: the source device and format, the conversion direction, the app version, what you expected, and what you got instead.

If you can share a sample that reproduces the failure, please remove sensitive information first: use a photo that contains no personal content, and strip location and other private metadata before uploading. A minimal sample that still reproduces the problem is more useful than a full personal album.

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
| Apple → Android, JPEG | Google/generic (recommended for newer Honor), Xiaomi, Samsung, Huawei, Honor (fallback), OPPO, vivo V2 |
| Apple → Android, HEIC | Samsung; retains the existing HEIC encoding (Google and Xiaomi Motion Photos do not use HEIC) |
| Android → Android | Google → Honor only; validated Google Motion Photo JPEG, preserving the complete original video and source cover |

Batch processing includes progress, cancellation, verified resume, skip/overwrite decisions, optional JSON manifests/reports, and separate auxiliary-track export. A single-frame auxiliary track is never stretched into fabricated motion.

Each photo can use an auxiliary high-resolution frame as its cover while keeping primary-track motion. Candidate-photo mode exports every cover as a separate Live Photo, or keeps only the main photo live and exports the rest as stills. Library delivery creates separate assets for these candidates.

Vendor support applies to the layouts the parser can validate, not every device or firmware. See the [format scope](docs/product/双向转换与移动布局.md) (Chinese).

## Build & run

Building requires Xcode Command Line Tools. The default build produces a Universal macOS app:

```bash
git clone https://github.com/LinoFish/MomentStays.git
cd MomentStays
bash scripts/build_app.sh
open dist/留刻.app
```

A built app requires no Python, FFmpeg, ExifTool, Homebrew, or additional runtime. To build only for Apple Silicon:

```bash
bash scripts/build_app.sh --arm64
bash scripts/build_app.sh --universal
```

To produce a DMG, ZIP, and checksum list from the built app:

```bash
python3 -m pip install --target .build/dmg-tools -r packaging/dmg-requirements.txt
bash scripts/make_dmg.sh
```

The DMG uses only the app icon, Applications link and a drag arrow. The user guide is available from the app; licenses are inside the app bundle. These Python tools are needed only for packaging; app users do not need them.

Builds stay in local `dist/`. Current runtime validation is based on Intel/macOS 12; ARM hardware remains to be validated. The app uses ad-hoc signing without Developer ID signing or Apple notarization, so a downloaded copy may require approval in macOS security settings.

### First conversion

1. Select the direction and add files, a folder, or a Photos-library selection.
2. Choose the format and delivery destination: folder, Photos album, or system sharing.
3. Start conversion and review the queue/report. Stop or retry delivery when needed.

When importing files manually into Photos, select the matching JPEG/HEIC and MOV together. Independent auxiliary MOV files are ordinary videos. Keep the output folder outside the source folder.

## Local by design

Conversion and verification run locally. Personal sample photos, converted outputs, full reports, build products, and editor captures are excluded from this repository. The project artwork is code-drawn SVG and uses no personal media.

Optional reports can contain source paths, EXIF, dates, and hashes—keep them local. Run the publication audit before sharing source changes:

```bash
python3 scripts/check_publish.py
```

## Development

The legacy Python CLI has been removed. Python is retained only for format references, regression tests, packaging and documentation tooling; the app runs independently of it.

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
| Python format reference for regression and Rust parity | `motionlive/` |
| Build scripts and regression drivers | `scripts/`, `tests/` |
| App metadata and reproducible icon drawing | `packaging/` |
| Project icon and README artwork | [Brand assets](docs/branding/README.md) |

New to MomentStays? Start with the **[User Guide](docs/USER_GUIDE.md)** ([简体中文版](docs/USER_GUIDE.zh-CN.md)) — every feature explained, including auxiliary tracks and Photos library import.

Further technical documentation is currently in Chinese: [implementation](docs/development/SwiftUI实现说明.md), [library integration](docs/design/图库识别与自动入册规范.md), [sharing](docs/design/输出位置与系统共享规范.md), and [local fixtures/privacy](docs/development/本地样片与隐私.md).

iOS is not currently shipped. Complex timelines, unknown HEIF structures, and unsupported private HDR may be rejected explicitly. Actual Photos playback/import, iCloud, third-party sharing, phone-gallery HDR, and ARM/iOS hardware are not covered by the synthetic regression suite.

## FAQ

### How do I convert a Samsung or Xiaomi Motion Photo to an Apple Live Photo?

Add the Motion Photo file (or a folder, or a Photos-library selection) in MomentStays, choose the Android → Apple direction, and deliver the result to a Photos album or as a JPEG/HEIC + MOV pair. The cover timestamp is mapped to the real frame, and the video/audio streams are remuxed without re-encoding.

### Can I convert an iPhone Live Photo back to an Android Motion Photo?

Yes. In the Apple → Android direction, MomentStays reads the paired image + MOV and its still-image-time marker, and writes a Motion Photo for Google/generic (recommended for newer Honor phones), Xiaomi, Samsung, Huawei, Honor (fallback), OPPO, or vivo V2 layouts (JPEG), or the Samsung layout (HEIC; Google and Xiaomi Motion Photos are JPEG-only).

Newer Honor phones have been verified to recognize Google-format files exported by MomentStays. The Honor option remains a fallback. The separate Android → Android direction supports only safely validated Google Motion Photo JPEG → Honor; unsupported or ambiguous structures are rejected.

### Does converting a Motion Photo lose quality?

No video or audio re-encoding takes place: the original compressed samples are remuxed and verified sample by sample. Supported image paths keep the original JPEG/HEIC encoding, and existing Ultra HDR gain maps are preserved. One exception: Apple HEIC → Android JPEG cannot be lossless. The still image must be re-encoded because the format changes and HEIC support on Android is limited, and an Apple HDR gain map converted for Android may not be bit-exact. Choose the Samsung HEIC target to keep the original HEIC encoding.

### Which phones and formats are supported?

Google-compatible Motion Photo JPEGs, supported Samsung (including SEF and validated HEIC layouts), Xiaomi, Huawei/Honor, OPPO, and vivo V2 containers. See the Supported workflows table above for the exact directions and formats.

### Is MomentStays free and private?

Yes. MomentStays is GPL-3.0-only open source, runs entirely on your Mac, and never uploads your photos.

## Acknowledgements

Thank you to the projects, authors and maintainers whose code, tools, research and design workflows support MomentStays:

- **System APIs and typography:** [Apple frameworks and SDK documentation](https://developer.apple.com/documentation/), [SF Symbols](https://developer.apple.com/sf-symbols/) and [system fonts](https://developer.apple.com/fonts/).
- **Rust core code dependencies:** [RustCrypto](https://github.com/RustCrypto/hashes) (`sha2`, `md-5`), [roxmltree](https://github.com/RazrFalcon/roxmltree), [serde_json](https://github.com/serde-rs/json), and their complete locked transitive dependencies, listed with versions and licenses in the notices. The Rust core is experimental and separate from the native app.
- **Motion Photo implementation references:** [MotionTrans](https://github.com/Huakira/MotionTrans), [Live Photo Box](https://github.com/LengxiQwQ/live-photo-box), [ExifTool](https://exiftool.org/), [motionphoto](https://github.com/doodspav/motionphoto).
- **HDR, HEIC and compatibility research:** [OpenHarmony](https://github.com/openharmony/multimedia_image_framework), [libheif](https://github.com/strukturag/libheif), [libultrahdr](https://github.com/google/libultrahdr), [MotionPhotoSamples](https://github.com/NightMean/MotionPhotoSamples). Evaluated alternatives: [libmotionphoto](https://github.com/google/libmotionphoto), [MotionPhoto2](https://github.com/PetrVys/MotionPhoto2), [LiveMux](https://github.com/ysya/LiveMux).
- **Packaging, documentation and validation:** [ds_store](https://github.com/dmgbuild/ds_store), [mac_alias](https://github.com/dmgbuild/mac_alias), [dmgbuild](https://github.com/dmgbuild/dmgbuild), [Chromium](https://chromium.googlesource.com/chromium/src/+/main/chrome/installer/mac/generate_dsstore.py), [Python-Markdown](https://github.com/Python-Markdown/markdown), [importlib_metadata](https://github.com/python/importlib_metadata), [zipp](https://github.com/jaraco/zipp), [fontTools](https://github.com/fonttools/fonttools) and [Brotli](https://github.com/google/brotli).
- **Design and review Skills:** [huashu-design](https://github.com/alchaincyf/huashu-design) for app prototype exploration and website design; [frontend-design](https://github.com/anthropics/skills/tree/main/skills/frontend-design) for visual direction; [ui-ux-pro-max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill) for design/interaction guidance; [ux-audit](https://github.com/paulunemoon/ux-audit-skill) for evidence-based review.
- **Website fonts:** [Noto Serif SC / Noto CJK](https://github.com/notofonts/noto-cjk), [Adobe / Source Han Serif](https://github.com/adobe-fonts/source-han-serif) and [Google Fonts](https://github.com/google/fonts); [SIL OFL 1.1](docs/branding/fonts/OFL.txt). See [font credits and full license](docs/branding/fonts/README.md).
- **Specifications and interoperability:** [Android Motion Photo](https://developer.android.com/media/platform/motion-photo-format), [Apple developer documentation](https://developer.apple.com/documentation/)/[HIG](https://developer.apple.com/design/human-interface-guidelines/), [Adobe XMP](https://developer.adobe.com/xmp/), [W3C RDF](https://www.w3.org/RDF/)/[WCAG](https://www.w3.org/TR/WCAG22/), [ISO media](https://www.iso.org/standard/85596.html)/[gain-map formats](https://www.iso.org/standard/86775.html) and [LocalSend](https://github.com/localsend/localsend) sharing research.

See the [complete classified third-party notices](THIRD_PARTY_NOTICES.md) for each role, all 23 locked Rust crates, toolchain credits, upstream license links and distribution scope. References, evaluated alternatives and Skills are identified separately from shipped dependencies.

## License

The original source code, documentation, and artwork are licensed under the **GNU General Public License, version 3 only**. Commercial use is allowed. When distributing covered derivative works, provide their corresponding source under the GPL and meet the license's redistribution requirements.

See [LICENSE](LICENSE), [copyright and version choice](COPYRIGHT), and [third-party/distribution notes](THIRD_PARTY_NOTICES.md). Personal input media and separately licensed system or third-party components retain their own rights and terms.
