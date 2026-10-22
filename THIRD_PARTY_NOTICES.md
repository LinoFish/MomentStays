# Third-party materials and distribution

MomentStays's original source, documentation, and artwork are licensed under **GPL-3.0-only**, with copyright **2026 Lino**. See [LICENSE](LICENSE) and [COPYRIGHT](COPYRIGHT).

## Runtime dependencies

The native application uses frameworks supplied by macOS, including AppKit, SwiftUI, AVFoundation, ImageIO, CoreGraphics, PhotoKit, and CryptoKit. The project build does not vendor or bundle copies of those Apple frameworks. Their own terms remain applicable; the project license does not relicense Apple software.

The optional Python workflow uses the Python standard library and the project's native helper. No third-party Python package is bundled by the current build scripts.

## Optional validation tools

`tests/ProbeUltraHDR.cpp` is a test-only adapter for Google's [libultrahdr](https://github.com/google/libultrahdr). The library and its headers are external prerequisites for that optional probe; the app build does not compile or bundle them. The upstream [license file](https://github.com/google/libultrahdr/blob/main/LICENSE) provides MIT and Apache-2.0 license texts. Copies of upstream materials retain their own license and attribution requirements.

Links to format specifications, research projects, and reference samples are references, not copies of those projects or a grant of rights to their materials. Personal media and local research downloads are excluded from this repository.

## Distributing a binary

The application build copies `LICENSE`, `COPYRIGHT`, and this file into the app's Resources directory. The DMG packager also places them beside the application.

When conveying a covered binary, provide the corresponding source through a method permitted by GPLv3 section 6. Use the source that actually corresponds to that build, including build/install scripts and the required notices. If offering a binary download, provide recipients equivalent access to its corresponding source as required by that section. A private repository URL alone does not provide source access to recipients who cannot access it.

The current packaging scripts do not automatically create a corresponding-source release or a written source offer. Arrange source access before distributing a binary. Adopting this license does not change the GitHub repository's private/public setting.
