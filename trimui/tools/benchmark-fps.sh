#!/bin/sh
# Run on the Trimui over SSH. Small evidence files only; no game data or saves copied.
set -eu
APP=/mnt/SDCARD/Data/ports/nvforever
OUT=${1:?output directory required}
MODE=${2:-baseline}
test -d "$APP/home/saves"
! pidof nvforever >/dev/null 2>&1 || { echo 'Game already running'; exit 1; }
mkdir -p "$OUT"
STAGE=$(mktemp -d /tmp/nvf-benchmark.XXXXXX)
RUN=
SAMPLER=
restore() {
    trap - EXIT HUP INT TERM
    p=$(pidof nvforever 2>/dev/null || true)
    [ -z "$p" ] || kill -TERM $p 2>/dev/null || true
    if [ -n "$RUN" ]; then wait "$RUN" 2>/dev/null || true; fi
    if [ -n "$SAMPLER" ]; then kill "$SAMPLER" 2>/dev/null || true; wait "$SAMPLER" 2>/dev/null || true; fi
    touch /tmp/stay_awake /tmp/stay_alive
    echo nvf_admin_session > /sys/power/wake_lock
    for name in home/NVForever.ini render-size tune log.txt; do
        [ ! -f "$STAGE/$name" ] || cp "$STAGE/$name" "$APP/$name"
    done
    rm -rf "$STAGE"
}
mkdir -p "$STAGE/home"
for name in home/NVForever.ini render-size tune log.txt; do cp "$APP/$name" "$STAGE/$name"; done
trap restore EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM
SAVE=$(ls -t "$APP/home/saves/"*.nvfsave | head -1)
sha256sum "$APP/home/saves/"* > "$OUT/$MODE.saves-before.sha256"
cp "$STAGE/home/NVForever.ini" "$OUT/$MODE.ini"
cp "$APP/extra-env" "$OUT/original-extra-env"
cp "$APP/tune" "$OUT/original-tune"
cp "$APP/render-size" "$OUT/original-render-size"
case "$MODE" in
    baseline|control|candidate) ;;
    profile) command -v python3 >/dev/null || { echo 'Source /mnt/SDCARD/System/etc/ex_config for Python first'; exit 2; }; export NVF_GPU_REPORT=1 ;;
    *) echo 'Unknown mode'; exit 2 ;;
esac
touch /tmp/stay_awake /tmp/stay_alive
echo nvf_admin_session > /sys/power/wake_lock
echo "START mode=$MODE save=$SAVE render=$(cat "$APP/render-size") size_cull=${NVF_SIZE_CULL:-0}"
cat /sys/kernel/debug/pvr/status > "$OUT/$MODE.gpu-before.txt"
cd "$APP"
: > "$APP/log.txt"
sh "$APP/run.sh" --play --load "$SAVE" --run-seconds "${NVF_BENCH_SECONDS:-85}" </dev/null > "$OUT/$MODE.launcher.txt" 2>&1 &
RUN=$!
i=0
while ! grep -q 'loading screen down after' "$APP/log.txt"; do
    kill -0 "$RUN" 2>/dev/null || { echo 'Game exited while loading'; exit 3; }
    i=$((i+1)); [ "$i" -lt 100 ] || { echo 'Load exceeded 200 seconds'; exit 4; }
    sleep 2
done
echo "LOADED mode=$MODE"
if [ "$MODE" = profile ]; then
    python3 "$APP/tools/frame-costs.py" "$(pidof nvforever)" "${NVF_BENCH_SECONDS:-85}" > "$OUT/$MODE.frame-costs.jsonl" &
    SAMPLER=$!
fi
echo 'uptime cpu_khz cpu_mC gpu_mC rss_kB swap_kB available_kB gpu_pct pgmajfault' > "$OUT/$MODE.telemetry.tsv"
while kill -0 "$RUN" 2>/dev/null; do
    p=$(pidof nvforever 2>/dev/null || true)
    [ -n "$p" ] || break
    cpu=$(cat /sys/class/thermal/thermal_zone0/temp)
    [ "$cpu" -lt 85000 ] || { echo "THERMAL_STOP $cpu"; exit 5; }
    printf '%s %s %s %s ' "$(cut -d' ' -f1 /proc/uptime)" "$(cat /sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq)" "$cpu" "$(cat /sys/class/thermal/thermal_zone1/temp)" >> "$OUT/$MODE.telemetry.tsv"
    awk '/VmRSS:/{r=$2}/VmSwap:/{s=$2}END{printf "%s %s ",r,s}' "/proc/$p/status" >> "$OUT/$MODE.telemetry.tsv"
    awk '/MemAvailable:/{printf "%s ",$2}' /proc/meminfo >> "$OUT/$MODE.telemetry.tsv"
    sed -n 's/GPU Utilisation: *\([0-9]*\)%.*/\1/p' /sys/kernel/debug/pvr/status | tr '\n' ' ' >> "$OUT/$MODE.telemetry.tsv"
    awk '/^pgmajfault /{print $2}' /proc/vmstat >> "$OUT/$MODE.telemetry.tsv"
    sleep 3
done
wait "$RUN" || true
RUN=
if [ -n "$SAMPLER" ]; then wait "$SAMPLER" || true; SAMPLER=; fi
touch /tmp/stay_awake /tmp/stay_alive
echo nvf_admin_session > /sys/power/wake_lock
sed 's/\x1b\[[0-9;]*m//g' "$APP/log.txt" | gzip -c > "$OUT/$MODE.log.gz"
grep 'trimui performance' "$APP/log.txt" | tail -6
cat /sys/kernel/debug/pvr/status > "$OUT/$MODE.gpu-after.txt"
sha256sum "$APP/home/saves/"* > "$OUT/$MODE.saves-after.sha256"
cmp "$OUT/$MODE.saves-before.sha256" "$OUT/$MODE.saves-after.sha256"
echo "FINISHED mode=$MODE saves_unchanged=yes"
