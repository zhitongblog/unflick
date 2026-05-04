# Third-party licenses

unflick is **MIT-licensed** (see [LICENSE](LICENSE)). It bundles, statically links, or invokes the following third-party software at runtime. The full license text for the LGPL/GPL components is in the [`licenses/`](licenses/) directory shipped alongside every installer; the aggregate listing below satisfies the attribution requirements of the MIT, Apache-2.0, ISC, BSD, and Mozilla Public Licenses.

## Bundled at runtime

| Component | License | Linkage | Source |
|---|---|---|---|
| **libmpv-2.dll** (Windows) / system `libmpv` (macOS Homebrew, Linux distro pkg) | **LGPL-2.1-or-later** | Dynamic load via `libloading` | https://github.com/mpv-player/mpv |
| **ffmpeg.exe** (Windows, gyan.dev "essentials" build) | **GPL-3.0-or-later** (`--enable-gpl --enable-version3`, includes libx264 / libx265 / libxvid) | Subprocess invocation only — never linked into unflick | https://www.gyan.dev/ffmpeg/builds/ · https://github.com/FFmpeg/FFmpeg |
| **yt-dlp.exe** | Unlicense (public domain dedication) | Subprocess invocation only | https://github.com/yt-dlp/yt-dlp |
| **whisper-cli + ggml libraries** (AI edition only) | MIT | Subprocess invocation only | https://github.com/ggml-org/whisper.cpp |

## Rust dependencies (statically linked into the unflick binary)

Generated with `cargo license --avoid-build-deps`. Build-only crates (proc-macros, codegen helpers) are excluded — they are not present in the shipped binary.

### Apache-2.0 OR MIT — 350 crates
`ahash`, `android_system_properties`, `anstream`, `anstyle`, `anstyle-parse`, `anstyle-query`, `anstyle-wincon`, `anyhow`, `arbitrary`, `async-broadcast`, `async-channel`, `async-executor`, `async-fs`, `async-io`, `async-lock`, `async-net`, `async-process`, `async-recursion`, `async-signal`, `async-task`, `async-trait`, `atomic-waker`, `base64`, `base64`, `bit-set`, `bit-vec`, `bitflags`, `bitflags`, `block-buffer`, `blocking`, `bumpalo`, `camino`, `cargo-platform`, `cesu8`, `cfg-if`, `cgl`, `chrono`, `clap`, `clap_builder`, `clap_derive`, `clap_lex`, `colorchoice`, `concurrent-queue`, `cookie`, `core-foundation`, `core-foundation-sys`, `core-graphics`, `core-graphics`, `core-graphics-types`, `cpufeatures`, _… and 300 more_

### MIT — 142 crates
`ashpd`, `atk`, `atk-sys`, `block2`, `bytes`, `cairo-rs`, `cairo-sys-rs`, `cargo_metadata`, `cfb`, `combine`, `convert_case`, `darling`, `darling_core`, `darling_macro`, `derive_more`, `derive_more`, `derive_more-impl`, `dlib`, `dlopen2`, `dlopen2_derive`, `dom_query`, `endi`, `gdk`, `gdk-pixbuf`, `gdk-pixbuf-sys`, `gdk-sys`, `gdkwayland-sys`, `gdkx11`, `gdkx11-sys`, `generic-array`, `gio`, `gio-sys`, `glib`, `glib-macros`, `glib-sys`, `gobject-sys`, `gtk`, `gtk-sys`, `gtk3-macros`, `http-body`, `http-body-util`, `http-range`, `hyper`, `hyper-util`, `ico`, `infer`, `is-docker`, `is-wsl`, `javascriptcore-rs`, `javascriptcore-rs-sys`, _… and 92 more_

### Apache-2.0 OR MIT OR Zlib — 18 crates
`bytemuck`, `dispatch2`, `miniz_oxide`, `objc2-app-kit`, `objc2-cloud-kit`, `objc2-core-data`, `objc2-core-foundation`, `objc2-core-graphics`, `objc2-core-image`, `objc2-core-text`, `objc2-core-video`, `objc2-exception-helper`, `objc2-io-surface`, `objc2-osa-kit`, `objc2-quartz-core`, `objc2-ui-kit`, `objc2-web-kit`, `raw-window-handle`

### Unicode-3.0 — 18 crates
`icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `yoke-derive`, `zerofrom`, `zerofrom-derive`, `zerotrie`, `zerovec`, `zerovec-derive`

### Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT — 15 crates
`linux-raw-sys`, `rustix`, `wasi`, `wasip2`, `wasip3`, `wasm-encoder`, `wasm-metadata`, `wasmparser`, `wit-bindgen`, `wit-bindgen`, `wit-bindgen-core`, `wit-bindgen-rust`, `wit-bindgen-rust-macro`, `wit-component`, `wit-parser`

### MPL-2.0 — 7 crates
`cssparser`, `cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors`, `selectors`

### MIT OR Unlicense — 6 crates
`aho-corasick`, `byteorder`, `memchr`, `same-file`, `walkdir`, `winapi-util`

### Apache-2.0 — 6 crates
`glutin`, `glutin_egl_sys`, `glutin_glx_sys`, `glutin_wgl_sys`, `sync_wrapper`, `tao`

### ISC — 4 crates
`libloading`, `libloading`, `rustls-webpki`, `untrusted`

### BSD-3-Clause — 3 crates
`alloc-no-stdlib`, `alloc-stdlib`, `subtle`

### Apache-2.0 OR ISC OR MIT — 3 crates
`hyper-rustls`, `rustls`, `rustls-native-certs`

### CDLA-Permissive-2.0 — 3 crates
`webpki-root-certs`, `webpki-roots`, `webpki-roots`

### Zlib — 2 crates
`foldhash`, `foldhash`

### Apache-2.0 OR BSD-3-Clause OR MIT — 2 crates
`num_enum`, `num_enum_derive`

### Apache-2.0 OR LGPL-2.1-or-later OR MIT — 2 crates
`r-efi`, `r-efi`

### Apache-2.0 OR BSD-2-Clause OR MIT — 2 crates
`zerocopy`, `zerocopy-derive`

### 0BSD OR Apache-2.0 OR MIT — 1 crate
`adler2`

### BSD-3-Clause AND MIT — 1 crate
`brotli`

### BSD-3-Clause OR MIT — 1 crate
`brotli-decompressor`

### Apache-2.0 AND MIT — 1 crate
`dpi`

### Apache-2.0 OR CC0-1.0 OR MIT-0 — 1 crate
`dunce`

### (Apache-2.0 OR MIT) AND BSD-3-Clause — 1 crate
`encoding_rs`

### Apache-2.0 AND ISC — 1 crate
`ring`

### (Apache-2.0 OR MIT) AND Unicode-3.0 — 1 crate
`unicode-ident`

## Frontend (npm) dependencies — production only

Generated with `pnpm licenses list --prod --json`. Dev-only packages (vite, tsc, tailwind, etc.) are not bundled into the released app.

### MIT — 12 packages
`@types/prop-types@15.7.15`, `@types/react@18.3.28`, `csstype@3.2.3`, `framer-motion@11.18.2`, `js-tokens@4.0.0`, `loose-envify@1.4.0`, `motion-dom@11.18.1`, `motion-utils@11.18.1`, `react@18.3.1`, `react-dom@18.3.1`, `scheduler@0.23.2`, `zustand@5.0.12`

### Apache-2.0 OR MIT — 1 package
`@tauri-apps/api@2.10.1`

### MIT OR Apache-2.0 — 1 package
`@tauri-apps/plugin-shell@2.3.5`

### 0BSD — 1 package
`tslib@2.8.1`

## Compliance notes

### GPL-3.0 (ffmpeg)
ffmpeg is invoked as an **external subprocess** via `std::process::Command` for clip extraction and stream URL probing. unflick does not link to or modify ffmpeg. Per GPL-3.0 §5, this is "mere aggregation" and does not extend the GPL to unflick. Users are free to remove or replace `ffmpeg.exe` with any compatible build. Full GPL-3.0 text: [`licenses/GPL-3.0.txt`](licenses/GPL-3.0.txt). Source code: https://github.com/FFmpeg/FFmpeg ; Windows build source: https://www.gyan.dev/ffmpeg/builds/ (`ffmpeg-essentials_build` 8.1).

### LGPL-2.1+ (libmpv)
libmpv is loaded dynamically at runtime via `libloading::Library::new`. Users can replace the bundled DLL or link a different libmpv build without recompiling unflick. Full LGPL-2.1 text: [`licenses/LGPL-2.1.txt`](licenses/LGPL-2.1.txt). Source code: https://github.com/mpv-player/mpv .

### MPL-2.0 (cssparser, selectors, et al.)
A handful of crates pulled in by the Servo HTML parsing chain are MPL-2.0 (file-level copyleft). unflick does not modify any MPL-2.0 file. Each MPL-2.0 covered file retains its original notice in `cargo`'s registry cache. Full MPL-2.0 text: [`licenses/MPL-2.0.txt`](licenses/MPL-2.0.txt).

### MIT, Apache-2.0, ISC, BSD
The bulk of Rust + npm dependencies use these permissive licenses. The aggregate listing in this file serves as the attribution required by clause 2 of the MIT license, clause 4(d) of Apache-2.0, the BSD attribution clause, and the ISC retention clause. No modifications were made to any of these dependencies; each retains its own copyright in the source distribution.

---
_This file is regenerated at every release from `cargo license` + `pnpm licenses list`. Run those commands at the same git revision to verify._
