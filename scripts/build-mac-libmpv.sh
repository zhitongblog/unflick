#!/usr/bin/env bash
#
# Build the libmpv the macOS app ships: one universal libmpv.2.dylib, every
# dependency linked in statically, depending on nothing but the system.
#
#   ./scripts/build-mac-libmpv.sh           # → src-tauri/mpv-dev/libmpv.2.dylib
#   ./scripts/build-mac-libmpv.sh --clean   # forget every cached stage first
#
# Why this exists rather than Homebrew's mpv: Homebrew builds mpv without
# libdvdnav, and homebrew-core formulas take no options, so on a Mac there
# was no way to get a libmpv that plays a DVD at all. Building our own also
# means the app no longer needs Homebrew to play anything.
#
# What is in it is what unflick uses and no more: the render API over
# OpenGL, lavfi audio filters (equalizer, loudness), scaletempo2, libass
# subtitles with uchardet to guess a .srt's encoding, dav1d for AV1,
# VideoToolbox decoding, HTTPS through the system's SecureTransport — and
# discs: libdvdnav/libdvdread with libdvdcss for DVDs, libbluray for
# Blu-ray. No Lua, JavaScript, vapoursynth, rubberband or libarchive; none
# of them is reachable from unflick.
#
# Each architecture is built into its own prefix — x86_64 is cross-compiled
# on Apple Silicon, where Rosetta runs the configure probes — and the two
# dylibs are joined with lipo. Stages are cached by name and version under
# src-tauri/vendor/libmpv-mac, so a rebuild after bumping one library
# rebuilds that library and what links it, not the world.
#
# Build-time tools (none of them ship): brew install meson ninja nasm cmake
#                                        pkgconf autoconf automake libtool

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${UNFLICK_LIBMPV_WORK:-$ROOT/src-tauri/vendor/libmpv-mac}"
OUT="$ROOT/src-tauri/mpv-dev/libmpv.2.dylib"
ARCHS="${UNFLICK_LIBMPV_ARCHS:-arm64 x86_64}"

# The app's own floor (tauri.conf.json → bundle.macOS.minimumSystemVersion).
# A library built for a newer macOS loads there and then fails on the first
# missing symbol, which is far worse than refusing to install.
export MACOSX_DEPLOYMENT_TARGET=11.0

MPV_TAG=v0.41.0
FFMPEG_TAG=n9.0.2
DAV1D_TAG=1.5.4
FREETYPE_TAG=VER-2-14-3
FRIBIDI_TAG=v1.0.17
HARFBUZZ_TAG=14.5.0
LIBASS_TAG=0.17.5
LIBPLACEBO_TAG=v7.360.1
UCHARDET_TAG=v0.0.8
DVDCSS_TAG=1.6.0
DVDREAD_TAG=7.1.1
DVDNAV_TAG=7.0.0
BLURAY_TAG=1.5.1

if [ "${1:-}" = "--clean" ]; then
  rm -rf "$WORK"
fi

for tool in meson ninja nasm cmake pkg-config autoreconf git lipo; do
  command -v "$tool" >/dev/null || {
    echo "ERROR: $tool not found — brew install meson ninja nasm cmake pkgconf autoconf automake libtool" >&2
    exit 1
  }
done

mkdir -p "$WORK/src"
JOBS="$(sysctl -n hw.ncpu)"

# ------------------------------------------------------------------ sources --
fetch() { # name url tag [--recursive]
  local dir="$WORK/src/$1-$3"
  [ -d "$dir" ] && return 0
  echo "==> fetch $1 $3"
  rm -rf "$dir.tmp"
  git -c advice.detachedHead=false clone -q --depth 1 --branch "$3" \
    ${4:+--recurse-submodules --shallow-submodules} "$2" "$dir.tmp"
  mv "$dir.tmp" "$dir"
}

fetch ffmpeg     https://github.com/FFmpeg/FFmpeg.git                     "$FFMPEG_TAG"
fetch dav1d      https://code.videolan.org/videolan/dav1d.git             "$DAV1D_TAG"
fetch freetype   https://gitlab.freedesktop.org/freetype/freetype.git     "$FREETYPE_TAG"
fetch fribidi    https://github.com/fribidi/fribidi.git                   "$FRIBIDI_TAG"
fetch harfbuzz   https://github.com/harfbuzz/harfbuzz.git                 "$HARFBUZZ_TAG"
fetch libass     https://github.com/libass/libass.git                     "$LIBASS_TAG"
fetch libplacebo https://code.videolan.org/videolan/libplacebo.git        "$LIBPLACEBO_TAG" --recursive
fetch uchardet   https://gitlab.freedesktop.org/uchardet/uchardet.git     "$UCHARDET_TAG"
fetch libdvdcss  https://code.videolan.org/videolan/libdvdcss.git         "$DVDCSS_TAG"
fetch libdvdread https://code.videolan.org/videolan/libdvdread.git        "$DVDREAD_TAG"
fetch libdvdnav  https://code.videolan.org/videolan/libdvdnav.git         "$DVDNAV_TAG"
fetch libbluray  https://code.videolan.org/videolan/libbluray.git         "$BLURAY_TAG" --recursive
fetch mpv        https://github.com/mpv-player/mpv.git                    "$MPV_TAG"

# ------------------------------------------------------------ per arch ------
build_arch() {
  local arch="$1"
  local cpu host
  case "$arch" in
    arm64)  cpu=aarch64; host=aarch64-apple-darwin ;;
    x86_64) cpu=x86_64;  host=x86_64-apple-darwin ;;
    *) echo "unknown arch $arch" >&2; exit 1 ;;
  esac

  local PREFIX="$WORK/$arch/prefix"
  local BUILD="$WORK/$arch/build"
  mkdir -p "$PREFIX/lib/pkgconfig" "$BUILD"

  local flags="-arch $arch -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"

  # Only what this script built is visible to pkg-config. Without this a
  # Homebrew library of the same name is found first and linked dynamically,
  # and the dylib quietly depends on /opt/homebrew again.
  export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
  unset PKG_CONFIG_PATH

  local cross="$WORK/$arch/cross.ini"
  cat > "$cross" <<EOF
[binaries]
c = ['clang', '-arch', '$arch']
cpp = ['clang++', '-arch', '$arch']
objc = ['clang', '-arch', '$arch']
objcpp = ['clang++', '-arch', '$arch']
ar = 'ar'
strip = 'strip'
pkg-config = 'pkg-config'
nasm = 'nasm'

[built-in options]
c_args = ['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET']
cpp_args = ['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET']
objc_args = ['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET']
c_link_args = ['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET']
cpp_link_args = ['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET']
objc_link_args = ['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET']

[properties]
needs_exe_wrapper = false
pkg_config_libdir = '$PREFIX/lib/pkgconfig'

[host_machine]
system = 'darwin'
cpu_family = '$cpu'
cpu = '$cpu'
endian = 'little'
EOF

  stage() { # name tag — true when already built
    local stamp="$PREFIX/.built-$1-$2"
    if [ -f "$stamp" ]; then return 0; fi
    echo "==> [$arch] $1 $2"
    return 1
  }
  done_stage() { touch "$PREFIX/.built-$1-$2"; }

  meson_build() { # name tag [meson options...]
    local name="$1" tag="$2"; shift 2
    stage "$name" "$tag" && return 0
    local b="$BUILD/$name-$tag"
    rm -rf "$b"
    meson setup "$b" "$WORK/src/$name-$tag" --cross-file "$cross" \
      --prefix "$PREFIX" --libdir lib --buildtype release \
      -Ddefault_library=static -Dprefer_static=true "$@" >"$b.log" 2>&1 ||
      { tail -40 "$b.log"; exit 1; }
    ninja -C "$b" -j"$JOBS" install >>"$b.log" 2>&1 || { tail -40 "$b.log"; exit 1; }
    done_stage "$name" "$tag"
  }

  cmake_build() { # name tag [cmake options...]
    local name="$1" tag="$2"; shift 2
    stage "$name" "$tag" && return 0
    local b="$BUILD/$name-$tag"
    rm -rf "$b"
    cmake -S "$WORK/src/$name-$tag" -B "$b" -G Ninja \
      -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
      -DCMAKE_INSTALL_LIBDIR=lib \
      -DCMAKE_OSX_ARCHITECTURES="$arch" \
      -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOSX_DEPLOYMENT_TARGET" \
      -DBUILD_SHARED_LIBS=OFF \
      -DCMAKE_POLICY_VERSION_MINIMUM=3.5 "$@" >"$b.log" 2>&1 || { tail -40 "$b.log"; exit 1; }
    ninja -C "$b" -j"$JOBS" install >>"$b.log" 2>&1 || { tail -40 "$b.log"; exit 1; }
    done_stage "$name" "$tag"
  }

  autotools_build() { # name tag [configure options...]
    local name="$1" tag="$2"; shift 2
    stage "$name" "$tag" && return 0
    local b="$BUILD/$name-$tag"
    rm -rf "$b"
    cp -R "$WORK/src/$name-$tag" "$b"
    (
      # Chained, not sequenced: `set -e` does not apply inside a subshell
      # whose status is being tested, so a failed configure would otherwise
      # run on into make and bury its own error under make's.
      cd "$b" &&
      { [ -x configure ] || autoreconf -fi >"$b.log" 2>&1; } &&
      ./configure --host="$host" --prefix="$PREFIX" --libdir="$PREFIX/lib" \
        --enable-static --disable-shared --with-pic \
        CC="clang $flags" CXX="clang++ $flags" "$@" >>"$b.log" 2>&1 &&
      make -j"$JOBS" >>"$b.log" 2>&1 &&
      make install >>"$b.log" 2>&1
    ) || { tail -40 "$b.log"; exit 1; }
    done_stage "$name" "$tag"
  }

  # A library's build system is whatever its maintainers chose, so each one
  # is driven the way it builds, not the way the others do.
  build_with() { # name tag [options...]
    local src="$WORK/src/$1-$2"
    if [ -f "$src/meson.build" ]; then meson_build "$@"
    elif [ -f "$src/CMakeLists.txt" ]; then cmake_build "$@"
    else autotools_build "$@"
    fi
  }

  meson_build dav1d "$DAV1D_TAG" -Denable_tools=false -Denable_tests=false

  meson_build freetype "$FREETYPE_TAG" -Dharfbuzz=disabled -Dpng=disabled \
    -Dbrotli=disabled -Dbzip2=disabled -Dzlib=disabled -Dtests=disabled

  meson_build fribidi "$FRIBIDI_TAG" -Ddocs=false -Dbin=false -Dtests=false

  meson_build harfbuzz "$HARFBUZZ_TAG" -Dfreetype=enabled -Dglib=disabled \
    -Dgobject=disabled -Dcairo=disabled -Dicu=disabled -Dtests=disabled \
    -Ddocs=disabled -Dutilities=disabled -Dintrospection=disabled \
    -Dcoretext=disabled -Dchafa=disabled

  meson_build libass "$LIBASS_TAG" -Dfontconfig=disabled -Dcoretext=enabled \
    -Ddirectwrite=disabled -Dasm=disabled -Dlibunibreak=disabled \
    -Dtest=disabled -Dcompare=disabled -Dprofile=disabled -Dfuzz=disabled \
    -Dcheckasm=disabled

  meson_build libplacebo "$LIBPLACEBO_TAG" -Dvulkan=disabled -Dopengl=enabled \
    -Dd3d11=disabled -Dglslang=disabled -Dshaderc=disabled -Dlcms=disabled \
    -Ddovi=disabled -Dlibdovi=disabled -Dunwind=disabled -Dxxhash=disabled \
    -Ddemos=false -Dtests=false

  cmake_build uchardet "$UCHARDET_TAG" -DBUILD_BINARY=OFF -DBUILD_STATIC=ON

  # libdvdread links libdvdcss outright instead of dlopen-ing it at run time:
  # a dlopen of a bare name would search dyld's default paths, not the app
  # bundle, and every encrypted disc would come back as noise.
  build_with libdvdcss  "$DVDCSS_TAG"
  build_with libdvdread "$DVDREAD_TAG" $( [ -f "$WORK/src/libdvdread-$DVDREAD_TAG/meson.build" ] \
    && echo "-Dlibdvdcss=enabled" || echo "--with-libdvdcss")
  build_with libdvdnav  "$DVDNAV_TAG"

  build_with libbluray "$BLURAY_TAG" $( [ -f "$WORK/src/libbluray-$BLURAY_TAG/meson.build" ] \
    && echo "-Dbdj_jar=disabled -Dlibxml2=disabled -Dfreetype=disabled -Dfontconfig=disabled -Denable_tools=false -Denable_docs=false" \
    || echo "--disable-bdjava-jar --without-libxml2 --without-freetype --without-fontconfig --disable-doxygen-doc")

  if ! stage ffmpeg "$FFMPEG_TAG"; then
    local b="$BUILD/ffmpeg-$FFMPEG_TAG"
    rm -rf "$b"; mkdir -p "$b"
    (
      cd "$b" &&
      # --disable-autodetect, then name each thing: ffmpeg otherwise links
      # whatever it finds on the machine — SDL, xz, a Homebrew libxml2 —
      # and the dylib stops being self-contained on the one machine that
      # built it, which is the one machine where nobody would notice.
      "$WORK/src/ffmpeg-$FFMPEG_TAG/configure" \
        --prefix="$PREFIX" --libdir="$PREFIX/lib" \
        --enable-cross-compile --arch="$arch" --target-os=darwin \
        --cc="clang" --cxx="clang++" \
        --extra-cflags="$flags" --extra-ldflags="$flags" \
        --pkg-config=pkg-config --pkg-config-flags=--static \
        --x86asmexe=nasm \
        --enable-static --disable-shared --enable-pic \
        --disable-programs --disable-doc --disable-debug \
        --enable-gpl --disable-autodetect \
        --enable-videotoolbox --enable-audiotoolbox \
        --enable-securetransport --enable-zlib --enable-bzlib --enable-iconv \
        --enable-libdav1d \
        --disable-avdevice >"$b.log" 2>&1 &&
      make -j"$JOBS" >>"$b.log" 2>&1 &&
      make install >>"$b.log" 2>&1
    ) || { tail -40 "$b.log"; exit 1; }
    done_stage ffmpeg "$FFMPEG_TAG"
  fi

  if ! stage mpv "$MPV_TAG"; then
    local b="$BUILD/mpv-$MPV_TAG"
    rm -rf "$b"
    # libmpv itself is the one shared library; everything under it was
    # built static above, and prefer_static makes pkg-config say so.
    # Cocoa and Swift on, the way Homebrew builds it — not because unflick
    # uses mpv's window (it draws through the render API into its own
    # view), but because 0.41 keeps macOS plumbing the audio output needs
    # (cfstr_get_cstr, in osdep/utils-mac.c) behind the Cocoa switch. The
    # first build here turned Cocoa off; it linked, and then segfaulted the
    # moment CoreAudio opened, calling a function that was never there.
    #
    # It linked because mpv sets b_lundef=false, which on macOS means
    # `-undefined dynamic_lookup`: a missing symbol is left for run time
    # instead of failing the link. b_lundef=true puts the error back where
    # it belongs, and the verification below checks for it regardless.
    #
    # Swift gets the target spelled out: mpv hands swiftc no architecture,
    # and x86_64 is cross-compiled.
    #
    # Off: mpv's own Now Playing / media-key integration and Touch Bar —
    # the window is unflick's, and so are the media keys.
    #
    # -liconv: macOS keeps iconv in its own system library, and with
    # prefer_static meson's probe neither finds a static one nor tries the
    # dylib — so uchardet, which needs iconv, fails the configure.
    meson setup "$b" "$WORK/src/mpv-$MPV_TAG" --cross-file "$cross" \
      --prefix "$PREFIX" --libdir lib --buildtype release \
      -Ddefault_library=shared -Dprefer_static=true -Db_lundef=true \
      "-Dc_link_args=['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET', '-liconv']" \
      "-Dobjc_link_args=['-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET', '-liconv']" \
      -Dswift-build=enabled -Dcocoa=enabled \
      "-Dswift-flags=-target $arch-apple-macos$MACOSX_DEPLOYMENT_TARGET" \
      -Dmacos-media-player=disabled -Dmacos-touchbar=disabled \
      -Dlibmpv=true -Dcplayer=false -Dtests=false -Dgpl=true \
      -Ddvdnav=enabled -Dlibbluray=enabled -Duchardet=enabled \
      -Dlibavdevice=disabled \
      -Dlua=disabled -Djavascript=disabled -Dlibarchive=disabled \
      -Drubberband=disabled -Dvapoursynth=disabled -Dzimg=disabled \
      -Djpeg=disabled -Dlcms2=disabled -Dvulkan=disabled \
      -Dcoreaudio=enabled -Dgl=enabled \
      -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled \
      >"$b.log" 2>&1 || { tail -60 "$b.log"; exit 1; }
    ninja -C "$b" -j"$JOBS" install >>"$b.log" 2>&1 || { tail -40 "$b.log"; exit 1; }
    done_stage mpv "$MPV_TAG"
  fi
}

for arch in $ARCHS; do
  build_arch "$arch"
done

# ------------------------------------------------------------------ join ----
mkdir -p "$(dirname "$OUT")"
slices=()
for arch in $ARCHS; do
  lib="$WORK/$arch/prefix/lib/libmpv.2.dylib"
  [ -f "$lib" ] || { echo "ERROR: no $lib" >&2; exit 1; }
  slices+=("$lib")
done
rm -f "$OUT"
lipo -create "${slices[@]}" -output "$OUT"
# The app finds it by path; the install name only has to not point at the
# build machine.
install_name_tool -id @rpath/libmpv.2.dylib "$OUT"
# mpv adds the build machine's Xcode Swift directory as an rpath. Nothing on
# a user's machine lives there; the OS copy under /usr/lib/swift is the one.
for arch in $ARCHS; do
  otool -arch "$arch" -l "$OUT" | awk '/LC_RPATH/{f=1} f&&/ path /{print $2; f=0}'
done | sort -u | while read -r rp; do
  case "$rp" in /usr/lib/*) ;; *) install_name_tool -delete_rpath "$rp" "$OUT" 2>/dev/null || true ;; esac
done
strip -x "$OUT"

# ---------------------------------------------------------------- verify ----
# The whole point is that nothing outside the system is needed. Say which
# dependency is not, rather than finding out on a machine without it.
echo "==> Verifying"
lipo -archs "$OUT"
# Per architecture: `otool -L` on a universal file prints a header line for
# each slice, and a header is not a dependency.
bad="$(for arch in $ARCHS; do otool -arch "$arch" -L "$OUT" | tail -n +2; done \
  | awk '{print $1}' \
  | grep -v -E '^(/usr/lib/|/System/Library/|@rpath/libmpv\.2\.dylib)' || true)"
if [ -n "$bad" ]; then
  echo "ERROR: libmpv depends on something that is not part of macOS:" >&2
  echo "$bad" >&2
  exit 1
fi
# A symbol left for dyld to find at run time is a symbol that was missing
# at link time — the exact shape of the CoreAudio segfault above.
lookups="$(nm -m "$OUT" | grep 'dynamically looked up' || true)"
if [ -n "$lookups" ]; then
  echo "ERROR: libmpv leaves symbols for run time — they were missing at link time:" >&2
  echo "$lookups" >&2
  exit 1
fi
# Read once, then search: `nm | grep -q` under pipefail fails whenever grep
# finds its match early and nm dies of the closed pipe.
exported="$(nm -gU "$OUT")"
for sym in _mpv_create _mpv_render_context_create; do
  grep -q " $sym\$" <<<"$exported" || { echo "ERROR: $sym is not exported" >&2; exit 1; }
done
for arch in $ARCHS; do
  minos="$(otool -arch "$arch" -l "$OUT" | awk '/LC_BUILD_VERSION/{f=1} f&&/minos/{print $2; exit}')"
  echo "    $arch: minos $minos"
done
ls -lh "$OUT"
echo "==> Done: $OUT"
