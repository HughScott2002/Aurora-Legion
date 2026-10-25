#!/usr/bin/env bash
# Score a Figma export against a capture, numerically.
#
# Usage: diff.sh <figma-export.png> <capture.png>
#
# Two numbers, because they answer different questions:
#
#   strict   pixels that differ at all. Text never reaches zero: Pango
#            and Figma rasterize the same glyphs differently, so this
#            number is a ceiling, not a goal.
#
#   flat     pixels that differ by more than 10%, which in practice
#            means a different surface: a moved edge, a wrong fill, a
#            missing widget. Antialiasing lands under the threshold.
#            This is the number the kit work drives toward zero.
#
# Both images should be 640x720 (export the Figma frame at 1x, crop the
# capture with capture.sh). Different sizes are reported and refused
# rather than resampled, because a resize would blur exactly the edges
# being checked.

set -u

if [ $# -ne 2 ]; then
    echo "usage: $0 <figma-export.png> <capture.png>" >&2
    exit 1
fi

figma_png="$1"
capture_png="$2"

for f in "$figma_png" "$capture_png"; do
    if [ ! -f "$f" ]; then
        echo "no such file: $f" >&2
        exit 1
    fi
done

figma_size=$(identify -format '%wx%h' "$figma_png" 2>/dev/null)
capture_size=$(identify -format '%wx%h' "$capture_png" 2>/dev/null)

if [ -z "$figma_size" ] || [ -z "$capture_size" ]; then
    echo "could not read image metadata (is ImageMagick's identify installed?)" >&2
    exit 1
fi

if [ "$figma_size" != "$capture_size" ]; then
    echo "size mismatch: figma $figma_size, capture $capture_size" >&2
    echo "export the frame at 1x and compare like with like" >&2
    exit 1
fi

strict=$(compare -metric AE "$figma_png" "$capture_png" null: 2>&1)
flat=$(compare -fuzz 10% -metric AE "$figma_png" "$capture_png" null: 2>&1)

# compare prints raw counts to stderr with a trailing parenthesised
# fraction; keep just the count.
strict_count=$(echo "$strict" | sed 's/ .*//')
flat_count=$(echo "$flat" | sed 's/ .*//')

total=$(identify -format '%w' "$figma_png" 2>/dev/null)
total=$((total * $(identify -format '%h' "$figma_png" 2>/dev/null)))

strict_pct=$(awk -v a="$strict_count" -v t="$total" 'BEGIN { printf "%.2f", 100 * a / t }')
flat_pct=$(awk -v a="$flat_count" -v t="$total" 'BEGIN { printf "%.2f", 100 * a / t }')

echo "strict (any difference):   $strict_count px ($strict_pct%)"
echo "flat   (>10% difference):  $flat_count px ($flat_pct%)"
echo
echo "flat is the one that means drift; strict includes text rasterization"
