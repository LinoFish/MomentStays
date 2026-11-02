# momentstays-core (Rust)

Cross-platform Motion Photo / Live Photo engine for MomentStays: parser, vendor
converters and Live ↔ Motion assembly. Rust is the only implementation.

- Byte-level only: JPEG markers, MPF, XMP / Extended XMP, ISO BMFF boxes, Google
  cover-time descriptors, Huawei/Honor `LIVE_`, Samsung SEF (JPEG + HEIC `mpvd`), OPPO, vivo.
- No pixel decode or encode, no AVFoundation. Video and audio bytes are never touched.
- Error texts are stable and covered by tests; they were originally taken verbatim from the former Swift engine.
- Every error also has a kind (`Error::kind()`, `error_kind` in JSON): `not_motion_photo`, `malformed`, `unsupported`, `cover`, `verify_failed`, `io`, `invalid_argument`. Branch on the kind, not the text.

## Build and use

```bash
cd core
cargo build --release
./target/release/momentstays-core inspect photo.jpg            # JSON summary
./target/release/momentstays-core inspect -j 8 ~/Photos/       # folder (recursive, JPEG/HEIF) in parallel, JSON Lines
./target/release/momentstays-core bench                        # synthetic throughput (or pass files)
./target/release/momentstays-core extract photo.jpg --still s.jpg --video v.mp4
cargo test --release
```

Works on macOS, Linux and Windows; Android builds via `android/build.sh`. Inputs are memory-mapped,
never copied whole.

## Parity fixtures (`tests/pycli.rs`)

`core/tests/fixtures-pycli/` holds base64 fixtures and expected outputs that were generated
once from the former Python reference (`motionlive`, since removed). The tests need no Python
at run time; they pin parse results, `live_still` / `ultra_from_apple` / `legacy_color_profile`
bytes and `legacy_apple` stops/gain bytes.

## Android writers (`model`, JPEG path)

`model::MotionModel` is the neutral model: still JPEG bytes, MP4 bytes and a
cover (`Time(us)` or Huawei `Frame`), built from any file `format::parse`
understands. `model::write` writes it back out for Google, Samsung SEF,
Huawei LIVE_, OPPO or vivo JPEG, reusing the Swift `AndroidExport` layout
(XMP motion document, SEF directory, LIVE_ tail). Video bytes are copied,
never re-encoded; the still keeps its coding data and gain map (only XMP
segments are rewritten, MPF offsets repaired). Every write is validated by
parsing it back: video bytes, still coding hash and cover must match.

Refusals, not guesses: a Huawei target needs a frame cover and other targets
need a time cover; converting between the two needs the MP4 sample-table
reader (ROADMAP item 7) and is rejected for now. HEIC output is implemented
for Google/Samsung targets (`heifwrite::assemble_heic`, ROADMAP A3);
other HEIC targets are refused, and MOV->MP4 remux lives in `live2motion`
(item 8), not in `model`.

## MP4 -> MOV remux (`remux`, ROADMAP item 9a)

`remux::remux_to_mov(data, video_index)` rebuilds an MP4 as a QuickTime MOV
without re-encoding: the chosen video track plus all audio tracks are kept,
other tracks (Google mett, hint, text) are dropped. Sample bytes are copied
verbatim; stts/stsc/stsz/ctts/stss boxes are copied verbatim, stco/co64 is
rebuilt for the new mdat layout, mvhd/tkhd/mdhd are rebuilt (movie timescale
1_000_000, track timescales preserved), and the edit list is copied with
segment durations rescaled to the movie timescale, so the output playback
timeline matches the source. `remux::check_remux` validates a remux the way
Swift `movie()` does: per kept track, sample count, sizes, concatenated
sample hash and presentation times must match exactly.

Limits: negative presentation times are refused (no explicit edit-list
handling for them, same rule as Swift). The movie-level content identifier
(item 9b first half) is implemented: `remux_to_mov_with_identifier` writes
moov/udta/meta/ilst (UTF-8 `----` item), `read_content_identifier` reads it
back, and ffprobe independently reports the tag. The still-image-time
track (item 9b second half) is implemented in the native iPhone form
(`remux_to_mov_with_identifier_and_mett`): a `meta`-handler track with a
`mebx` sample entry carrying the `mdta/com.apple.quicktime.still-image-time`
key, one 9-byte sample (value -1), mdhd timescale 600, and an edit list
whose empty edit runs up to the cover point. The stsd/sample bytes are
copied verbatim from MIT-licensed public samples (see `tests/SOURCES.md`);
no AVAssetWriter-style `mett` entry exists in any verified public sample,
so that form is NOT replicated — comparing against Swift `movie()` output
and confirming Photos recognition remain Needs Mac check items. Verified
on Linux: 5 remux tests (synthetic video+audio, mett-track drop,
negative-PTS refusal, second video selection, real ffmpeg H.264 file)
plus ffprobe on the real-file output (pts/dts/sizes identical to source,
decodes cleanly), 3 identifier tests, and 5 still-image-time tests
(writer round-trip incl. cover 0 and negative-cover refusal, native
1-key/3-key reader forms, other-meta-track disambiguation) plus
`mett_cover_time` verified against the original public iPhone files
(1,233,333 us and 1,260,000 us covers).

## Live -> Motion assembly (`live2motion`, ROADMAP item 8)

`live2motion::assemble` takes an Apple Live Photo pair (JPEG still + MOV)
and a target vendor plus cover, and returns a vendor Motion Photo JPEG:
MOV->MP4 sample-copy remux (`remux::remux_to_mp4`: exactly one video and
at most one audio track, mett dropped, ftyp `mp42`), JPEG still cleaning
(`pymotion::live_still_clean`: Swift `liveStill` with updateIdentifier
false — static-only truncation, motion XMP strip, EXIF untouched,
gain-map bytes verified), then `model::write` with its parse-back
validation. `mett_cover_time` reads the cover from the marker track's
sample timing through the edit list (accepting Swift `mett` entries and
native `mebx` entries identified by their still-image-time key; the
sample value itself is not checked — native files write -1, Swift
`movie()` writes 0, and Swift's reader accepts {-1, 0, 255}). Limits: HEIC stills are
refused (HEIC live_still is not implemented); the vendor cover kind must
match (`Cover::Frame` for Huawei, `Cover::Time` otherwise — convert with
`cover.rs` first). Verified on Linux: 8 tests plus ffprobe on the
real-file MP4 (pts/dts/sizes identical to source, decodes cleanly).

## Motion -> Live Photo pair (`live`, `motion-to-live`)

`live::from_motion` turns any Motion Photo `format::parse` accepts (JPEG or
HEIC, every vendor) into a complete Live Photo pair, and
`momentstays-convert motion-to-live <file> -o <dir>` writes it as
`<dir>/<stem>.jpg|.heic` + `<dir>/<stem>.mov`:

- still: static-only, motion XMP removed, a new ContentIdentifier in the Apple
  MakerNote (tag 17); coding data must hash the same as the source;
- movie: sample-copy remux of the primary video track (`--video-index` picks
  another) plus audio, the same identifier in `moov/udta`, and a
  still-image-time track whose marker sits on the cover frame;
- cover: the recorded time is matched to a real frame on the media timeline
  and on the presentation (edit-list) timeline. If they name different frames
  the conversion is refused; `--time-domain media|presentation` picks one.
  Huawei frame numbers are used as is; no midpoint fallback.

Before anything is written, the pair is read back: still identifier and
coding hash, movie identifier, marker time, and the marker landing on the same
frame in the output timeline. Both files are written and verified before
either replaces a target. Not covered: converting legacy Apple HDR gain maps
(needs pixel work); Ultra HDR bytes are kept. Photos import itself can only be
checked on a Mac.

## Cross-platform convert CLI (`momentstays-convert`, ROADMAP item 11)

```bash
cargo build --release --bin momentstays-convert
./target/release/momentstays-convert vendor in.jpg out.jpg --to samsung
./target/release/momentstays-convert to-live video.mp4 --uuid <uuid> -o out.mov
./target/release/momentstays-convert to-motion still.jpg movie.mov --to google --cover-time 800000 -o out.jpg
```

Outputs are written safely (`output::write_atomic`): a temp file next to
the target, checked from the bytes on disk (format read-back for Motion
Photos, sample-by-sample for remuxed video), then renamed into place. A
failed run never leaves a partial file or touches an existing one, and an
output path that names an input is refused. Vendor conversion streams the
video from the memory-mapped input instead of copying it.

This binary has no platform dependency: vendor conversion, Motion->MOV remux and Live->Motion assembly
all run on the Rust core, on any OS. `vendor` takes the cover from the
input unless `--cover-time` / `--cover-frame` overrides it, converting
Time<->Frame via the video sample tables when the target needs the other
form (refused, never guessed, when out of tolerance). `to-motion` also
accepts `--cover-from-mett` (marker timing from the MOV's marker track;
the marker value itself is not checked — native -1 and Swift 0 are both
accepted). `to-live` writes the content identifier and, with
`--cover-time-us <us>`, the native-form still-image-time track; without
that flag it warns that the marker is missing. Vendor
aliases: `xiaomi` and `honor` use the Google-compatible layout. HEIC
inputs are supported for Google/Samsung targets (`heifwrite`); other
HEIC targets are refused.

See [ROADMAP.md](ROADMAP.md) for what is ported and what is next.
