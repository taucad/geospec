#!/usr/bin/env bash
# Makes `.solidcore/`: solidcore at the pinned revision with the evaluation
# harness's patches applied (`../../solidcore-eval/patches`), the checkout
# Cargo.toml's [patch] section builds against. Run it once before
# `cargo build`, and again after a patch changes.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
rev="2c5f750d782b74af9de91f3eb333a1414d4b8c89"
dest="$here/.solidcore"
if [ ! -d "$dest/.git" ]; then
  git clone --quiet --no-checkout https://github.com/tangency-labs/solidcore "$dest"
fi
git -C "$dest" checkout --quiet --force "$rev"
git -C "$dest" clean --quiet -fdx
patches=("$here"/../../solidcore-eval/patches/*.patch)
for p in "${patches[@]}"; do
  git -C "$dest" apply --whitespace=nowarn "$p"
done
echo "solidcore $rev with ${#patches[@]} patches in $dest"
