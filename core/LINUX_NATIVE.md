# Linux codec backend (`--features linux-native`)

`platform::SystemNative` on Linux, for the GNOME app (Flatpak on
`org.gnome.Platform//50`). Off by default: default, macOS and Windows builds
link no system libraries.

```sh
cargo test --features linux-native        # needs the -dev packages below
```

Ubuntu/Debian packages: `pkg-config libturbojpeg0-dev liblcms2-dev libheif-dev
libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-{base,good,bad}
gstreamer1.0-libav`; tests also use `ffmpeg` and `heif-enc` (`libheif-examples`).

## Libraries and licences

All system libraries are **dynamically** linked and, in the Flatpak, provided by
the runtime (HEVC/AAC decoders by the auto-installed
`org.freedesktop.Platform.codecs-extra`). No codec is bundled.

| Library | Used for | Licence | Link |
|---|---|---|---|
| libjpeg-turbo (TurboJPEG 2.x API, also in 3.x) | JPEG decode/encode | BSD-3-Clause / IJG / Zlib | dynamic, `extern "C"` |
| Little CMS 2 (`liblcms2` only, not its plugins) | sRGB ICC, ICC → sRGB | MIT | dynamic, `extern "C"` |
| libheif (≥ 1.16) | HEIC decode, gain-map aux image | LGPL-3.0 | dynamic, `extern "C"` |
| GStreamer 1.x core/base/app/video | video frames, audio decode | LGPL-2.1 | dynamic via gstreamer-rs |

`build.rs` uses pkg-config only for versions and search paths and emits exactly
`-lturbojpeg -llcms2 -lheif` (Debian's `lcms2.pc` also lists the GPL
fast_float/threaded plugins, which are never linked).

Rust crates added (optional, only with the feature): `gstreamer`,
`gstreamer-app`, `gstreamer-video` (MIT OR Apache-2.0) and their dependencies
(glib/gobject/gio-sys, gstreamer-*-sys: MIT; futures-*, num-*, smallvec,
itertools, kstring, …: MIT OR Apache-2.0; muldiv, slab, winnow: MIT;
target-lexicon: Apache-2.0 WITH LLVM-exception); build dependency
`pkg-config` (MIT OR Apache-2.0). No GPL/AGPL crate.

## Per method

| `Native` method | Linux implementation | Parity notes |
|---|---|---|
| `image_decodes` | JPEG: full entropy decode at 1/8 scale (TurboJPEG). HEIF: libheif decode of the primary. | Truncation ("premature end") = ImageIO *incomplete* → `Some(false)`; other libjpeg warnings pass, like ImageIO. Size limit 16384 as on Apple. **No HEVC/AV1 decoder → `None`** (unknown, never "damaged"). PNG/TIFF/GIF/WebP/BMP → `None`; other bytes → `Some(false)`. |
| `srgb_icc` | `cmsCreate_sRGBProfile`, saved as ICC v2.1 | Different bytes from Apple's profile, same colour space. |
| `decode_gray_jpeg` | TurboJPEG `TJPF_GRAY`, accurate IDCT, stride = width | `Ok(None)` unless the JPEG is one-component. IDCT rounding may differ by ±1 from AppleJPEG. |
| `decode_srgb_jpeg` | TurboJPEG RGB; embedded RGB/Gray ICC → sRGB with lcms2 (perceptual); RGBA, alpha 255 | Coded orientation (no EXIF rotation), as Apple. Untagged gray is replicated (Apple uses Generic Gray γ2.2). CMYK/YCCK refused (`Unsupported`). |
| `encode_gray_jpeg` | TurboJPEG gray, 1 component | Quality: libjpeg `round(q·100)`, clamped 1–100 (ImageIO's mapping is undocumented). |
| `reencode_jpeg` | JPEG: decode + 4:2:0 re-encode, carries APP1 EXIF, APP2 ICC, APP13 IPTC verbatim (XMP/MPF/vendor segments dropped, like an ImageIO properties re-encode). HEIC: libheif RGB (clap applied, irot/imir undone → coded axes), EXIF item → APP1 with Orientation from irot/imir, ICC → APP2. | ImageIO rewrites EXIF; here it is copied (incl. thumbnail). nclx-only HEIC colour is not converted. |
| `heic_gain_map` | libheif aux image `urn:com:apple:photo:2020:aux:hdrgainmap` of the primary; XMP metadata block → `HDRGainMapVersion`/`HDRGainMapHeadroom`; 8-bit mono plane | Coded (unrotated) axes like ImageIO aux data. Non-8-bit map → `image: None`. Declared map that cannot be read → error (never SDR). Verified on the public iPhone 15 Pro sample: version 131072, headroom 4.946382, 2016×1512. |
| `frame_jpeg` | Rust picks the sample (`platform::portable::frame_plan`); only the GOP from the preceding **IDR** (else `stss` sync) to the target is pushed through `appsrc ! decodebin ! videoconvert`; frame matched by PTS; track matrix applied in Rust; JPEG q=1.0 4:4:4 | Same timing rules as `apple_video.rs`: media time must lie in exactly one non-empty edit, frame = latest presentation start ≤ time (zero tolerance). Only 90°-multiple/mirror matrices (others refused). H.264/HEVC only (`avc1/avc3/hvc1/hev1`). HDR (HLG/PQ) frames are converted without tone mapping. |
| `audio_decodes` | Track count from the Rust MP4 reader; each `soun` track decoded `qtdemux ! decodebin ! audioconvert` to S16, frames counted | `Some(false)` for count mismatch, undecodable/unknown fourcc or zero frames; **missing decoder for a known codec → `None`** (engine reports "unavailable"). Limits 100 M frames / 30 s as on Apple. |
| `prepare_output_times` | default (no host metadata stage) | same as Apple backend. |

`memory_budget()` on Linux reads `/proc/meminfo` `MemTotal`, with the Apple
policy (memory / 8, ≥ 256 MiB; 1 GiB fallback). In a Flatpak this is the
host's memory (cgroup limits are not consulted).

## Degradation

Nothing panics on a missing codec. `tests/linux_native_missing.rs` disables
the HEVC/AAC GStreamer decoders and libheif plugins and checks:
`frame_jpeg` → `ErrorKind::Unsupported`, `audio_decodes` → `None`,
`image_decodes` on HEIC → `Some(true)` or `None`, never `Some(false)`.
