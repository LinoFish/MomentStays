# momentstays-core (Rust, experimental)

Cross-platform port of the MomentStays Motion Photo container parser. It lives in
`core/` and is not integrated into the macOS app.

- Byte-level only: JPEG markers, MPF, XMP / Extended XMP, ISO BMFF boxes, Google
  cover-time descriptors, Huawei/Honor `LIVE_`, Samsung SEF (JPEG + HEIC `mpvd`), OPPO, vivo.
- No pixel decode or encode, no AVFoundation. Video and audio bytes are never touched.
- Error messages match `native/PhotoFormat.swift` verbatim so results can be diffed.

## Build and use

```bash
cd core
cargo build --release
./target/release/momentstays-core inspect photo.jpg            # JSON summary
./target/release/momentstays-core extract photo.jpg --still s.jpg --video v.mp4
cargo test --release
```

Works on macOS, Linux and Windows.

## Parity

```bash
python3 scripts/check_rust_core.py                       # vs Python reference (any OS)
bash scripts/build_backend.sh                            # macOS: build the Swift test driver
python3 scripts/check_rust_core.py --swift .build/backend/native-tests --samples ~/MotionSamples
```

On macOS the script also compares `timestamp_us`, `ultra`, `coding_hash` and error
text with the Swift engine. Run it with real samples before relying on any module.

See [ROADMAP.md](ROADMAP.md) for what is ported and what is next.
