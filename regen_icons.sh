#! /usr/bin/env nix-shell
#! nix-shell -i bash -p inkscape
set -euo pipefail



declare -A icon_map=(
    [frontend/static/icon-512.png]="frontend/static/favicon.svg:512"
    [frontend/static/icon-192.png]="frontend/static/favicon.svg:192"
    [frontend/static/icon-maskable-512.png]="frontend/static/icon-maskable.svg:512"
)

for output in "${!icon_map[@]}"; do
    IFS=':' read -r input width <<< "${icon_map[$output]}"
    inkscape --export-type=png --export-width="$width" --export-filename="$output" "$input"
done
