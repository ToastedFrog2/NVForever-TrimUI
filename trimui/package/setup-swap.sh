#!/bin/sh
# Ensure swap exists on internal storage. Existing files are never reformatted.
# run.sh calls --ensure; manual use: setup-swap.sh [MiB] [absolute-file-path].
set -eu
APP=${NVF_APP_DIR:-/mnt/SDCARD/Data/ports/nvforever}
INTERNAL=${NVF_SWAP_INTERNAL_DIR:-/mnt/UDISK}
PROC_SWAPS=${NVF_PROC_SWAPS:-/proc/swaps}
PROC_MOUNTS=${NVF_PROC_MOUNTS:-/proc/mounts}
LOCK="$APP/.swap-setup.lock"
TMP=""; RECORD=""; CHILD=""; OWNED=0; AUTO=0
fail() { echo "swap setup: $2" >&2; exit "$1"; }
active() { awk -v t="$1" 'NR>1 && $1==t {f=1} END {exit !f}' "$PROC_SWAPS"; }
cleanup() {
    if [ -n "$CHILD" ]; then kill "$CHILD" 2>/dev/null || :; wait "$CHILD" 2>/dev/null || :; fi
    [ -z "$TMP" ] || { [ ! -f "$TMP" ] || rm -- "$TMP"; }
    [ -z "$RECORD" ] || { [ ! -f "$RECORD" ] || rm -- "$RECORD"; }
    if [ "$OWNED" = 1 ]; then
        for f in temp target pid start boot; do [ ! -f "$LOCK/$f" ] || rm -- "$LOCK/$f"; done
        rmdir "$LOCK" 2>/dev/null || :
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM
record() {
    if [ ! -r "$APP/swap-file" ] || [ "$(head -n 1 "$APP/swap-file")" != "$SWAP" ]; then
        RECORD=$(mktemp "$APP/.swap-file.XXXXXX")
        printf '%s\n' "$SWAP" > "$RECORD"
        mv "$RECORD" "$APP/swap-file"
        RECORD=""
    fi
}
if [ "${1:-}" = --ensure ]; then
    [ "$#" = 1 ] || fail 2 "unexpected arguments"
    AUTO=1
    MB=2048
    if [ -s "$APP/swap-file" ]; then SWAP=$(head -n 1 "$APP/swap-file"); else SWAP="$INTERNAL/nvforever.swap"; fi
else
    [ "$#" -le 2 ] || fail 2 "usage: setup-swap.sh [MiB] [absolute-file-path]"
    MB=${1:-2048}; SWAP=${2:-$INTERNAL/nvforever.swap}
fi
case "$MB" in ''|*[!0-9]*) fail 2 "size must be an integer in MiB";; esac
while [ "${MB#0}" != "$MB" ]; do MB=${MB#0}; done
[ -n "$MB" ] || MB=0
[ "$MB" -ge 64 ] && [ "$MB" -le 8192 ] || fail 2 "size must be between 64 and 8192 MiB"
case "$SWAP" in /*) ;; *) fail 2 "swap path must be absolute";; esac
case "$SWAP" in *[[:space:]]*) fail 2 "swap path must not contain whitespace";; esac
[ -r "$PROC_SWAPS" ] || fail 22 "cannot read active swap information"
[ -d "$APP" ] || fail 22 "port directory is missing"
[ ! -L "$LOCK" ] || fail 24 "unexpected setup lock"

# Recover only this helper's recorded temporary file after a crash/reboot.
boot=$(cat /proc/sys/kernel/random/boot_id 2>/dev/null || echo unknown)
start_of() { awk '{print $22}' "/proc/$1/stat" 2>/dev/null || :; }
if ! mkdir "$LOCK" 2>/dev/null; then
    pid=$(cat "$LOCK/pid" 2>/dev/null || :)
    oldboot=$(cat "$LOCK/boot" 2>/dev/null || :)
    oldstart=$(cat "$LOCK/start" 2>/dev/null || :)
    case "$pid" in ''|*[!0-9]*) fail 24 "another setup is starting; retry shortly";; esac
    if [ "$oldboot" = "$boot" ] && [ "$oldstart" = "$(start_of "$pid")" ] && kill -0 "$pid" 2>/dev/null; then
        fail 24 "another swap setup is already running"
    fi
    oldtmp=$(cat "$LOCK/temp" 2>/dev/null || :)
    oldtarget=$(cat "$LOCK/target" 2>/dev/null || :)
    case "$oldtmp" in
        "$oldtarget".setup.??????)
            if [ -f "$oldtmp" ] && ! active "$oldtmp"; then rm -- "$oldtmp"; fi;;
    esac
    for f in temp target pid start boot; do [ ! -f "$LOCK/$f" ] || rm -- "$LOCK/$f"; done
    rmdir "$LOCK" 2>/dev/null || fail 24 "unexpected files in old setup lock"
    mkdir "$LOCK" 2>/dev/null || fail 24 "another setup started; retry shortly"
fi
OWNED=1
printf '%s\n' "$$" > "$LOCK/pid"
printf '%s\n' "$boot" > "$LOCK/boot"
start_of "$$" > "$LOCK/start"
printf '%s\n' "$SWAP" > "$LOCK/target"

# Another port may already provide a sufficient internal swap file. Share it.
if [ "$AUTO" = 1 ] && [ ! -e "$SWAP" ] && [ ! -L "$SWAP" ] && [ ! -s "$APP/swap-file" ]; then
    shared=$(awk -v p="$INTERNAL/" -v k="$((MB * 1024 - 1024))" 'NR>1 && $2=="file" && index($1,p)==1 && $3>=k {print $1; exit}' "$PROC_SWAPS")
    [ -z "$shared" ] || SWAP=$shared
fi
if active "$SWAP"; then
    size=$(awk -v p="$SWAP" '$1==p {print $3; exit}' "$PROC_SWAPS")
    [ "$size" -ge "$((MB * 1024 - 1024))" ] || fail 21 "configured active swap is smaller than required"
    record
    echo "swap ready: reusing active $SWAP"
    exit 0
fi
[ ! -L "$SWAP" ] || fail 21 "refusing a symbolic-link swap file"
if [ -e "$SWAP" ]; then
    [ -f "$SWAP" ] || fail 21 "configured path is not a regular swap file"
    size=$(stat -c %s "$SWAP")
    [ "$size" -ge "$((MB * 1024 * 1024))" ] || fail 21 "existing file is too small; left unchanged"
    swapon -p 10 "$SWAP" || fail 21 "existing file could not be enabled; left unchanged"
    chmod 600 "$SWAP"
    record
    echo "swap ready: enabled existing $SWAP"
    exit 0
fi

DIR=${SWAP%/*}
[ -d "$DIR" ] && [ -w "$DIR" ] || fail 22 "internal storage is unavailable or read-only"
fs=$(awk -v p="$DIR/" '($2=="/" || index(p,$2"/")==1) && length($2)>n {n=length($2);f=$3} END {print f}' "$PROC_MOUNTS")
[ "$fs" = ext4 ] || fail 22 "new swap requires mounted ext4 storage; no file created"
available=$(df -Pk "$DIR" | awk 'END {print $4}')
case "$available" in ''|*[!0-9]*) fail 22 "cannot determine free storage";; esac
required=$(((MB + 256) * 1024))
[ "$available" -ge "$required" ] || fail 20 "need $((MB + 256)) MiB free on internal storage for setup"
[ ! -x "$APP/show-loading" ] || "$APP/show-loading" swap || :
echo "Creating ${MB} MiB swap on internal storage at $SWAP; this can take several minutes."
umask 077
TMP=$(mktemp "$SWAP.setup.XXXXXX")
printf '%s\n' "$TMP" > "$LOCK/temp"
dd if=/dev/zero of="$TMP" bs=1M count="$MB" &
CHILD=$!
if wait "$CHILD"; then CHILD=""; else CHILD=""; fail 23 "allocation failed; temporary file removed"; fi
mkswap "$TMP" || fail 23 "swap initialization failed"
sync
# A hard link publishes the prepared file without replacing an existing file.
ln "$TMP" "$SWAP" || fail 23 "destination already exists or could not be created"
rm -- "$TMP"; TMP=""
swapon -p 10 "$SWAP" || fail 23 "swap could not be enabled; prepared file kept for retry"
record
echo "swap ready: created $SWAP"
