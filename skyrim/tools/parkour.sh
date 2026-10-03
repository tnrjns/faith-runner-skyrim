#!/usr/bin/env bash
# Surveys (the plugin's F10, in the SKSE log folder) -> the "Faith Runner Parkour" MO2 mod:
#   SKSE/Plugins/FaithParkour/<worldspace>.bin   Faith-only collision fixes
set -e
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../.."
logs="$(powershell -NoProfile -Command "[Environment]::GetFolderPath('MyDocuments')")/My Games/Skyrim Special Edition/SKSE"
mod="$(cygpath -u "$LOCALAPPDATA")/ModOrganizer/Skyrim Special Edition/mods/Faith Runner Parkour"
work="$root/target/parkour"
mkdir -p "$work" "$mod/SKSE/Plugins/FaithParkour"
shopt -s nullglob
placements=()
for s in "$logs"/FaithSurvey_*.bin; do
  echo "== $s"
  (cd "$root" && cargo run -q -p parkour_tool --release -- "$s" "$work")
done
cp "$work"/*.bin "$mod/SKSE/Plugins/FaithParkour/" 2>/dev/null || true
# Only invisible, Faith-only fixes: no visible objects are added to the cities.
echo "done: $mod"
