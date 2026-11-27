# Third-party notices / 第三方说明

MomentStays's original source and documentation are **GPL-3.0-only**, copyright **2026 Lino**; the names and icon are covered by [TRADEMARKS.md](TRADEMARKS.md), not the GPL. Third-party materials keep their own copyrights and terms. See [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT). 中文致谢摘要见 [README.zh-CN.md](README.zh-CN.md#致谢)。

## 1. System dependencies and shipped assets

The native app uses Apple system frameworks (AppKit, SwiftUI, AVFoundation, ImageIO, PhotoKit and others), SF Symbols and system fonts. They are supplied by the operating system and are not bundled; Apple's terms apply. The Rust core is statically linked into the app; its dependencies are listed in section 2. The app icon and DMG background are drawn by the project's own Swift scripts.

| Component | Use | Terms |
| --- | --- | --- |
| [Noto Serif SC / Noto CJK](https://github.com/notofonts/noto-cjk) | Regular and bold WOFF2 subsets for the GitHub Pages website (`assets/fonts/ms-serif-*.woff2`, CSS alias `MS Serif`). Website only; not in the app. | **SIL Open Font License 1.1**; © 2017–2024 Adobe, Copyright 2012 Google Inc. Full [OFL](docs/branding/fonts/OFL.txt) and [font credits](docs/branding/fonts/README.md) are retained. |

### Linux system libraries (optional `linux-native` backend)

The Linux build of the Rust core (`cargo build --features linux-native`, off by default; macOS, iOS and Windows builds do not use it) calls these system libraries. They are **dynamically linked and not bundled**: in the GNOME Flatpak they are provided by the `org.gnome.Platform` runtime (HEVC/AAC decoders by `org.freedesktop.Platform.codecs-extra`), elsewhere by the distribution. No codec implementation is shipped by this project. Details: [core/LINUX_NATIVE.md](core/LINUX_NATIVE.md).

| Library | Use | License | Link |
| --- | --- | --- | --- |
| [libjpeg-turbo](https://libjpeg-turbo.org/) (TurboJPEG API) | JPEG decode/encode | BSD-3-Clause / IJG / Zlib | dynamic (`-lturbojpeg`) |
| [Little CMS 2](https://www.littlecms.com/) (`liblcms2` only, not the GPL fast_float/threaded plugins) | ICC → sRGB conversion, sRGB profile | MIT | dynamic (`-llcms2`) |
| [libheif](https://github.com/strukturag/libheif) | HEIC decode and gain-map auxiliary image | LGPL-3.0 | dynamic (`-lheif`), runtime-provided |
| [GStreamer](https://gstreamer.freedesktop.org/) core/base/app/video | Video frame extraction, audio decode check | LGPL-2.1 | dynamic via gstreamer-rs, runtime-provided |

## 2. Rust core: third-party code dependencies

`core/Cargo.lock` locks 84 third-party crates: 34 used by every build (10 direct, 24 transitive or build-time) and 50 used only by the optional Linux backend (`--features linux-native`, 4 direct, 46 transitive or build-time). Versions and license expressions below come from the lockfile and crates.io package metadata. Crates are resolved by Cargo, not vendored in this repository, and not every crate is linked into every target.

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

### Optional Linux backend (`linux-native` feature, Linux targets only)

Enabled only with `--features linux-native` on Linux; not compiled into the macOS, iOS or Windows apps. `cargo deny check licenses` runs with all features, so these crates are checked too.

| Crate | Locked version | License expression | Purpose |
| --- | --- | --- | --- |
| [gstreamer](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.4 | `MIT OR Apache-2.0` | GStreamer bindings: video frame and audio decode pipelines |
| [gstreamer-app](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.2 | `MIT OR Apache-2.0` | `appsrc`/`appsink` bindings for the frame pipeline |
| [gstreamer-video](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.4 | `MIT OR Apache-2.0` | video caps and frame mapping |
| [pkg-config](https://github.com/rust-lang/pkg-config-rs) | 0.3.34 | `MIT OR Apache-2.0` | build script: library versions and search paths |
| [atomic_refcell](https://github.com/mozilla/atomic_refcell) | 0.1.14 | `Apache-2.0 OR MIT` | runtime helper of gstreamer-rs / glib |
| [autocfg](https://github.com/cuviper/autocfg) | 1.5.1 | `Apache-2.0 OR MIT` | build-time (num-traits build script) |
| [bitflags](https://github.com/bitflags/bitflags) | 2.13.2 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [cfg-expr](https://github.com/EmbarkStudios/cfg-expr) | 0.20.11 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [either](https://github.com/rayon-rs/either) | 1.19.0 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [futures-channel](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [futures-core](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [futures-executor](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [futures-macro](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` | proc-macro used by gstreamer-rs / glib |
| [futures-sink](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [futures-task](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [futures-util](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [gio-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` | GLib/GObject bindings used by gstreamer-rs |
| [glib](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` | GLib/GObject bindings used by gstreamer-rs |
| [glib-macros](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` | GLib/GObject bindings used by gstreamer-rs |
| [glib-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` | GLib/GObject bindings used by gstreamer-rs |
| [gobject-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.9 | `MIT` | GLib/GObject bindings used by gstreamer-rs |
| [gstreamer-app-sys](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.4 | `MIT` | gstreamer-rs internals / FFI declarations |
| [gstreamer-base](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.4 | `MIT OR Apache-2.0` | gstreamer-rs internals / FFI declarations |
| [gstreamer-base-sys](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.4 | `MIT` | gstreamer-rs internals / FFI declarations |
| [gstreamer-sys](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.4 | `MIT` | gstreamer-rs internals / FFI declarations |
| [gstreamer-video-sys](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs) | 0.25.4 | `MIT` | gstreamer-rs internals / FFI declarations |
| [heck](https://github.com/withoutboats/heck) | 0.5.0 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [itertools](https://github.com/rust-itertools/itertools) | 0.15.0 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [kstring](https://github.com/cobalt-org/kstring) | 2.0.5 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [muldiv](https://github.com/sdroege/rust-muldiv) | 1.0.1 | `MIT` | runtime helper of gstreamer-rs / glib |
| [num-integer](https://github.com/rust-num/num-integer) | 0.1.47 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [num-rational](https://github.com/rust-num/num-rational) | 0.4.2 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [num-traits](https://github.com/rust-num/num-traits) | 0.2.19 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [option-operations](https://github.com/fengalin/option-operations) | 0.6.1 | `MIT/Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [pastey](https://github.com/as1100k/pastey) | 0.2.3 | `MIT OR Apache-2.0` | proc-macro used by gstreamer-rs / glib |
| [pin-project-lite](https://github.com/taiki-e/pin-project-lite) | 0.2.18 | `Apache-2.0 OR MIT` | runtime helper of gstreamer-rs / glib |
| [serde_spanned](https://github.com/toml-rs/toml) | 1.1.3 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [slab](https://github.com/tokio-rs/slab) | 0.4.12 | `MIT` | runtime helper of gstreamer-rs / glib |
| [smallvec](https://github.com/smallvec/smallvec) | 1.16.6 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [static_assertions](https://github.com/nvzqz/static-assertions-rs) | 1.1.0 | `MIT OR Apache-2.0` | runtime helper of gstreamer-rs / glib |
| [system-deps](https://github.com/gdesmott/system-deps) | 9.0.1 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [target-lexicon](https://github.com/bytecodealliance/target-lexicon) | 0.13.5 | `Apache-2.0 WITH LLVM-exception` | build-time (`*-sys` build scripts) |
| [toml](https://github.com/toml-rs/toml) | 1.2.3+spec-1.1.0 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [toml_datetime](https://github.com/toml-rs/toml) | 1.1.3+spec-1.1.0 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [toml_parser](https://github.com/toml-rs/toml) | 1.1.7+spec-1.1.0 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [toml_writer](https://github.com/toml-rs/toml) | 1.2.0+spec-1.1.0 | `MIT OR Apache-2.0` | build-time (`*-sys` build scripts) |
| [version-compare](https://gitlab.com/timvisee/version-compare) | 0.2.1 | `MIT` | build-time (`*-sys` build scripts) |
| [windows-link](https://github.com/microsoft/windows-rs) | 0.2.1 | `MIT OR Apache-2.0` | Windows-only dependency of gio-sys; locked but never built on Linux |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.61.2 | `MIT OR Apache-2.0` | Windows-only dependency of gio-sys; locked but never built on Linux |
| [winnow](https://github.com/winnow-rs/winnow) | 1.0.5 | `MIT` | build-time (`*-sys` build scripts) |

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
| [libheif](https://github.com/strukturag/libheif) | HEIF/HEIC container structure research. (The optional Linux backend also links the runtime's libheif dynamically; see section 1.) | See upstream [COPYING](https://github.com/strukturag/libheif/blob/master/COPYING). |
| [MotionPhotoSamples](https://github.com/NightMean/MotionPhotoSamples) | Public Samsung Motion Photo samples for local compatibility checks. | Sample files are not republished. |

Acknowledgements do not imply endorsement by any upstream project.

## 4. Binary and source distribution

The build copies `LICENSE`, `COPYRIGHT` and this file into the app's Resources directory. The DMG contains the signed app and an Applications link. Website font copyright and the complete OFL are also retained with the font files in the GitHub Pages branch.

GPL-3.0-only requires that distributed binaries come with access to the corresponding source. Each release tag identifies the corresponding project source and build/install scripts, published in this repository. Third-party dependencies and assets keep their own licenses; the project's GPL-3.0-only declaration does not relicense them, system frameworks or users' input media.

Dependency policy: third-party code must be under a permissive license compatible with app-store distribution (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unlicense, Unicode, MPL-2.0, CC0; OFL for fonts). GPL, AGPL, SSPL and bundled/statically linked LGPL code is not accepted. LGPL libraries are used only when supplied by the operating system or the Flatpak runtime and dynamically linked (see section 1). Rust crates are checked in CI with `cargo deny check licenses` against `core/deny.toml`; every new dependency is recorded in this file. See the 「依赖协议规则」 section of [AGENTS.md](AGENTS.md).

## 5. Software gain-map codec licence notices

The experimental OPPO UHDR path uses `jpeg-decoder` with default features disabled
and `oxideav-h265` (PCM encoder only) with `oxideav-core`. It does not link x265,
FFmpeg or libheif (the separate Linux backend's dynamic libheif use is covered in section 1). The added locked dependency graph contains no GPL/LGPL crate.
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

## 6. GNOME (Linux) app: `linux/` (`momentstays-gnome`)

The GNOME app ships only as a Flatpak on `org.gnome.Platform//50`. GTK 4, libadwaita, GLib/GIO, GdkPixbuf, Pango, Cairo, Graphene and the C library's gettext (`bindtextdomain`, `g_dgettext`) are LGPL system libraries supplied by the GNOME runtime and linked dynamically; none of them is bundled or statically linked. HEVC/AAC decoding, when the Linux codec backend is enabled, comes from Flathub's `org.freedesktop.Platform.codecs-extra` runtime extension, not from the app. The app icon is the project's own `docs/branding/icon-light.svg` (see [TRADEMARKS.md](TRADEMARKS.md)).

`linux/Cargo.lock` locks the core's crates (section 2) plus 60 further crates (3 direct, the rest transitive or build-time); `momentstays-core` and `serde_json` are reused from the core. All are MIT / Apache-2.0 (one `Apache-2.0 WITH LLVM-exception`) and are checked with `cargo deny check --config core/deny.toml licenses` in the `gnome-ui` workflow.

### Direct dependencies

| Crate | Locked version | License expression | Use |
| --- | --- | --- | --- |
| [glib-build-tools](https://github.com/gtk-rs/gtk-rs-core) | 0.22.8 | `MIT` | Build time: compiles the GResource bundle (CSS, icon) with the SDK’s glib-compile-resources |
| [gtk4](https://github.com/gtk-rs/gtk4-rs) | 0.11.5 | `MIT` | GTK 4 bindings (UI) |
| [libadwaita](https://gitlab.gnome.org/World/Rust/libadwaita-rs) | 0.9.2 | `MIT` | libadwaita bindings (UI) |

### Transitive and build dependencies

| Crate | Locked version | License expression |
| --- | --- | --- |
| [autocfg](https://github.com/cuviper/autocfg) | 1.5.1 | `Apache-2.0 OR MIT` |
| [bitflags](https://github.com/bitflags/bitflags) | 2.13.2 | `MIT OR Apache-2.0` |
| [cairo-rs](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [cairo-sys-rs](https://github.com/gtk-rs/gtk-rs-core) | 0.22.9 | `MIT` |
| [cfg-expr](https://github.com/EmbarkStudios/cfg-expr) | 0.20.11 | `MIT OR Apache-2.0` |
| [field-offset](https://github.com/Diggsey/rust-field-offset) | 0.3.6 | `MIT OR Apache-2.0` |
| [futures-channel](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [futures-core](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [futures-executor](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [futures-io](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [futures-macro](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [futures-task](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [futures-util](https://github.com/rust-lang/futures-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [gdk-pixbuf-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.9 | `MIT` |
| [gdk-pixbuf](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [gdk4-sys](https://github.com/gtk-rs/gtk4-rs) | 0.11.5 | `MIT` |
| [gdk4](https://github.com/gtk-rs/gtk4-rs) | 0.11.5 | `MIT` |
| [gio-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [gio](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [glib-macros](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [glib-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [glib](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [gobject-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.9 | `MIT` |
| [graphene-rs](https://github.com/gtk-rs/gtk-rs-core) | 0.22.8 | `MIT` |
| [graphene-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.9 | `MIT` |
| [gsk4-sys](https://github.com/gtk-rs/gtk4-rs) | 0.11.5 | `MIT` |
| [gsk4](https://github.com/gtk-rs/gtk4-rs) | 0.11.5 | `MIT` |
| [gtk4-macros](https://github.com/gtk-rs/gtk4-rs) | 0.11.5 | `MIT` |
| [gtk4-sys](https://github.com/gtk-rs/gtk4-rs) | 0.11.5 | `MIT` |
| [hashbrown](https://github.com/rust-lang/hashbrown) | 0.17.2 | `MIT OR Apache-2.0` (newer lock than core) |
| [heck](https://github.com/withoutboats/heck) | 0.5.0 | `MIT OR Apache-2.0` |
| [itoa](https://github.com/dtolnay/itoa) | 1.0.19 | `MIT OR Apache-2.0` (newer lock than core) |
| [libadwaita-sys](https://gitlab.gnome.org/World/Rust/libadwaita-rs) | 0.9.2 | `MIT` |
| [memoffset](https://github.com/Gilnaa/memoffset) | 0.9.1 | `MIT` |
| [pango-sys](https://github.com/gtk-rs/gtk-rs-core) | 0.22.11 | `MIT` |
| [pango](https://github.com/gtk-rs/gtk-rs-core) | 0.22.9 | `MIT` |
| [pin-project-lite](https://github.com/taiki-e/pin-project-lite) | 0.2.18 | `Apache-2.0 OR MIT` |
| [pkg-config](https://github.com/rust-lang/pkg-config-rs) | 0.3.34 | `MIT OR Apache-2.0` |
| [proc-macro-crate](https://github.com/bkchr/proc-macro-crate) | 3.5.0 | `MIT OR Apache-2.0` |
| [rustc_version](https://github.com/djc/rustc-version-rs) | 0.4.1 | `MIT OR Apache-2.0` |
| [semver](https://github.com/dtolnay/semver) | 1.0.28 | `MIT OR Apache-2.0` |
| [serde_spanned](https://github.com/toml-rs/toml) | 1.1.3 | `MIT OR Apache-2.0` |
| [slab](https://github.com/tokio-rs/slab) | 0.4.12 | `MIT` |
| [smallvec](https://github.com/smallvec/smallvec) | 1.16.6 | `MIT OR Apache-2.0` |
| [syn](https://github.com/dtolnay/syn) | 3.0.10 | `MIT OR Apache-2.0` (newer lock than core) |
| [system-deps](https://github.com/gdesmott/system-deps) | 7.0.8 | `MIT OR Apache-2.0` |
| [system-deps](https://github.com/gdesmott/system-deps) | 9.0.1 | `MIT OR Apache-2.0` |
| [target-lexicon](https://github.com/bytecodealliance/target-lexicon) | 0.13.5 | `Apache-2.0 WITH LLVM-exception` |
| [toml](https://github.com/toml-rs/toml) | 1.2.3+spec-1.1.0 | `MIT OR Apache-2.0` |
| [toml_datetime](https://github.com/toml-rs/toml) | 1.1.3+spec-1.1.0 | `MIT OR Apache-2.0` |
| [toml_edit](https://github.com/toml-rs/toml) | 0.25.21+spec-1.1.0 | `MIT OR Apache-2.0` |
| [toml_parser](https://github.com/toml-rs/toml) | 1.1.7+spec-1.1.0 | `MIT OR Apache-2.0` |
| [toml_writer](https://github.com/toml-rs/toml) | 1.2.0+spec-1.1.0 | `MIT OR Apache-2.0` |
| [version-compare](https://gitlab.com/timvisee/version-compare) | 0.2.1 | `MIT` |
| [windows-link](https://github.com/microsoft/windows-rs) | 0.2.1 | `MIT OR Apache-2.0` |
| [windows-sys](https://github.com/microsoft/windows-rs) | 0.61.2 | `MIT OR Apache-2.0` |
| [winnow](https://github.com/winnow-rs/winnow) | 1.0.5 | `MIT` |
