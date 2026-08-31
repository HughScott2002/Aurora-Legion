#!/usr/bin/env bash
# Interleaved A/B measurement of two lighting daemons on one machine.
#
# Usage:
#   docs/measure-compare.sh --a-cmd CMD --b-cmd CMD [options]
#
#   --a-name NAME     label for arm A (default: a)
#   --a-cmd  CMD      shell command that runs arm A in the foreground
#   --b-name NAME     label for arm B (default: b)
#   --b-cmd  CMD      shell command that runs arm B in the foreground
#   --rounds N        measured rounds per arm (default: 3)
#   --window SECS     length of the measured window (default: 60)
#   --warmup SECS     discarded window after start (default: same as --window)
#   --settle SECS     pause after killing an arm, before starting the next
#                     (default: 5)
#   --out FILE        CSV output path (default: measure-compare.csv)
#
# Why it is shaped this way, since the shape is the point:
#
#   Arms alternate. Round 1 runs A then B, round 2 runs B then A, and so
#   on. Batching all of one arm and then all of the other loads every bit
#   of machine drift onto whichever ran second, where it is
#   indistinguishable from a real difference.
#
#   Each round restarts the process. Repeated windows against one
#   long-lived process measure steady-state noise only and hide anything
#   that varies from start to start.
#
#   The first window after each start is discarded. Startup, page faults
#   and first-render costs dominate a window and are not what is being
#   compared.
#
#   Arms are never run concurrently. They contend for the same keyboard.
#
#   Only PIDs this script started are ever killed. Pattern-killing on a
#   command line matches the shell that is running the pattern.
#
# Reports the median of the measured rounds and the full raw values.
# Three rounds cannot support a significance claim; the raw values and
# the spread are there so a reader can judge the difference themselves.
#
# KNOWN GAPS, not yet fixed. See the tracking issue before trusting a
# run of this script:
#
#   - The CSV stores derived rates. It should store raw counters (PSS
#     KiB, cpu ticks, wakeup count, elapsed seconds) and derive rates at
#     report time, so a reader can recompute rather than trust.
#   - The summary takes independent medians per arm, which throws away
#     the pairing the interleaving creates. Per-round A minus B deltas
#     are the measurement; medians of each arm separately are not.
#   - An odd --rounds leaves the order unbalanced: arm A goes first one
#     more time than arm B. Use an even count until this is enforced.
#   - CPU comes from clock ticks, resolution 0.017% over a 60 s window.
#     /proc/PID/task/*/schedstat field 1 is cumulative on-cpu
#     nanoseconds and has no such floor. Prefer it.
#   - Provenance is printed to the terminal but not saved beside the CSV.

set -u

a_name="a"
a_cmd=""
b_name="b"
b_cmd=""
rounds=3
window_secs=60
warmup_secs=""
settle_secs=5
out_file="measure-compare.csv"

while [ $# -gt 0 ]; do
    case "$1" in
        --a-name) a_name="$2"; shift 2 ;;
        --a-cmd) a_cmd="$2"; shift 2 ;;
        --b-name) b_name="$2"; shift 2 ;;
        --b-cmd) b_cmd="$2"; shift 2 ;;
        --rounds) rounds="$2"; shift 2 ;;
        --window) window_secs="$2"; shift 2 ;;
        --warmup) warmup_secs="$2"; shift 2 ;;
        --settle) settle_secs="$2"; shift 2 ;;
        --out) out_file="$2"; shift 2 ;;
        *) echo "unknown option: $1" >&2; exit 1 ;;
    esac
done

if [ -z "$a_cmd" ] || [ -z "$b_cmd" ]; then
    echo "usage: $0 --a-cmd CMD --b-cmd CMD [options]" >&2
    exit 1
fi

if [ -z "$warmup_secs" ]; then
    warmup_secs="$window_secs"
fi

ticks_per_sec=$(getconf CLK_TCK)

# The measured process is the one this script forked. A nix wrapper execs
# into the real binary and keeps its pid, so the pid stays valid.
started_pid=""

start_arm() {
    local command="$1"
    # shellcheck disable=SC2086
    eval "$command" >/dev/null 2>&1 &
    started_pid=$!
}

stop_arm() {
    if [ -n "$started_pid" ] && [ -d "/proc/$started_pid" ]; then
        kill "$started_pid" 2>/dev/null
        # Give it a moment to release the device before the next arm.
        local waited=0
        while [ -d "/proc/$started_pid" ] && [ "$waited" -lt 10 ]; do
            sleep 1
            waited=$((waited + 1))
        done
    fi
    started_pid=""
}

read_pss_kib() {
    awk '/^Pss:/ { print $2 }' "/proc/$1/smaps_rollup" 2>/dev/null
}

read_cpu_ticks() {
    awk '{ print $14 + $15 }' "/proc/$1/stat" 2>/dev/null
}

# Voluntary context switches are the sensitive metric for a change that
# removes a polling loop. A 100 ms poll is about 10 wakeups a second;
# cpu percent bottoms out at the sampler's resolution long before this
# does.
read_wakeups() {
    awk '/^voluntary_ctxt_switches:/ { print $2 }' "/proc/$1/status" 2>/dev/null
}

read_threads() {
    awk '/^Threads:/ { print $2 }' "/proc/$1/status" 2>/dev/null
}

# One measured round: start, discard a warm-up window, sample a window.
# Appends a CSV row and echoes a human-readable line.
measure_round() {
    local label="$1"
    local command="$2"
    local round="$3"

    start_arm "$command"
    sleep "$warmup_secs"

    if [ ! -d "/proc/$started_pid" ]; then
        echo "  $label round $round: process exited during warm-up, skipped" >&2
        started_pid=""
        return 1
    fi

    local pss_start cpu_start wake_start
    pss_start=$(read_pss_kib "$started_pid")
    cpu_start=$(read_cpu_ticks "$started_pid")
    wake_start=$(read_wakeups "$started_pid")

    sleep "$window_secs"

    if [ ! -d "/proc/$started_pid" ]; then
        echo "  $label round $round: process exited during the window, skipped" >&2
        started_pid=""
        return 1
    fi

    local pss_end cpu_end wake_end threads
    pss_end=$(read_pss_kib "$started_pid")
    cpu_end=$(read_cpu_ticks "$started_pid")
    wake_end=$(read_wakeups "$started_pid")
    threads=$(read_threads "$started_pid")

    stop_arm
    sleep "$settle_secs"

    awk -v label="$label" -v round="$round" \
        -v pss="$pss_end" -v cpu_ticks="$((cpu_end - cpu_start))" \
        -v wakes="$((wake_end - wake_start))" -v threads="$threads" \
        -v hz="$ticks_per_sec" -v secs="$window_secs" -v out="$out_file" \
        'BEGIN {
            cpu_percent = (cpu_ticks / hz) / secs * 100.0
            pss_mib = pss / 1024.0
            wakes_per_sec = wakes / secs
            printf "%s,%d,%.1f,%.3f,%.2f,%d\n", label, round, pss_mib, cpu_percent, wakes_per_sec, threads >> out
            printf "  %-10s round %d: pss %.1f MiB, cpu %.3f%%, wakeups %.2f/s, threads %d\n", label, round, pss_mib, cpu_percent, wakes_per_sec, threads
        }'
}

# EXIT and HUP matter as much as INT: a script that dies any other way
# leaves its arm resident, holding the keyboard against the next run.
trap 'stop_arm' EXIT
trap 'stop_arm; exit 130' INT TERM HUP

echo "arm,round,pss_mib,cpu_percent,wakeups_per_sec,threads" > "$out_file"

echo "environment"
echo "  kernel:    $(uname -r)"
echo "  governor:  $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo unknown)"
echo "  clk_tck:   $ticks_per_sec (one tick over ${window_secs}s = $(awk -v hz="$ticks_per_sec" -v s="$window_secs" 'BEGIN { printf "%.3f", (1/hz)/s*100 }')% of a core)"
echo "  rounds:    $rounds, window ${window_secs}s, warm-up ${warmup_secs}s discarded"
echo "  arm A:     $a_name  <- $a_cmd"
echo "  arm B:     $b_name  <- $b_cmd"
echo

round=1
while [ "$round" -le "$rounds" ]; do
    echo "round $round of $rounds"
    # Alternate which arm goes first so drift within a round does not
    # accumulate against one of them.
    if [ $((round % 2)) -eq 1 ]; then
        measure_round "$a_name" "$a_cmd" "$round"
        measure_round "$b_name" "$b_cmd" "$round"
    else
        measure_round "$b_name" "$b_cmd" "$round"
        measure_round "$a_name" "$a_cmd" "$round"
    fi
    round=$((round + 1))
done

echo
echo "summary (median of $rounds rounds, range in brackets)"
awk -F, 'NR > 1 { rows[$1] = rows[$1] " " NR; pss[$1] = pss[$1] " " $3; cpu[$1] = cpu[$1] " " $4; wake[$1] = wake[$1] " " $5; thr[$1] = thr[$1] " " $6 }
END {
    for (arm in pss) {
        printf "  %s\n", arm
        report(arm, "pss (MiB)   ", pss[arm])
        report(arm, "cpu (%)     ", cpu[arm])
        report(arm, "wakeups/s   ", wake[arm])
        report(arm, "threads     ", thr[arm])
    }
}
function report(arm, name, values,   n, i, sorted, mid, lo, hi, raw) {
    n = split(values, sorted, " ")
    if (n == 0) return
    # insertion sort, n is tiny
    for (i = 2; i <= n; i++) {
        v = sorted[i]; j = i - 1
        while (j > 0 && sorted[j] + 0 > v + 0) { sorted[j+1] = sorted[j]; j-- }
        sorted[j+1] = v
    }
    mid = (n % 2 == 1) ? sorted[(n+1)/2] : (sorted[n/2] + sorted[n/2+1]) / 2.0
    lo = sorted[1]; hi = sorted[n]
    raw = values; sub(/^ /, "", raw); gsub(/ /, ", ", raw)
    printf "    %s median %-8s [%s to %s]   raw: %s\n", name, mid, lo, hi, raw
}' "$out_file"

echo
echo "raw rows written to $out_file"
