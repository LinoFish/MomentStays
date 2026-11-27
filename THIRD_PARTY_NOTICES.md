# Third-party notices / 第三方说明

MomentStays's original source and documentation are **GPL-3.0-only**, copyright **2026 Lino**; the names and icon are covered by [TRADEMARKS.md](TRADEMARKS.md), not the GPL. Third-party materials keep their own copyrights and terms. See [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT). 中文致谢摘要见 [README.zh-CN.md](README.zh-CN.md#致谢)。

## 1. System dependencies and shipped assets

The native app uses Apple system frameworks (AppKit, SwiftUI, AVFoundation, ImageIO, PhotoKit and others), SF Symbols and system fonts. They are supplied by the operating system and are not bundled; Apple's terms apply. The Rust core is statically linked into the app; its dependencies are listed in section 2. The app icon and DMG background are drawn by the project's own Swift scripts.

| Component | Use | Terms |
| --- | --- | --- |
| [Noto Serif SC / Noto CJK](https://github.com/notofonts/noto-cjk) | Regular and bold WOFF2 subsets for the GitHub Pages website (`assets/fonts/ms-serif-*.woff2`, CSS alias `MS Serif`). Website only; not in the app. | **SIL Open Font License 1.1**; © 2017–2024 Adobe, Copyright 2012 Google Inc. Full [OFL](docs/branding/fonts/OFL.txt) and [font credits](docs/branding/fonts/README.md) are retained. |

## 2. Rust core: third-party code dependencies

`core/Cargo.lock` locks 34 third-party crates (10 direct, 24 transitive or build-time); versions and license expressions below come from the lockfile and package metadata. Crates are resolved by Cargo, not vendored in this repository, and not every crate is linked into every target.

### Direct dependencies

| Crate | Locked version | License expression |
| --- | --- | --- |
| [base64](https://github.com/marshallpierce/rust-base64) | 0.22.1 | `MIT OR Apache-2.0` |
| [jpeg-decoder](https://github.com/image-rs/jpeg-decoder) | 0.3.2 | `MIT OR Apache-2.0` |
| [oxideav-h265](https://github.com/OxideAV/oxideav-h265) | 0.0.17 | `MIT` |
| [libc](https://github.com/rust-lang/libc) | 0.2.191 | `MIT OR Apache-2.0` |
| [md-5](https://github.com/RustCrypto/hashes) | 0.10.6 | `MIT OR Apache-2.0` |
| [memchr](https://github.com/BurntSushi/memchr) | 2.8.3 | `Unlicense OR MIT` |
| [memmap2](https://github.com/RazrFalcon/memmap2-rs) | 0.9.12 | `MIT OR Apache-2.0` |
| [serde_json](https://github.com/serde-rs/json) | 1.0.154 | `MIT OR Apache-2.0` |
| [serde](https://github.com/serde-rs/serde) | 1.0.229 | `MIT OR Apache-2.0` |
| [sha2](https://github.com/RustCrypto/hashes) | 0.10.9 | `MIT OR Apache-2.0` |

### Transitive and build dependencies

| Crate | Locked version | License expression |
| --- | --- | --- |
| [block-buffer](https://github.com/RustCrypto/utils) | 0.10.4 | `MIT OR Apache-2.0` |
| [bytemuck](https://github.com/Lokathor/bytemuck) | 1.25.2 | `Zlib OR Apache-2.0 OR MIT` |
| [oxideav-core](https://github.com/OxideAV/oxideav-core) | 0.1.43 | `MIT` |
| [thiserror](https://github.com/dtolnay/thiserror) / thiserror-impl | 2.0.21 | `MIT OR Apache-2.0` |
| [cfg-if](https://github.com/rust-lang/cfg-if) | 1.0.5 | `MIT OR Apache-2.0` |
| [cpufeatures](https://github.com/RustCrypto/utils) | 0.2.17 | `MIT OR Apache-2.0` |
| [crypto-common](https://github.com/RustCrypto/traits) | 0.1.7 | `MIT OR Apache-2.0` |
| [digest](https://github.com/RustCrypto/traits) | 0.10.7 | `MIT OR Apache-2.0` |
| [equivalent](https://github.com/indexmap-rs/equivalent) | 1.0.3 | `Apache-2.0 OR MIT` |
| [foldhash](https://github.com/orlp/foldhash) | 0.2.0 | `Zlib` |
| [generic-array](https://github.com/fizyk20/generic-array.git) | 0.14.7 | `MIT` |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.17.1 | `MIT OR Apache-2.0` |
| [indexmap](https://github.com/indexmap-rs/indexmap) | 2.14.2 | `Apache-2.0 OR MIT` |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.18 | `MIT OR Apache-2.0` |
| [proc-macro2](https://github.com/dtolnay/proc-macro2) | 1.0.107 | `MIT OR Apache-2.0` |
| [quote](https://github.com/dtolnay/quote) | 1.0.47 | `MIT OR Apache-2.0` |
| [serde_core](https://github.com/serde-rs/serde) | 1.0.229 | `MIT OR Apache-2.0` |
| [serde_derive](https://github.com/serde-rs/serde) | 1.0.229 | `MIT OR Apache-2.0` |
| [syn](https://github.com/dtolnay/syn) | 3.0.9 | `MIT OR Apache-2.0` |
| [typenum](https://github.com/paholg/typenum) | 1.20.1 | `MIT OR Apache-2.0` |
| [unicode-ident](https://github.com/dtolnay/unicode-ident) | 1.0.27 | `(MIT OR Apache-2.0) AND Unicode-3.0` |
| [version_check](https://github.com/SergioBenitez/version_check) | 0.9.5 | `MIT/Apache-2.0` |
| [zmij](https://github.com/dtolnay/zmij) | 1.0.23 | `MIT` |

Each upstream crate's own license file and notices remain authoritative.

## 3. Format research and test samples

Thanks to these projects for format research. Their code is not linked into or bundled with the app.

| Project | Contribution | Upstream terms |
| --- | --- | --- |
| [MotionTrans](https://github.com/Huakira/MotionTrans) | `MediaDataOffset` and static-image separation behavior. | See upstream repository. |
| [Live Photo Box](https://github.com/LengxiQwQ/live-photo-box) | Huawei `LIVE_` cover-frame fields, OPPO namespaces and vivo V2 video-length semantics. | [GPL-3.0](https://github.com/LengxiQwQ/live-photo-box/blob/master/LICENSE); used as a format reference only; its code is not linked or bundled. |
| [ExifTool](https://exiftool.org/) ([Samsung parser](https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Samsung.pm)) | Samsung SEF entry layout and `mpv2` pointer interpretation. | Upstream Perl licensing terms. |
| [doodspav/motionphoto](https://github.com/doodspav/motionphoto) | Samsung SEF directory fields and Google/Samsung encapsulation comparisons. | [Apache-2.0](https://github.com/doodspav/motionphoto/blob/master/LICENSE). |
| [OpenHarmony multimedia_image_framework](https://github.com/openharmony/multimedia_image_framework) | HDR helpers and MPF parsing research. | [Apache-2.0](https://github.com/openharmony/multimedia_image_framework/blob/master/LICENSE). |
| [Google libultrahdr](https://github.com/google/libultrahdr) | ISO 21496-1 gain-map metadata semantics; optional external prerequisite for the test-only `apple/shared/tests/ProbeUltraHDR.cpp`, not compiled into the app. | Upstream MIT / Apache-2.0 notices. |
| [libheif](https://github.com/strukturag/libheif) | HEIF/HEIC container structure research. | See upstream [COPYING](https://github.com/strukturag/libheif/blob/master/COPYING). |
| [MotionPhotoSamples](https://github.com/NightMean/MotionPhotoSamples) | Public Samsung Motion Photo samples for local compatibility checks. | Sample files are not republished. |

Acknowledgements do not imply endorsement by any upstream project.

## 4. Binary and source distribution

The build copies `LICENSE`, `COPYRIGHT` and this file into the app's Resources directory. The DMG contains the signed app and an Applications link. Website font copyright and the complete OFL are also retained with the font files in the GitHub Pages branch.

GPL-3.0-only requires that distributed binaries come with access to the corresponding source. Each release tag identifies the corresponding project source and build/install scripts, published in this repository. Third-party dependencies and assets keep their own licenses; the project's GPL-3.0-only declaration does not relicense them, system frameworks or users' input media.

Dependency policy: third-party code must be under a permissive license compatible with app-store distribution (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unlicense, Unicode, MPL-2.0, CC0; OFL for fonts). GPL, AGPL, SSPL and bundled/statically linked LGPL code is not accepted. Rust crates are checked in CI with `cargo deny check licenses` against `core/deny.toml`; every new dependency is recorded in this file. See the 「依赖协议规则」 section of [AGENTS.md](AGENTS.md).

## 5. Software gain-map codec licence notices

The experimental OPPO UHDR path uses `jpeg-decoder` with default features disabled
and `oxideav-h265` (PCM encoder only) with `oxideav-core`. It does not link x265,
FFmpeg or libheif. The added locked dependency graph contains no GPL/LGPL crate.
These permissive copyright licences do not grant or settle HEVC patent rights.
The following notices are included in the app through this file.

### oxideav-h265 0.0.17 / oxideav-core 0.1.43

```text
MIT License

Copyright (c) 2026 Karpelès Lab Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### jpeg-decoder 0.3.2

```text
MIT License

Copyright (c) 2016 The jpeg-decoder Developers

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### bytemuck 1.25.2

```text
MIT License

Copyright (c) 2019 Daniel "Lokathor" Gee.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice (including the next paragraph) shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### thiserror / thiserror-impl 2.0.21

```text
Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```
