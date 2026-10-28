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

Pending: registered with the HEIC writer milestone (ROADMAP A3),
not yet added.
