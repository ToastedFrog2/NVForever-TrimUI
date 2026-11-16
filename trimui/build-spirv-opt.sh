#!/bin/bash
# Reproducible ARM Linux shader optimizer for the stock PowerVR compiler workaround.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/deps"
mkdir -p "$out"
df -h "$here"
du -sh "$here"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/nvf-spirv.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
llvm_ar="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin/llvm-ar"
printf '#!/bin/sh\nexec "%s" s "$@"\n' "$llvm_ar" > "$scratch/ranlib"
chmod +x "$scratch/ranlib"
git clone --quiet --depth 1 --branch v2026.4 https://github.com/KhronosGroup/SPIRV-Tools.git "$scratch/source"
revision=$(python3 - "$scratch/source/DEPS" <<'PY'
import re,sys
print(re.search(r"'spirv_headers_revision':\s*'([^']+)'",open(sys.argv[1]).read())[1])
PY
)
git clone --quiet --no-checkout https://github.com/KhronosGroup/SPIRV-Headers.git "$scratch/source/external/spirv-headers"
git -C "$scratch/source/external/spirv-headers" checkout --quiet "$revision"
cmake -S "$scratch/source" -B "$scratch/build" -G Ninja \
  -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=aarch64 \
  -DCMAKE_AR="$llvm_ar" -DCMAKE_RANLIB="$scratch/ranlib" \
  -DCMAKE_C_COMPILER="$(command -v zig)" -DCMAKE_C_COMPILER_ARG1=cc \
  -DCMAKE_CXX_COMPILER="$(command -v zig)" -DCMAKE_CXX_COMPILER_ARG1=c++ \
  -DCMAKE_C_FLAGS='-target aarch64-linux-gnu.2.33 -mcpu=cortex_a53' \
  -DCMAKE_CXX_FLAGS='-target aarch64-linux-gnu.2.33 -mcpu=cortex_a53' \
  -DCMAKE_EXE_LINKER_FLAGS=-static-libstdc++ -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
  -DCMAKE_BUILD_TYPE=Release -DSPIRV_SKIP_TESTS=ON -DSPIRV_WERROR=OFF \
  -DSPIRV_TOOLS_BUILD_STATIC=ON -DBUILD_SHARED_LIBS=OFF
cmake --build "$scratch/build" --target spirv-opt -j 6
"$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin/llvm-strip" \
  -s -o "$out/spirv-opt" "$scratch/build/tools/spirv-opt"
cp "$scratch/source/LICENSE" "$out/SPIRV-Tools-LICENSE"
cp "$scratch/source/external/spirv-headers/LICENSE" "$out/SPIRV-Headers-LICENSE"
{ git -C "$scratch/source" rev-parse HEAD; echo "SPIRV-Headers $revision"; } > "$out/spirv-opt.version"
du -sh "$scratch" "$out"
echo "Built $out/spirv-opt"
