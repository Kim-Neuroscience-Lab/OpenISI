#!/usr/bin/env bash
# Materialise the GENUINE oracle reference code at PINNED upstream commits.
#
# The oracle suite is faithfulness validation: it executes the field's real
# reference code live. That code (`reference/ISI` = SNLC/Garrett MATLAB,
# `reference/NeuroAnalysisTools` = Allen/Zhuang Python) is third-party and large
# (the full `reference/` tree is ~289 MB), so it is gitignored — NOT vendored into
# this repo. Instead it is fetched here, byte-pristine, at exact pinned commits.
#
# This is the "per-oracle locked environment / pristine reference, reproducible off
# this machine" guarantee of the validation-foundation goal: CI (and any clean
# machine) reconstructs the EXACT reference the oracle was validated against from
# the committed pins below — the machine's own copy is never assumed.
#
# The pins ARE the committed record of "the exact reference version". Update one
# ONLY deliberately (and re-run the oracle suite): a pin bump is a reference-version
# change, reviewed like any other.
#
# Usage:  tools/oracle/fetch-reference.sh [dest_dir=reference]

set -euo pipefail

# name|upstream URL|pinned commit
PINS=(
  "ISI|https://github.com/SNLC/ISI.git|175f012d0be1208a851ca26939066ddb4c66756c"
  "NeuroAnalysisTools|https://github.com/zhuangjun1981/NeuroAnalysisTools.git|0c7acdb745ef93e009ec538af11252e743f9d430"
)

dest="${1:-reference}"
mkdir -p "$dest"

for pin in "${PINS[@]}"; do
  IFS='|' read -r name url commit <<<"$pin"
  dir="$dest/$name"
  if [ -d "$dir/.git" ]; then
    echo "[fetch-reference] $name: present — fetching + checking out $commit"
    git -C "$dir" fetch --quiet origin || true
  else
    echo "[fetch-reference] $name: cloning $url"
    rm -rf "$dir"
    git clone --quiet "$url" "$dir"
  fi
  # Full clone above (not shallow) so an arbitrary pinned commit is checkable out.
  git -C "$dir" checkout --quiet "$commit"
  got="$(git -C "$dir" rev-parse HEAD)"
  if [ "$got" != "$commit" ]; then
    echo "[fetch-reference] ERROR: $name is at $got, expected $commit" >&2
    exit 1
  fi
  echo "[fetch-reference] $name @ $got (byte-pristine, pinned)"
done

echo "[fetch-reference] done — reference materialised under '$dest/'."
