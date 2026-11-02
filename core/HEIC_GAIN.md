# Apple HEIC gain-map reading

`heic_gain::read(&bytes)` reads HEIC auxiliary pixels directly from Rust using
ImageIO and CoreFoundation. No Swift code or callback is used. It returns Apple
HDR version/headroom metadata and owned 8-bit gray pixels with the original row
stride. It neither re-encodes pixels nor converts HDR parameters.

The backend requires Apple ImageIO (macOS 11+ / iOS 14.1+). Other platforms return
an unsupported-backend error. This is Rust-owned system API integration, not a
portable software HEVC decoder.

Container validation happens before decoding. Invalid dimensions, short buffers,
overflow, excessive output (>256 MiB), and unavailable declared gain maps are
errors. Unsupported pixel formats return `image: None`; callers must reject or
preserve them. `Ok(None)` means no Apple map was exposed, **not** proof of SDR.
Callers must retain HDR/ISO `tmap`, MakerNote/headroom and version eligibility
checks. Dynamic HEIC containers must first be split into a validated still.

Inspect with:

```sh
cargo run --manifest-path core/Cargo.toml --example heic_gain -- input.heic
cargo test --manifest-path core/Cargo.toml --lib heic_gain
```

App routing and the full-engine `Native` adapter are not changed by this module.
