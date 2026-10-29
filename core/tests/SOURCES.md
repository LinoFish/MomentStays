# Public sample registry (ROADMAP: samples used for verification)

Rule: only samples with a clear source and a permissive license
(public domain, CC0, MIT/Apache/BSD, or vendor documentation samples)
are used. Unlicensed samples are never relied on. Samples are test
data only; they never enter product logic. Large sample files are
NOT committed to this repo; only the small byte excerpts a test
actually needs are embedded in the test files, with the source file's
SHA-256 below so the excerpt can be re-verified against the original.

## Still-image-time track (ROADMAP item 9b2)

Terminology correction, verified on all files below: no real iPhone
Live Photo MOV contains a `mett` sample entry or an `mdta`-handler
track. The native form is a `meta`-handler track with a `mebx`
sample entry whose `keys` box carries `mdta` /
`com.apple.quicktime.still-image-time` (plus, in camera files,
transform and reference-dimensions keys). The Rust writer replicates
this native `mebx` form; an AVAssetWriter-style `mett` entry has no
public sample and is therefore NOT replicated (see ROADMAP Needs Mac
check for the remaining Mac hex-dump step).

### 1. osxphotos iPhone Live Photo (MIT) — primary source

- Source repo: https://github.com/RhetTbull/osxphotos (LICENSE: MIT,
  verified at https://github.com/RhetTbull/osxphotos/blob/main/LICENSE)
- File: `tests/Test-Live-15.7.2.photoslibrary/originals/C/C3090F66-942C-41D3-BEC7-4F2B4876A109_3.mov`
  (raw: https://raw.githubusercontent.com/RhetTbull/osxphotos/main/tests/Test-Live-15.7.2.photoslibrary/originals/C/C3090F66-942C-41D3-BEC7-4F2B4876A109_3.mov)
- Size 4,215,771 bytes,
  SHA-256 `0f3a840ee174c127a463ad59b55123a88d66392202dcc3be0e6aca9f2f56b5fd`
- Background (per the Tika TIKA-2861 discussion): a real iPhone 15 Pro /
  iOS 18.5 Live Photo pair.
- Used for: the still track's 3-key `mebx` stsd entry (293 bytes) and
  105-byte sample embedded in `tests/mett.rs` (`STILL_3KEY`,
  `STILL_3KEY_SAMPLE`); the edit-list cover mapping (empty edit
  740/600 s = 1,233,333 us, verified by `mett_cover_time` on the
  original file); the `gmhd`/`gmin` media header bytes and the tkhd
  flags (0x0f) the writer copies.
- Same-source corroboration (not independent sources): two more
  osxphotos files with byte-identical still-track stsd entries and
  the same 740/600 s empty edit:
  `tests/Test-Live-15.7.2.photoslibrary/originals/A/A0002D0F-7D4E-40F5-BA53-8A11096B1E6B.mov`
  (SHA-256 `1c3713d59d866810e512b858d937abf85cfb8f950942d57b4b6f85359c003b1d`)
  and
  `tests/Test-Media-Types-15.7.2.photoslibrary/originals/9/9CD04458-A18F-4F92-8361-0AE85A72FAFF_3.mov`
  (SHA-256 `025058984b43d8dab4108772df059c8f85bd4326cd6bcc099598a6df410c1854`).

### 2. logseq-livephotos-plugin demo (MIT) — second independent source

- Source repo: https://github.com/zhaochunqi/logseq-livephotos-plugin
  (LICENSE: MIT, verified from the repo's LICENSE file)
- File: https://cdn.jsdelivr.net/gh/zhaochunqi/logseq-livephotos-plugin@main/resources/20260118-QnJYmcw1I-1.mov
- Size 3,036,089 bytes,
  SHA-256 `5cd548c5d5c9f977611ecfbcabe50102d23ac5f2d4615e5d74b52a3c8189ef59`
- Used for: the 1-key `mebx` stsd entry (96 bytes) and 9-byte sample
  (`00000009 00000001 ff`) that the writer emits verbatim
  (`remux.rs` `METT_STSD_ENTRY` / `METT_SAMPLE`, cross-checked by
  `tests/mett.rs` `METT_1KEY` / `STILL_SAMPLE`).
- Caveat: this file is remuxed (moov first, 1.6 s video, still-track
  timescale 44100, no empty edit), so its still-track TIMING is not
  trusted — only its stsd/sample byte form, which matches source 1's
  first key exactly. `mett_cover_time` on this file returns 0 us,
  which is what its own edit list says, not a native cover point.

### Corroborating only (NOT relied on — no license found)

- `https://cdn.makzan.net/IMG_7677.mov` (personal CDN file, no license
  statement): its still track's 1-key stsd entry and 9-byte sample are
  byte-identical to source 2, and its edit list is native-form
  (empty edit 756/600 s = 1,260,000 us, verified by `mett_cover_time`
  on the file). Recorded here as corroboration only; no committed
  byte depends on it, since sources 1 and 2 already pin the same
  bytes under MIT.

### Excluded candidates (checked, not used)

- `https://zeitkapsl.eu/videos/live_photo_demo.mov`: one `vide` track
  only, no timed-metadata track, no license statement.
- Tumblr demo `tumblr_o63c4ekE1X1twn7m0o1.mov`: link dead
  (14-byte `404 Not Found`); media license unclear anyway.
- Apache Tika synthetic fixtures (`testMP4_QuickTimeMetadata.mov`
  etc.): hand-built minimal containers with only content.identifier /
  location boxes, no still-image-time track.
- Wikimedia / Archive.org / vendor documentation searches: no
  downloadable native Live Photo MOV found.

## HEIC samples

Verified locally 2026-10-28 (parse + writer round-trip + `live_still`,
harness in /tmp, not committed): the files are 1.6-6.5 MB each, so they
are registered here but NOT committed to the repo (a .b64 copy would be
~2-9 MB each). The code paths they exercise are covered in-repo by
synthetic fixtures plus two regression tests derived from them
(`parse_largesize_mpvd_google` in `tests/heic.rs`,
`still_with_identifier_keeps_long8_makernote_fields` in
`tests/live2motion.rs`).

- Samsung HEIC, mpvd form (Galaxy S22 Ultra photo, MIT): source repo
  https://github.com/g0ddest/sm_motion_photo (LICENSE MIT, Copyright
  (c) 2020 Vitalii Velicodnii), file
  https://raw.githubusercontent.com/g0ddest/sm_motion_photo/master/tests/data/photo-sg22-ultra.heic
  (2,849,914 bytes, SHA-256
  3dfb0d488cd885652cf49abe846655bf50be27da6eec342d58d90d738e378752).
  Top-level ftyp/mdat/meta/mpvd (32-bit size, to EOF); XMP
  MotionPhoto=1, timestamp 2,990,844 us; EXIF samsung / SM-S9080.
  Parses as `samsung-heic-sef` (video 507,827..2,849,429, 2,341,602
  bytes, ftyp-led; the trailing 485-byte `sefd` box inside the mpvd
  payload is excluded by the SEF range); the Samsung writer round-trip
  re-parses with identical video bytes, coding hash and timestamp.
  Caveat: Samsung camera XMP does NOT follow Google semantics -
  Primary Padding=67 (not the 8-byte mpvd header) and video
  Length=104 (not the real video length). The parser locates video via
  SEF, not XMP Length, so it is unaffected; the writer follows Swift
  `xmp(heic:true)` (Padding 8, Length = video length), which re-parses
  cleanly. Whether Samsung Gallery accepts the Swift-style XMP is a
  Needs Mac check (device check, really).
- Samsung HEIC, sefd form (Galaxy S20 photo, same MIT repo):
  https://raw.githubusercontent.com/g0ddest/sm_motion_photo/master/tests/data/photo.heic
  (5,755,343 bytes, SHA-256
  f2dbe60e0402479f3c0d4246f30756fc73a416cdd9c64509c9767b070e22660e).
  Top-level ftyp/mdat/meta/sefd, NO mpvd; the SEFT trailer's
  MotionPhoto_Data holds the MP4; EXIF samsung / SM-G981U1. NOT parsed
  (no mpvd) - this documents that Samsung HEIC has two encapsulations
  and the core currently supports only the mpvd form. Out of scope for
  A3 (mpvd only); sefd support is future work.
- Google HEIC (libmphoto official fixture, Apache-2.0): source repo
  https://github.com/googleinterns/libmphoto (LICENSE Apache-2.0),
  motion file
  https://raw.githubusercontent.com/googleinterns/libmphoto/master/sample_data/heic_motion_photo/motion_photo.heic
  (3,127,074 bytes, SHA-256
  807defcdc85ca2b4e8ee4f1bcffbbce80809db836eed6c59eddb8c39f6323cee)
  and paired still
  https://raw.githubusercontent.com/googleinterns/libmphoto/master/sample_data/heic_motion_photo/still.heic
  (1,582,857 bytes, SHA-256
  0abff0df3b7a3d23863b35eecdd16a8d6ede65e0038c5682732d450c732cdf8b).
  Motion = still + mpvd; the mpvd uses largesize form (size field 1 +
  64-bit length 1,544,217), XMP Primary image/heic Length=0 Padding=16
  (= largesize header), video Length=1,544,201 (= total - 16),
  timestamp 0. Parses as `google-heic` (video 1,582,873..3,127,074 =
  mpvd + 16); the Google writer round-trip re-parses with identical
  video bytes, coding hash and timestamp. This file forced two parser
  fixes: largesize mpvd headers (the video start was hard-coded to
  mpvd + 8) and the Exif gate (the fixture's still has NO Exif item;
  Exif is now optional for HEIC motion photos - layout + XMP + video
  checks suffice).
- Apple Live Photo JPEG still (iPhone 15 Pro Max photo, CC0): Wikimedia
  Commons, File:Sturgeon's Mill - August 2025 - Sarah Stierch 31.jpg,
  author Missvain, LicenseShortName CC0,
  https://upload.wikimedia.org/wikipedia/commons/3/3f/Sturgeon%27s_Mill_-_August_2025_-_Sarah_Stierch_31.jpg
  (6,521,985 bytes, SHA-256
  a5e142b2d750992e3d691be07b061e509382fc37a6af4bbdf05e8c41f1e9a168).
  EXIF Make Apple, Model iPhone 15 Pro Max; MakerNote (1,745 bytes, 53
  entries) tag 0x0011 (decimal 17, ContentIdentifier) =
  9C72AB1C-2837-4690-AFF8-AF11AF9F5A36, tag 0x0017 (decimal 23,
  LivePhotoVideoIndex) present; no XMP segment.
  `live_still_with_identifier` replaces tag 17 with the new uuid, keeps
  tag 23, keeps Make/Model, keeps coding bytes, keeps SOI/EOI; the
  EXIF segment grows (new MakerNote + rebuilt IFDs appended, the
  Swift-parity strategy). The MakerNote uses LONG8 (type 16) for tags
  0x0017 and 0x0025 - this forced the `maker_note` type-table fix
  (LONG8/IFD8 sizes added). Limitation: Commons has the still only,
  no paired MOV, so the ContentIdentifier cannot be cross-checked
  against video-side content.identifier.
