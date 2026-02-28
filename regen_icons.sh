#! /usr/bin/env nix-shell
#! nix-shell -i bash -p inkscape jq
set -euo pipefail

STATIC=frontend/static
MANIFEST="$STATIC/manifest.json"
LAYOUT="frontend/src/routes/+layout.svelte"

# Each entry: "base_name:source_svg:width"
# base_name is the stable prefix; a content hash is appended to the final filename.
icons=(
    "icon-192:$STATIC/favicon.svg:192"
    "icon-512:$STATIC/favicon.svg:512"
    "icon-maskable-512:$STATIC/icon-maskable.svg:512"
)

for entry in "${icons[@]}"; do
    IFS=':' read -r base source width <<< "$entry"

    # Render to a temp file first
    tmpfile=$(mktemp /tmp/icon-XXXXXX.png)
    inkscape --export-type=png --export-width="$width" --export-filename="$tmpfile" "$source"

    # 8-character content hash for cache-busting
    hash=$(sha256sum "$tmpfile" | cut -c1-8)
    outfile="$STATIC/${base}-${hash}.png"

    # Remove all previously generated files for this base name
    rm -f "$STATIC/${base}"*.png

    mv "$tmpfile" "$outfile"
    echo "Generated: $outfile"

    # Update manifest.json: rewrite the src for any icon matching this base
    # (handles both hashed and un-hashed names from previous runs)
    updated=$(jq --arg base "$base" --arg src "/${base}-${hash}.png" \
        '.icons = [.icons[] | if (.src | test("^/" + $base + ".*\\.png$")) then .src = $src else . end]' \
        "$MANIFEST")
    echo "$updated" > "$MANIFEST"

    # Update the PNG fallback link in +layout.svelte for icon-512
    if [[ "$base" == "icon-512" ]]; then
        sed -i "s|href=\"/icon-512[^\"]*\.png\"|href=\"/${base}-${hash}.png\"|g" "$LAYOUT"
        echo "Updated: $LAYOUT (icon-512 PNG fallback → /${base}-${hash}.png)"
    fi
done

echo "Done. manifest.json updated."
