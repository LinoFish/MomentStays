# Third-party materials and distribution

MomentStays's original source, documentation, and artwork are licensed under **GPL-3.0-only**, with copyright **2026 Lino**. See [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT).

## Runtime dependencies

The native application uses frameworks supplied by macOS, including AppKit, SwiftUI, AVFoundation, ImageIO, CoreGraphics, PhotoKit, and CryptoKit. The project build does not vendor or bundle copies of those Apple frameworks. Their own terms remain applicable; the project license does not relicense Apple software.

The optional Python workflow uses the Python standard library and the project's native helper. No third-party Python package is bundled by the current build scripts.

## Optional validation tools

`tests/ProbeUltraHDR.cpp` is a test-only adapter for Google's [libultrahdr](https://github.com/google/libultrahdr). The library and its headers are external prerequisites for that optional probe; the app build does not compile or bundle them. The upstream [license file](https://github.com/google/libultrahdr/blob/main/LICENSE) provides MIT and Apache-2.0 license texts. Copies of upstream materials retain their own license and attribution requirements.

Links to format specifications, research projects, and reference samples are references, not copies of those projects or a grant of rights to their materials. Personal media and local research downloads are excluded from this repository.

## Format research and compatibility references

The source and development records identify these research references:

| Project | Scope consulted |
| --- | --- |
| [MotionTrans](https://github.com/Huakira/MotionTrans) | `MediaDataOffset` and static-image separation behavior. |
| [Live Photo Box](https://github.com/LengxiQwQ/live-photo-box) | Huawei `LIVE_` cover-frame fields, OPPO namespaces and vivo V2 video-length semantics. |
| [ExifTool](https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Samsung.pm) | Samsung SEF entry layout and `mpv2` pointers. |
| [doodspav/motionphoto](https://github.com/doodspav/motionphoto) | Samsung SEF directory fields. |
| [Google libultrahdr](https://github.com/google/libultrahdr/blob/main/lib/src/gainmapmetadata.cpp) | ISO 21496-1 fractional metadata and gain-map parameter semantics; also the external validation dependency described above. |
| [OpenHarmony multimedia_image_framework](https://github.com/openharmony/multimedia_image_framework) | HDR helper and MPF parsing research, with public fixtures used for local comparison. |
| [libheif](https://github.com/strukturag/libheif) | HEIF/HEIC container structure. |
| [NightMean/MotionPhotoSamples](https://github.com/NightMean/MotionPhotoSamples) | Public Samsung Motion Photo fixtures used in local compatibility checks. |

These acknowledgements describe the recorded research scope; they do not claim that the referenced converters are dependencies of the native app. Local research copies and downloaded samples are not part of the published source or installer. In particular, a public sample URL does not itself grant permission to redistribute its media. Upstream materials retain their own licensing terms.

## Documentation, installer and design tooling

- [ds_store](https://github.com/dmgbuild/ds_store) and [mac_alias](https://github.com/dmgbuild/mac_alias): MIT-licensed build dependencies, pinned in `packaging/dmg-requirements.txt`, used by `packaging/dmg_layout.py`. Their Python code is not bundled in the app or DMG.
- [dmgbuild](https://github.com/dmgbuild/dmgbuild) and Chromium's [DMG layout generator](https://chromium.googlesource.com/chromium/src/+/main/chrome/installer/mac/generate_dsstore.py): references for installer creation and Finder layout fields.
- [Python-Markdown](https://github.com/Python-Markdown/markdown): BSD-3-Clause documentation-tool dependency imported by `scripts/sync_website_guides.py`; not needed to run the app.
- [huashu-design](https://github.com/alchaincyf/huashu-design): design workflow used during interface-prototype exploration, as recorded in the design documentation; not an app runtime dependency.

Apple and Google developer documentation and format specifications are additional technical references. LocalSend, when installed and enabled by the user, is an external system-sharing target; it is not bundled or linked into MomentStays.

## Distributing a binary

The application build copies `LICENSE`, `COPYRIGHT`, and this file into the app's Resources directory. The DMG contains that signed application and an Applications link; notices remain inside the application bundle.

When conveying a covered binary, provide the corresponding source through a method permitted by GPLv3 section 6. Use the source that actually corresponds to that build, including build/install scripts and the required notices. If offering a binary download, provide recipients equivalent access to its corresponding source as required by that section. A private repository URL alone does not provide source access to recipients who cannot access it.

The current packaging scripts do not automatically create a corresponding-source release or a written source offer. Arrange source access before distributing a binary. Adopting this license does not change the GitHub repository's private/public setting.
