#!/bin/bash
# Copies trimui/dist to the device over SSH: nvforever/ to /mnt/SDCARD/Data/ports/nvforever
# and NVForever.sh to /mnt/SDCARD/Roms/PORTS. Usage: TRIMUI_SSH=root@DEVICE-IP trimui/deploy.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
: "${TRIMUI_SSH:?set TRIMUI_SSH=root@DEVICE-IP}"
opts=(-o ServerAliveInterval=5 -o ServerAliveCountMax=6 -o ConnectTimeout=10)
cd "$here/dist"
COPYFILE_DISABLE=1 tar cf - nvforever NVForever.sh | gzip -1 | ssh "${opts[@]}" "$TRIMUI_SSH" '
    set -e
    mkdir -p /mnt/SDCARD/Data/ports/nvforever.new /mnt/SDCARD/Roms/PORTS
    cd /mnt/SDCARD/Data/ports/nvforever.new
    gzip -dc | tar xf -
    mkdir -p ../nvforever
    cp -R nvforever/. ../nvforever/
    cp NVForever.sh /mnt/SDCARD/Roms/PORTS/
    cd .. && rm -rf nvforever.new
    sync
    ls -la /mnt/SDCARD/Data/ports/nvforever'
