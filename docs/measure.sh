#!/bin/sh
# Sample one process for 60 seconds: PSS (KiB) from smaps_rollup at start
# and end, plus CPU time consumed over the window as percent of one core.
#
# Usage: docs/measure.sh <pid> [<seconds>]
#
# These are the numbers behind the README comparison table; see
# docs/measurements.md for the scenarios they were collected under.

pid="$1"
window_secs="${2:-60}"

if [ -z "$pid" ] || [ ! -d "/proc/$pid" ]; then
    echo "usage: $0 <pid> [<seconds>]" >&2
    exit 1
fi

read_pss_kib() {
    awk '/^Pss:/ { print $2 }' "/proc/$pid/smaps_rollup"
}

# utime + stime, in clock ticks.
read_cpu_ticks() {
    awk '{ print $14 + $15 }' "/proc/$pid/stat"
}

ticks_per_sec=$(getconf CLK_TCK)

pss_start_kib=$(read_pss_kib)
cpu_start_ticks=$(read_cpu_ticks)

sleep "$window_secs"

if [ ! -d "/proc/$pid" ]; then
    echo "process $pid exited during the window" >&2
    exit 1
fi

pss_end_kib=$(read_pss_kib)
cpu_end_ticks=$(read_cpu_ticks)

cpu_delta_ticks=$((cpu_end_ticks - cpu_start_ticks))

awk -v pss_start="$pss_start_kib" -v pss_end="$pss_end_kib" \
    -v cpu_ticks="$cpu_delta_ticks" -v hz="$ticks_per_sec" -v secs="$window_secs" \
    'BEGIN {
        cpu_percent = (cpu_ticks / hz) / secs * 100.0
        printf "pss_start: %.1f MiB\n", pss_start / 1024.0
        printf "pss_end:   %.1f MiB\n", pss_end / 1024.0
        printf "cpu_avg:   %.2f%% of one core over %ds\n", cpu_percent, secs
    }'
