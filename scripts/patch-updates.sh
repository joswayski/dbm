#!/usr/bin/env bash
# Builds the patch updates to one asset of this release from the newest
# earlier releases (docs/releases.md#patch-updates).
#
#   scripts/patch-updates.sh ASSET FILE BUILD OUT
#
# ASSET is the release asset's name (DBM-macOS.zip, DBM-Windows-x64.exe,
# DBM-Linux-x64, DBM-Linux-x86_64.AppImage), FILE this release's copy of it,
# BUILD this release's build number. Writes ASSET-FROM-to-BUILD.dbmpatch files
# to OUT. A base that is missing or can't be patched from is skipped: without
# a patch, installed copies download the full file. Needs gh, jq, unzip, and
# tools/dbm-delta (DBM_DELTA, default target/release/dbm-delta).
set -euo pipefail

asset="$1"
file="$(realpath "$2")"
build="$3"
out="$(realpath -m "$4")"
delta="$(realpath "${DBM_DELTA:-target/release/dbm-delta}")"
repo="${GITHUB_REPOSITORY:-joswayski/anybase}"
# Anybase ships in bursts of up to ~20 releases a day, then pauses for days.
# Twenty bases let a copy that updated any time during the last busy day patch.
bases="${PATCH_BASES:-20}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

# macOS ships a zip of the app; patches are built between the unpacked apps.
unpack() {
  case "$1" in
    *.zip)
      rm -rf "$2" && mkdir -p "$2" && unzip -q "$1" -d "$2"
      find "$2" -mindepth 1 -maxdepth 1 -name '*.app' -print -quit
      ;;
    *) echo "$1" ;;
  esac
}

target="$(unpack "$file" "$work/target")"
[[ -e "$target" ]] || { echo "::warning::No app in $file; no patches for $asset."; exit 0; }
full_size="$(stat -c %s "$file")"

patch_from() {
  local tag="$1" dir="$work/bases/$1" from base patch size summary
  mkdir -p "$dir"
  gh release download "$tag" --repo "$repo" --dir "$dir" \
    --pattern native-latest.json --pattern "$asset" 2>/dev/null || return 0
  from="$(jq -er .build "$dir/native-latest.json" 2>/dev/null)" || return 0
  (( from < build )) || return 0
  base="$(unpack "$dir/$asset" "$dir/app")"
  patch="$out/$asset-$from-to-$build.dbmpatch"
  if ! summary="$("$delta" diff "$base" "$from" "$target" "$build" "$patch" 2>&1)"; then
    echo "::warning::No patch from build $from to $asset: $summary"
  else
    size="$(stat -c %s "$patch")"
    # A patch over half the full download is not worth it.
    if (( size * 2 > full_size )); then
      echo "Dropped the patch from build $from: $size of $full_size bytes."
      rm -f "$patch"
    else
      echo "Patch from build $from: $size of $full_size bytes $summary"
    fi
  fi
  rm -rf "$dir"
}
export -f patch_from unpack
export asset build out delta repo work target full_size

gh release list --repo "$repo" --exclude-drafts --exclude-pre-releases \
  --limit "$bases" --json tagName --jq '.[].tagName' > "$work/tags" ||
  { echo "::warning::Couldn't list releases; no patches for $asset."; exit 0; }
# One patch per core: each takes seconds to a minute and a few hundred MB.
xargs -P "$(nproc)" -I{} bash -c 'patch_from "$1"' _ {} < "$work/tags"
ls -l "$out"
