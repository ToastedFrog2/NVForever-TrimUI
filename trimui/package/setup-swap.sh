#!/bin/sh
# Makes a swap file for NVForever (size in MB, default 2048) on the internal ext4 storage
# (/mnt/UDISK; the SD card is FAT) and records it in swap-file; run.sh turns it on at
# each launch. Run once over SSH or from a terminal.
set -eu
APP=/mnt/SDCARD/Data/ports/nvforever
MB=${1:-2048}
SWAP=${2:-/mnt/UDISK/nvforever.swap}
if awk -v t="$SWAP" 'NR>1 && $1==t {f=1} END {exit !f}' /proc/swaps; then
    echo "$SWAP is already in use"; exit 0
fi
echo "making a ${MB} MB swap file at $SWAP (this takes a while on an SD card)"
dd if=/dev/zero of="$SWAP" bs=1M count="$MB"
chmod 600 "$SWAP"
mkswap "$SWAP"
swapon -p 10 "$SWAP"
echo "$SWAP" > "$APP/swap-file"
cat /proc/swaps
