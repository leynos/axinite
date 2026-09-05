#!/usr/bin/env bash
# Sample peak memory use and minimum free disk while a CI job runs.
#
# Right-sizing a runner needs two numbers the job does not otherwise report:
# how much memory it actually used at its peak, and how close it came to
# filling the volume. Both belong to the shape, not to the workload: Cargo
# scales parallelism with the processor count, so a peak measured on one shape
# does not transfer to another. Sample on the shape you intend to use.
#
# Usage:
#   ci-resource-sampler.sh start    # begin sampling in the background
#   ci-resource-sampler.sh report   # stop, print the peaks, write the summary
#
# The sampler wakes every SAMPLER_INTERVAL_SECONDS (default 10) and shells out,
# which is negligible for a compile job and unacceptable for one whose output
# is a timing. Do not run it on a benchmark job.

set -euo pipefail

STATE_DIR="${RUNNER_TEMP:-/tmp}/ci-resource-sampler"
PEAK_FILE="${STATE_DIR}/peak-memory-kib"
FREE_FILE="${STATE_DIR}/min-free-kib"
PID_FILE="${STATE_DIR}/sampler.pid"
INTERVAL="${SAMPLER_INTERVAL_SECONDS:-10}"

# Memory in use is total minus available, not total minus free: page cache is
# reclaimable, and counting it as used would report every job as near its
# limit.
used_memory_kib() {
  awk '/^MemTotal:/ {t = $2} /^MemAvailable:/ {a = $2} END {print t - a}' \
    /proc/meminfo
}

free_disk_kib() {
  df -Pk . | awk 'NR == 2 {print $4}'
}

# Write through a temporary file and rename, so a reader sees either the old
# value or the new one and never a truncated file. This is not sufficient on
# its own, because the update is a read-modify-write, but it removes the
# empty-file window that `set -u` would turn into a failed report.
write_atomic() {
  local path="$1" value="$2"
  printf '%s\n' "${value}" > "${path}.tmp"
  mv -f "${path}.tmp" "${path}"
}

sample_once() {
  local used free previous
  used="$(used_memory_kib)"
  free="$(free_disk_kib)"
  previous="$(cat "${PEAK_FILE}")"
  if [ "${used}" -gt "${previous}" ]; then
    write_atomic "${PEAK_FILE}" "${used}"
  fi
  previous="$(cat "${FREE_FILE}")"
  if [ "${free}" -lt "${previous}" ]; then
    write_atomic "${FREE_FILE}" "${free}"
  fi
}

start_sampler() {
  mkdir -p "${STATE_DIR}"
  printf '0\n' > "${PEAK_FILE}"
  # Seed the minimum with a value no real volume can exceed, so the first
  # sample always wins.
  printf '%s\n' "$((1 << 60))" > "${FREE_FILE}"
  (
    while :; do
      sample_once
      sleep "${INTERVAL}"
    done
  ) &
  printf '%s\n' "$!" > "${PID_FILE}"
  printf 'resource sampler: started, interval %ss, state %s\n' \
    "${INTERVAL}" "${STATE_DIR}"
}

# Stop the background loop and wait for it to go. Killing it and reading
# immediately races the loop's own read-modify-write: the report can catch a
# state file mid-rewrite, read an empty value, and then fail on arithmetic
# under `set -u`. Waiting is the only way to be sure no writer remains.
stop_sampler() {
  local pid waited
  [ -f "${PID_FILE}" ] || return 0
  pid="$(cat "${PID_FILE}")"
  [ -n "${pid}" ] || return 0
  kill "${pid}" 2>/dev/null || true
  # `wait` applies only to a child of this shell, which the loop is when
  # `report` runs in the same process as `start`. It is not when they are
  # separate steps, so poll for the process to disappear as well.
  wait "${pid}" 2>/dev/null || true
  waited=0
  while kill -0 "${pid}" 2>/dev/null; do
    sleep 1
    waited=$((waited + 1))
    if [ "${waited}" -ge 15 ]; then
      printf 'resource sampler: sampler %s did not exit; killing\n' "${pid}"
      kill -9 "${pid}" 2>/dev/null || true
      break
    fi
  done
  rm -f "${PID_FILE}"
}

report_sampler() {
  stop_sampler
  if [ ! -f "${PEAK_FILE}" ] || [ ! -f "${FREE_FILE}" ]; then
    printf 'resource sampler: no samples recorded\n'
    return 0
  fi
  # Take one final sample, so a job that finishes inside a single interval
  # still reports a real figure rather than the seed values. No writer remains
  # by this point, so this cannot race.
  sample_once
  local peak_mib free_mib total_mib
  peak_mib="$(( $(cat "${PEAK_FILE}") / 1024 ))"
  free_mib="$(( $(cat "${FREE_FILE}") / 1024 ))"
  total_mib="$(( $(awk '/^MemTotal:/ {print $2}' /proc/meminfo) / 1024 ))"
  printf 'resource sampler: peak memory used %s MiB of %s MiB\n' \
    "${peak_mib}" "${total_mib}"
  printf 'resource sampler: minimum free disk %s MiB\n' "${free_mib}"
  df -h .
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      printf '### Resource sampler (%s)\n\n' "${GITHUB_JOB:-job}"
      printf '| Measure | Value |\n| --- | --- |\n'
      printf '| Peak memory used | %s MiB of %s MiB |\n' \
        "${peak_mib}" "${total_mib}"
      printf '| Minimum free disk | %s MiB |\n' "${free_mib}"
    } >> "${GITHUB_STEP_SUMMARY}"
  fi
}

case "${1:-}" in
  start) start_sampler ;;
  report) report_sampler ;;
  *)
    printf 'usage: %s {start|report}\n' "${0##*/}" >&2
    exit 2
    ;;
esac
