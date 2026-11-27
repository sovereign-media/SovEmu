#!/usr/bin/env bash
# Runs one command and samples what it uses until it exits (sovereign-media/SovEmu#282): run
# inside the image, one worker per container, so the container's cgroup is the worker's.
#
#   sample.sh OUT_DIR -- COMMAND...
#
# Writes OUT_DIR/samples.csv (every 200 ms) and OUT_DIR/summary.json: peak resident, virtual
# and private writable (RLIMIT_DATA's) memory, the cgroup's peak (which also counts the page
# cache the worker read), threads, open files, and CPU seconds.
set -euo pipefail
out="$1"; shift; [ "$1" = "--" ] && shift
mkdir -p "$out"
tick=$(getconf CLK_TCK)
"$@" >"$out/stdout.log" 2>"$out/stderr.log" &
pid=$!
start=$(date +%s.%N)
echo "t,rss_kb,vm_kb,data_kb,anon_kb,file_kb,threads,fds,cpu_s,cgroup_kb" >"$out/samples.csv"
max_threads=0; max_fds=0; max_data=0; hwm=0; peak=0; cpu=0
while [ -d "/proc/$pid" ] && [ "$(cut -d' ' -f3 "/proc/$pid/stat" 2>/dev/null)" != Z ]; do
  status="$(cat "/proc/$pid/status" 2>/dev/null)" || break
  field() { awk -v k="$1:" '$1 == k { print $2 }' <<<"$status"; }
  rss=$(field VmRSS); vm=$(field VmSize); data=$(field VmData); anon=$(field RssAnon); file=$(field RssFile)
  threads=$(field Threads); hwm=$(field VmHWM); peak=$(field VmPeak)
  fds=$( (ls "/proc/$pid/fd" 2>/dev/null || true) | wc -l)
  read -r ut st < <(awk '{ print $14, $15 }' "/proc/$pid/stat" 2>/dev/null) || break
  cpu=$(awk -v u="$ut" -v s="$st" -v t="$tick" 'BEGIN { printf "%.2f", (u + s) / t }')
  cg=$(( $(cat /sys/fs/cgroup/memory.current) / 1024 ))
  now=$(awk -v n="$(date +%s.%N)" -v s="$start" 'BEGIN { printf "%.1f", n - s }')
  echo "$now,$rss,$vm,$data,$anon,$file,$threads,$fds,$cpu,$cg" >>"$out/samples.csv"
  [ "${threads:-0}" -gt "$max_threads" ] && max_threads=$threads
  [ "$fds" -gt "$max_fds" ] && max_fds=$fds
  [ "${data:-0}" -gt "$max_data" ] && max_data=$data
  sleep 0.2
done
wait "$pid" && code=0 || code=$?
wall=$(awk -v n="$(date +%s.%N)" -v s="$start" 'BEGIN { printf "%.1f", n - s }')
cg_peak=$(( $(cat /sys/fs/cgroup/memory.peak 2>/dev/null || echo 0) / 1024 ))
printf '{"exit":%s,"wall_s":%s,"cpu_s":%s,"rss_peak_kb":%s,"vm_peak_kb":%s,"data_peak_kb":%s,"cgroup_peak_kb":%s,"threads_max":%s,"fds_max":%s}\n' \
  "$code" "$wall" "$cpu" "${hwm:-0}" "${peak:-0}" "$max_data" "$cg_peak" "$max_threads" "$max_fds" | tee "$out/summary.json"
