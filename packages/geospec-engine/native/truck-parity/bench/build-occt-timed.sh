#!/usr/bin/env bash
# Mirrors native/occt/build-occt.sh (patches + CMake options) without archive-hash gates; times each phase.
set -euo pipefail
P=$(cd "$(dirname "$0")/../../occt" && pwd)
W=${WORK:?set WORK to a scratch directory}
OCCT_CHECKOUT=${OCCT_CHECKOUT:?set OCCT_CHECKOUT to an OCCT git checkout at the pinned commit}
SRC=$W/occt-build/src
B=$W/occt-build/build
I=$W/occt-build/install
JOBS=${JOBS:-4}
rm -rf "$W/occt-build"
mkdir -p "$SRC"
cp -r "$OCCT_CHECKOUT/." "$SRC"
rm -rf "$SRC/.git"
for p in stepcaf-early-assembly step-assembly-sharings brepgprop-gauss-direct-arith step-read-only-controllers resource-unicode-no-cjk geombndlib-torus-closed-form brepextrema-prepared-face-cf; do
  patch -s -t -F 0 -p1 -d "$SRC" -i "$P/$p.patch"
done
FL="-UOCC_CONVERT_SIGNALS -ffp-contract=off -ffile-prefix-map=$SRC=occt"
t0=$(date +%s)
cmake -S "$SRC" -B "$B" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$I" -DINSTALL_DIR="$I" -DINSTALL_DIR_LAYOUT=Unix -DINSTALL_DIR_WITH_VERSION=OFF \
  -DBUILD_LIBRARY_TYPE=Static -DBUILD_OPT_PROFILE=Default -DBUILD_USE_PCH=OFF -DBUILD_ADDITIONAL_TOOLKITS=TKDESTEP \
  -DBUILD_MODULE_ApplicationFramework=OFF -DBUILD_MODULE_DataExchange=OFF -DBUILD_MODULE_Draw=OFF -DBUILD_MODULE_FoundationClasses=OFF \
  -DBUILD_MODULE_ModelingAlgorithms=OFF -DBUILD_MODULE_ModelingData=OFF -DBUILD_MODULE_Visualization=OFF -DBUILD_DOC_Overview=OFF -DBUILD_DOC_RefMan=OFF \
  -DINSTALL_TEST_CASES=OFF -DUSE_GIT_HASH=OFF -DUSE_TCL=OFF -DUSE_TK=OFF -DUSE_FREETYPE=OFF -DUSE_OPENGL=OFF -DUSE_TBB=OFF -DUSE_MMGR_TYPE=NATIVE \
  -DBUILD_RELEASE_DISABLE_EXCEPTIONS=ON -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_C_FLAGS="$FL" -DCMAKE_CXX_FLAGS="$FL" > "$W/occt-configure.log"
t1=$(date +%s)
cmake --build "$B" --target install --parallel "$JOBS" > "$W/occt-build.log" 2>&1
t2=$(date +%s)
{
  echo "configure_s=$((t1 - t0)) build_install_s=$((t2 - t1)) jobs=$JOBS nproc=$(nproc)"
  find "$I/lib" -name 'libTK*.a' | wc -l
  du -sh "$I/lib"
  grep -c "Building CXX" "$W/occt-build.log" || true
} > "$W/occt-timing.txt"
