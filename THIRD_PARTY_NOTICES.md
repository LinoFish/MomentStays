# Third-party acknowledgements and distribution

第三方分类与致谢：涵盖实际使用的依赖、代码与格式研究、网站字体、构建与验证工具、设计 Skill，以及单独标明的评估参考。中文分类摘要见 [README.zh-CN.md](README.zh-CN.md#致谢)。

MomentStays's original source, documentation and artwork are **GPL-3.0-only**, copyright **2026 Lino**. Third-party materials retain their own copyrights and terms. See [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT).

## 1. Application code, system dependencies and shipped assets

The native build compiles the project's `native/` and `gui/` Swift files. The converter projects in section 4 are credited for their recorded research/reference roles; the current build does not link or vendor their converter libraries. The experimental Rust module has its own dependency list in section 2 and is not integrated into the macOS app.

| Component / project | Actual use | Distribution and terms |
| --- | --- | --- |
| [Apple frameworks and SDK](https://developer.apple.com/documentation/) | AppKit, SwiftUI, Foundation, AVFoundation, CoreMedia, CoreVideo, CoreGraphics, ImageIO, PhotoKit, UniformTypeIdentifiers and CryptoKit provide the native interface, media, photo-library and hashing APIs. | System dependencies; Apple SDK/framework terms apply. Their framework binaries are supplied by macOS. |
| [Apple SF Symbols](https://developer.apple.com/sf-symbols/) and [system fonts](https://developer.apple.com/fonts/) | Native controls, symbols and app typography. | Resolved through system APIs; Apple's applicable terms apply. |
| [Noto Serif SC / Noto CJK](https://github.com/notofonts/noto-cjk), [Google Fonts distribution](https://github.com/google/fonts/tree/main/ofl/notoserifsc) | Regular and bold WOFF2 subsets for the GitHub Pages website, served as `assets/fonts/ms-serif-regular.woff2` and `ms-serif-bold.woff2`; CSS family alias `MS Serif`. | **SIL Open Font License 1.1**. Font metadata credits **© 2017–2024 Adobe** and the upstream OFL file credits **Copyright 2012 Google Inc.** The [full OFL](docs/branding/fonts/OFL.txt) and [font credits](docs/branding/fonts/README.md) are retained. These fonts are website assets. |

The app's brand icon and DMG background are drawn by the project's Swift artwork scripts. They are not copies of an upstream converter's logo.

## 2. Experimental Rust core: third-party code dependencies

The following versions and license expressions are recorded by `core/Cargo.lock` and Cargo package metadata for this release. Dependencies are resolved by Cargo rather than vendored in this repository. This list includes optional, target-specific and build-time packages present in the lockfile; not every package is linked into every target.

### Direct dependencies

| Crate | Locked version | License expression | Use |
| --- | --- | --- | --- |
| [md-5](https://github.com/RustCrypto/hashes) | 0.10.6 | `MIT OR Apache-2.0` | Extended XMP digest verification |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.20.0 | `MIT OR Apache-2.0` | XML/XMP parsing |
| [serde_json](https://github.com/serde-rs/json) | 1.0.154 | `MIT OR Apache-2.0` | CLI JSON input/output |
| [sha2](https://github.com/RustCrypto/hashes) | 0.10.9 | `MIT OR Apache-2.0` | SHA-256 verification |

### Transitive and build dependencies

| Crate | Locked version | License expression | Scope |
| --- | --- | --- | --- |
| [block-buffer](https://github.com/RustCrypto/utils) | 0.10.4 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [cfg-if](https://github.com/rust-lang/cfg-if) | 1.0.5 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [cpufeatures](https://github.com/RustCrypto/utils) | 0.2.17 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [crypto-common](https://github.com/RustCrypto/traits) | 0.1.7 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [digest](https://github.com/RustCrypto/traits) | 0.10.7 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [generic-array](https://github.com/fizyk20/generic-array) | 0.14.7 | `MIT` | Transitive, build-time, optional or target-specific dependency |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.18 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [libc](https://github.com/rust-lang/libc) | 0.2.191 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | `Unlicense OR MIT` | Transitive, build-time, optional or target-specific dependency |
| [proc-macro2](https://github.com/dtolnay/proc-macro2) | 1.0.107 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [quote](https://github.com/dtolnay/quote) | 1.0.47 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [serde_core](https://github.com/serde-rs/serde) | 1.0.229 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [serde_derive](https://github.com/serde-rs/serde) | 1.0.229 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [syn](https://github.com/dtolnay/syn) | 3.0.9 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [typenum](https://github.com/paholg/typenum) | 1.20.1 | `MIT OR Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [unicode-ident](https://github.com/dtolnay/unicode-ident) | 1.0.27 | `(MIT OR Apache-2.0) AND Unicode-3.0` | Transitive, build-time, optional or target-specific dependency |
| [version_check](https://github.com/SergioBenitez/version_check) | 0.9.5 | `MIT/Apache-2.0` | Transitive, build-time, optional or target-specific dependency |
| [zmij](https://github.com/dtolnay/zmij) | 1.0.23 | `MIT` | Transitive, build-time, optional or target-specific dependency |

The `MIT/Apache-2.0` spelling is reproduced from the relevant package metadata. Each upstream crate's included license and notices remain authoritative.

## 3. Packaging, documentation, validation and release tools

| Project | Version / license | Actual use and distribution |
| --- | --- | --- |
| [ds_store](https://github.com/dmgbuild/ds_store) | 1.3.1; MIT | Writes Finder `.DS_Store` layout through `packaging/dmg_layout.py`; local DMG build dependency. |
| [mac_alias](https://github.com/dmgbuild/mac_alias) | 2.2.2; MIT | Creates the installer background's volume-relative alias; local DMG build dependency. |
| [Python-Markdown](https://github.com/Python-Markdown/markdown) | 3.9; BSD-3-Clause | Converts the project's manuals for GitHub Pages in `scripts/sync_website_guides.py`. |
| [importlib_metadata](https://github.com/python/importlib_metadata), [zipp](https://github.com/jaraco/zipp) | 8.7.1, Apache-2.0; 3.23.1, MIT | Transitive documentation-tool dependencies in the local Python 3.9 environment. |
| [Google libultrahdr](https://github.com/google/libultrahdr) | Upstream [LICENSE](https://github.com/google/libultrahdr/blob/main/LICENSE) includes MIT and Apache-2.0 notices. | External optional prerequisite for `tests/ProbeUltraHDR.cpp`; also an HDR metadata reference. The app does not compile or bundle the library. |
| [fontTools](https://github.com/fonttools/fonttools), [Brotli](https://github.com/google/brotli) | 4.60.1, MIT; 1.1.0, MIT | Local WOFF2 metadata inspection for this attribution inventory; their Python code is not included in the app or site. |
| [Swift](https://github.com/swiftlang/swift) | Upstream Apache-2.0 / runtime-exception terms; Apple SDK terms separately apply. | Native compilation; SDK command-line tools also provide signing, icon and disk-image utilities. |
| [Python](https://www.python.org/), [pip](https://github.com/pypa/pip) | PSF terms; pip MIT | Test-only format references, generated-fixture checks and local build/documentation tooling. |
| [Rust](https://github.com/rust-lang/rust), [Cargo](https://github.com/rust-lang/cargo) | MIT OR Apache-2.0 | Builds and checks the experimental Rust module. |
| [Git](https://git-scm.com/), [GitHub CLI](https://github.com/cli/cli) | Git GPL-2.0; GitHub CLI MIT | Version control, publishing and release asset management. |

The native app does not require users to install these Python packages, Git, Cargo, FFmpeg or ExifTool. Local tool installations, test outputs and private media are excluded from the distributed app and source.

## 4. Consulted implementations, format research and samples

These projects are acknowledged for the specific recorded reference work below. Their names here identify research or implementation comparisons, rather than additional app runtime dependencies.

| Project | Recorded reference / contribution | Upstream terms |
| --- | --- | --- |
| [MotionTrans](https://github.com/Huakira/MotionTrans) | `MediaDataOffset` and static-image separation behavior. | Historical source link; repository availability and license could not be reconfirmed in this audit. |
| [Live Photo Box](https://github.com/LengxiQwQ/live-photo-box) | Huawei `LIVE_` cover-frame fields, OPPO namespaces and vivo V2 video-length semantics. | [GPL v3 license](https://github.com/LengxiQwQ/live-photo-box/blob/master/LICENSE). |
| [ExifTool](https://exiftool.org/), [Samsung parser](https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Samsung.pm) | Samsung SEF entry layout and `mpv2` pointer interpretation. | Upstream's Perl licensing terms and file-level notices. |
| [doodspav/motionphoto](https://github.com/doodspav/motionphoto) | Samsung SEF directory fields and Google/Samsung encapsulation comparisons. | [Apache-2.0](https://github.com/doodspav/motionphoto/blob/master/LICENSE). |
| [Google libultrahdr](https://github.com/google/libultrahdr/blob/main/lib/src/gainmapmetadata.cpp) | ISO 21496-1 fractional metadata and gain-map parameter semantics; optional validation adapter described above. | Upstream MIT / Apache-2.0 notices. |
| [OpenHarmony multimedia_image_framework](https://github.com/openharmony/multimedia_image_framework) | HDR helpers, MPF parsing research and public fixtures for local comparison. | [Apache-2.0](https://github.com/openharmony/multimedia_image_framework/blob/master/LICENSE); individual assets retain their own terms. |
| [libheif](https://github.com/strukturag/libheif) | HEIF/HEIC container structures and cross-platform feasibility research. | See upstream [COPYING](https://github.com/strukturag/libheif/blob/master/COPYING) and component notices. |
| [MotionPhotoSamples](https://github.com/NightMean/MotionPhotoSamples) | Public Samsung Motion Photo fixtures for local compatibility checks. | No general media redistribution permission was confirmed. Sample files are not republished. |
| [dmgbuild](https://github.com/dmgbuild/dmgbuild) | Installer creation and Finder layout conventions. | [MIT](https://github.com/dmgbuild/dmgbuild/blob/main/LICENSE). |
| [Chromium DMG layout generator](https://chromium.googlesource.com/chromium/src/+/main/chrome/installer/mac/generate_dsstore.py) | Finder background, icon placement and `.DS_Store` layout field references. | Upstream [BSD-style license](https://chromium.googlesource.com/chromium/src/+/main/LICENSE) and file notices. |

### Evaluated alternatives for future cross-platform work

The development research also evaluated [Google libmotionphoto](https://github.com/google/libmotionphoto) (**Apache-2.0**), [PetrVys/MotionPhoto2](https://github.com/PetrVys/MotionPhoto2) (**MIT**) and [ysya/LiveMux](https://github.com/ysya/LiveMux) (**MIT**) for parser/encapsulation reuse and cross-platform feasibility. These remain evaluated references; the current native app does not integrate their code or executables. LiveMux records MotionPhoto2 ancestry, so those two are not treated as independent format evidence.

Public URLs and source-code licenses do not themselves grant rights to redistribute every referenced photograph or video. Local research copies and samples remain outside the published repository and installer; no personal sample filenames, device metadata or sample hashes are included in this inventory.

## 5. Design and review Skills

Thanks to the following authors and maintainers for development-time workflows used in this project. Skill instructions, local installations, preferences and intermediate audit/prototype records are kept outside the published software source and installer.

| Skill / project | Author / license | Use in MomentStays |
| --- | --- | --- |
| [huashu-design](https://github.com/alchaincyf/huashu-design) | [Huasheng / 花叔](https://github.com/alchaincyf); [MIT](https://github.com/alchaincyf/huashu-design/blob/master/LICENSE) | App prototype exploration, HTML interface prototypes and website design. |
| [frontend-design](https://github.com/anthropics/skills/tree/main/skills/frontend-design) | [Anthropic](https://github.com/anthropics); [Apache-2.0](https://github.com/anthropics/skills/blob/main/skills/frontend-design/LICENSE.txt) | Visual direction, hierarchy and implementation guidance for the interface. |
| [ui-ux-pro-max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill) | [NextLevelBuilder and contributors](https://github.com/nextlevelbuilder); [MIT](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill/blob/main/LICENSE) | Searchable design/UX guidance, interaction and layout checklists, including SwiftUI/platform considerations. |
| [ux-audit](https://github.com/paulunemoon/ux-audit-skill) | [Pauline Mila Alonso](https://github.com/paulunemoon); [MIT](https://github.com/paulunemoon/ux-audit-skill/blob/main/LICENSE) | Evidence-based UX review, severity/confidence framing and actionable accessibility/interaction findings. |

The UI/UX Pro Max catalog also credits [Google Fonts](https://github.com/google/fonts), [Phosphor Icons](https://github.com/phosphor-icons/core) and official platform/accessibility documentation as catalog sources. This is transitive catalog provenance; the native app uses system fonts and SF Symbols. The website's separately shipped Noto Serif SC subsets are credited in section 1.

## 6. Specifications, platform documentation and interoperability

- [Android Motion Photo specification](https://developer.android.com/media/platform/motion-photo-format): Google container/XMP fields and cover timestamp semantics.
- [Apple developer documentation](https://developer.apple.com/documentation/), [Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines/): Live Photo metadata, media timing, PhotoKit, macOS windows, menus and sharing APIs.
- [Adobe XMP](https://developer.adobe.com/xmp/) and [W3C RDF](https://www.w3.org/RDF/): metadata model and XML namespace conventions. [ISO BMFF / ISO/IEC 14496-12](https://www.iso.org/standard/85596.html), [HEIF / ISO/IEC 23008-12](https://www.iso.org/standard/89035.html) and [ISO 21496-1 gain-map metadata](https://www.iso.org/standard/86775.html) also inform format compatibility work.
- [W3C WCAG 2.2](https://www.w3.org/TR/WCAG22/): accessibility and reflow references used through the design/review workflows.
- [LocalSend](https://github.com/localsend/localsend): system-sharing ecosystem and interoperability research. Availability is controlled by the user's installed sharing services; LocalSend is not bundled or linked into the app.

Specifications and documentation remain subject to their publishers' terms. Acknowledgements do not imply endorsement by any upstream project, platform vendor or Skill author.

## 7. Binary and source distribution

The build copies `LICENSE`, `COPYRIGHT` and this file into the app's Resources directory. The DMG contains the signed app and an Applications link. Website font copyright and the complete OFL are also retained with the font files in the GitHub Pages branch.

A distributed GPL-covered binary must be accompanied by corresponding-source access as required by GPLv3 section 6. Release tags identify the corresponding project source and build/install scripts. Third-party dependencies and assets keep their own licenses; the project's GPL declaration does not relicense them, system frameworks, Skills or users' input media.
