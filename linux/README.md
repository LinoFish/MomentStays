# MomentStays for GNOME (`linux/`)

GTK 4 + libadwaita app (`momentstays-gnome`, binary `momentstays`) on top of
`momentstays-core` (`../core`, public API only: `engine::request::handle`
with the same JSON requests as the macOS app). Ships only as a Flatpak on
`org.gnome.Platform//50`. App ID: `com.linofish.MomentStays` (`src/config.rs`).

## Layout

| Path | What |
| --- | --- |
| `src/model/` | View-model logic without GTK, unit tested: the five-step workflow and its gating (`flow.rs`, a port of the macOS `WorkspaceView` rules), this run's plan (`plan.rs`, macOS `isMigrationReady` …), queue, selection and filters, sources, settings → core requests, result summary, feature claims |
| `src/engine.rs` | Blocking bridge to the core; the UI runs it with `gio::spawn_blocking` |
| `src/ui/` | Window laid out like the macOS app: queue column (step title, result menu, feature tabs and tools, grid/list with the photo inspector, status bar) and the 300 pt workflow column (step list, Back + primary action, hint, one `AdwNavigationPage` per step); direction cards, cover and burst dialogs, run/progress |
| `data/` | `style.css` (brand accent), GResource, desktop file, metainfo, icon |
| `po/` | gettext catalogs: `zh_CN` (zh-Hans), `zh_TW` (zh-Hant, also installed as `zh_HK`); English is the source |
| `flatpak/` | Draft Flatpak manifest |

## Build and test

Inside the GNOME 50 SDK (what CI does, see `.github/workflows/gnome-ui.yml`):

```sh
flatpak run --share=network --filesystem=$PWD --command=sh org.gnome.Sdk//50 -c \
  '. /usr/lib/sdk/rust-stable/enable.sh && cd linux && cargo clippy --all-targets -- -D warnings && cargo test'
flatpak-builder --force-clean --user --install build-dir linux/flatpak/com.linofish.MomentStays.yml
```

Translations: edit `po/translations.py`, then run `python3 po/update.py` from
`linux/` (regenerates `momentstays.pot` and the `.po` files; fails if a string
is untranslated). `po/gen_translations.py` refills `translations.py` for new
strings from the macOS app's catalog (zh-Hans) and OpenCC (zh-Hant).

## Linux codec backend

The default feature `linux-native` enables the core's Linux backend
(libjpeg-turbo, lcms2, libheif, GStreamer from the GNOME runtime; see
`core/LINUX_NATIVE.md`). Building outside the SDK needs their development
packages. `--no-default-features` builds without it; the app then shows a
banner, and the core refuses codec steps (pixel checks, re-encoding, video
frames) while byte-level conversion still runs.
