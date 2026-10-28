#!/bin/bash
# Source from the repository root. All fallback toolchain files stay ignored locally.
motionlive_rustc="${RUSTC:-$(command -v rustc)}"
motionlive_rust_sysroot="$("$motionlive_rustc" --print sysroot)"
if [[ -d "$motionlive_rust_sysroot/lib/rustlib/aarch64-apple-darwin/lib" && -d "$motionlive_rust_sysroot/lib/rustlib/x86_64-apple-darwin/lib" ]]; then
  return 0 2>/dev/null || exit 0
fi
motionlive_rust_version=$("$motionlive_rustc" --version | awk '{print $2}')
motionlive_rust_host=$("$motionlive_rustc" -vV | awk '/^host:/ {print $2}')
motionlive_toolchain_dir="$PWD/.build/rust-universal-toolchain"
motionlive_official_prefix="$motionlive_toolchain_dir/official-$motionlive_rust_version-$motionlive_rust_host"
mkdir -p "$motionlive_toolchain_dir"
motionlive_install_rust_component() {
  local component="$1" archive="$motionlive_toolchain_dir/$1.tar.xz"
  if [[ -f "$motionlive_official_prefix/lib/rustlib/$component.installed" ]]; then return; fi
  if [[ ! -f "$archive.sha256" ]]; then
    curl -fsSL --retry 2 --connect-timeout 20 "https://static.rust-lang.org/dist/$component.tar.xz.sha256" -o "$archive.sha256"
  fi
  if [[ ! -f "$archive" ]]; then
    curl -fsSL --retry 2 --connect-timeout 20 "https://static.rust-lang.org/dist/$component.tar.xz" -o "$archive.part"
    mv "$archive.part" "$archive"
  fi
  (cd "$motionlive_toolchain_dir" && shasum -a 256 -c "$component.tar.xz.sha256")
  if [[ ! -d "$motionlive_toolchain_dir/$component" ]]; then tar -xf "$archive" -C "$motionlive_toolchain_dir"; fi
  bash "$motionlive_toolchain_dir/$component/install.sh" --prefix="$motionlive_official_prefix" --disable-ldconfig
  touch "$motionlive_official_prefix/lib/rustlib/$component.installed"
}
motionlive_install_rust_component "rustc-$motionlive_rust_version-$motionlive_rust_host"
motionlive_install_rust_component "rust-std-$motionlive_rust_version-x86_64-apple-darwin"
motionlive_install_rust_component "rust-std-$motionlive_rust_version-aarch64-apple-darwin"
motionlive_rustc="$motionlive_official_prefix/bin/rustc"
motionlive_rust_sysroot="$("$motionlive_rustc" --print sysroot)"
