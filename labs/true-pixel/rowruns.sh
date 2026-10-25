#!/usr/bin/env bash
# Classify one row of a capture into runs of Adwaita surfaces, at 1px
# resolution. This is the tool that produced every click coordinate in
# capture.sh: point it at a row, read where each surface starts and
# ends, and the gaps between runs are widget boundaries.
#
# Usage: rowruns.sh <png> <y>
#
# Surfaces it knows, by their rendered pixels in dark Adwaita:
#   WIN    window background   #222226
#   CTRL   control fill        10% white over the window
#   SEL    selected pill       ~20% white over the window
#   BRIGHT text and icon cores
#
# Runs shorter than 2px are dropped, which discards most antialiasing.
# Anything else prints as OTHER: look at those runs when a new surface
# shows up (a new accent colour, a new shade) rather than guessing its
# triple from the stylesheet.

set -u

if [ $# -ne 2 ]; then
    echo "usage: $0 <png> <y>" >&2
    exit 1
fi

convert "$1" -depth 8 txt:- 2>/dev/null | tr -d '()' | awk -F'[:,]' -v ROW="$2" '
  {
    x=$1+0; y=$2+0; r=$3+0; g=$4+0; b=$5+0
    if (y != ROW) next
    if (r>=30 && r<=40 && g>=30 && g<=40 && b>=34 && b<=44) c="WIN"
    else if (r>=52 && r<=62 && g>=52 && g<=62 && b>=56 && b<=66) c="CTRL"
    else if (r>=70 && r<=95 && g>=70 && g<=95 && b>=74 && b<=99) c="SEL"
    else if (r>200) c="BRIGHT"
    else c="OTHER"
    if (c == prev) { end=x; n++ }
    else {
      if (prev != "" && n >= 2) printf "%-6s x=%d..%d (%d)\n", prev, start, end, n
      prev=c; start=x; end=x; n=1
    }
  }
  END { if (prev != "" && n >= 2) printf "%-6s x=%d..%d (%d)\n", prev, start, end, n }'
