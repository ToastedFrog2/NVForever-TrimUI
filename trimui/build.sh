#!/bin/bash
# Cross-builds NVForever for the TrimUI Smart Pro (aarch64 Linux, glibc 2.33) from macOS
# or Linux, and lays out the SD-card package in trimui/dist (docs/trimui.md).
# Needs: rustup with the aarch64-unknown-linux-gnu target, zig, cargo-zigbuild.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
sysroot="${TRIMUI_SYSROOT:-$here/sysroot}"
deps="${TRIMUI_DEPS:-$here/deps}"

# libasound to link against (cpal, through rodio): the device's own libasound.so.2 (Debian's
# needs glibc 2.34, the firmware has 2.33), with Debian's alsa.pc. Fetched once over SSH:
# TRIMUI_SSH=root@<device ip> trimui/build.sh
lib="$sysroot/usr/lib/aarch64-linux-gnu"
if [ ! -e "$lib/pkgconfig/alsa.pc" ]; then
    mkdir -p "$sysroot/debs" && cd "$sysroot/debs"
    curl -sSfLO "https://deb.debian.org/debian/pool/main/a/alsa-lib/libasound2-dev_1.2.8-1+b1_arm64.deb"
    cd "$sysroot"
    (mkdir -p x && cd x && ar x ../debs/libasound2-dev_1.2.8-1+b1_arm64.deb && tar xf data.tar.*)
    mkdir -p usr && cp -R x/usr/* usr/ && rm -rf x
fi
if [ ! -s "$lib/libasound.so.2.0.0" ] || [ ! -e "$sysroot/.device-libasound" ]; then
    : "${TRIMUI_SSH:?set TRIMUI_SSH=root@DEVICE-IP once to fetch libasound from the device}"
    ssh "$TRIMUI_SSH" 'cat /usr/lib/libasound.so.2.0.0' > "$lib/libasound.so.2.0.0"
    ln -sf libasound.so.2.0.0 "$lib/libasound.so.2" && ln -sf libasound.so.2.0.0 "$lib/libasound.so"
    touch "$sysroot/.device-libasound"
fi

if [ ! -x "$deps/spirv-opt" ]; then
    [ "$deps" = "$here/deps" ] || { echo "Missing optimizer in TRIMUI_DEPS" >&2; exit 1; }
    bash "$here/build-spirv-opt.sh"
fi
if [ "${1:-}" = --prepare-only ]; then exit 0; fi

export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR="$sysroot"
export PKG_CONFIG_LIBDIR="$sysroot/usr/lib/aarch64-linux-gnu/pkgconfig"
# Remap compiler-generated paths for distributable diagnostics/panic strings.
flags=("-C" "target-cpu=cortex-a53" "--remap-path-prefix=$root=/src/nvforever" "--remap-path-prefix=$HOME=/build")
export CARGO_ENCODED_RUSTFLAGS="$(IFS=$'\037'; echo "${flags[*]}")"
export CFLAGS_aarch64_unknown_linux_gnu="${CFLAGS_aarch64_unknown_linux_gnu:-} -ffile-prefix-map=$root=/src/nvforever -ffile-prefix-map=$HOME=/build"
export CXXFLAGS_aarch64_unknown_linux_gnu="${CXXFLAGS_aarch64_unknown_linux_gnu:-} -ffile-prefix-map=$root=/src/nvforever -ffile-prefix-map=$HOME=/build"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/engine/target-trimui}"

cd "$root/engine"
cargo zigbuild --profile "${NVF_CARGO_PROFILE:-release}" --target aarch64-unknown-linux-gnu.2.33 -p nvforever \
    --no-default-features --features trimui,intro-video

out="$here/dist/nvforever"

# Keep one delta-transfer base while testing over the handheld's slow Wi-Fi.
# This is outside the package tree and replaced at the next successful build.
if [ -x "$out/nvforever" ]; then ln -f "$out/nvforever" "$here/dist/previous-nvforever"; fi
rm -rf "$out" && mkdir -p "$out"
llvm_strip="$(ls "$(rustc --print sysroot)"/lib/rustlib/*/bin/llvm-strip 2>/dev/null | head -1 || true)"
bin="$CARGO_TARGET_DIR/aarch64-unknown-linux-gnu/${NVF_CARGO_PROFILE:-release}/nvforever"
[ -n "$llvm_strip" ] || { echo "LLVM tools required: rustup component add llvm-tools" >&2; exit 1; }
"$llvm_strip" -s -o "$out/nvforever" "$bin"
cp -R "$root/schemas" "$out/schemas"
cp "$here/package/run.sh" "$here/package/runtime-mainui.sh" "$here/package/setup-swap.sh" "$here/package/README.txt" "$here/package/dev-menu.ini" "$out/"
cp "$here/FPS-WORK.md" "$here/UPSTREAM-PERFORMANCE.md" "$out/"
cp "$here/package/NVForever.sh" "$here/dist/"
cp "$root/LICENSE" "$out/"
cp "$here/package/port.json" "$out/"
revision="${NVF_SOURCE_REVISION:-$(git -C "$root" rev-parse HEAD)}"
printf 'source=https://github.com/ToastedFrog2/NVForever-TrimUI\nrevision=%s\nprofile=trimui-portmaster\ncargo_profile=%s\n' "$revision" "${NVF_CARGO_PROFILE:-release}" > "$out/build-info.txt"
if [ -z "${NVF_SOURCE_REVISION:-}" ] && [ -n "$(git -C "$root" status --porcelain --untracked-files=normal)" ]; then
    echo 'local_changes=true' >> "$out/build-info.txt"
fi
mkdir -p "$out/tools"
cp "$here/tools/frame-costs.py" "$here/tools/summarize-frame-costs.py" "$here/tools/benchmark-fps.sh" "$out/tools/"
cp "$deps/spirv-opt" "$deps/SPIRV-Tools-LICENSE" "$deps/SPIRV-Headers-LICENSE" "$deps/spirv-opt.version" "$out/"
mkdir -p "$out/licenses"
cp "$root/LICENSE" "$deps/SPIRV-Tools-LICENSE" "$deps/SPIRV-Headers-LICENSE" "$out/licenses/"
zig cc -target aarch64-linux-gnu.2.33 -O2 -g0 "-ffile-prefix-map=$root=/src/nvforever" "-ffile-prefix-map=$HOME=/build" "$here/tools/loading.c" -o "$out/show-loading"
zig cc -target aarch64-linux-gnu.2.33 -O2 -g0 "-ffile-prefix-map=$root=/src/nvforever" "-ffile-prefix-map=$HOME=/build" "$here/tools/spirv-normalize.c" -o "$out/spirv-normalize"
# The cross-linker can reuse C startup objects containing cached debug paths.
"$llvm_strip" -s "$out/show-loading" "$out/spirv-normalize"
chmod +x "$out/nvforever" "$out/run.sh" "$out/setup-swap.sh" "$here/dist/NVForever.sh"
echo "PortMaster package: $here/dist (NVForever.sh and nvforever/ go in the PortMaster ports directory)"
