#!/bin/bash
# PORTMASTER: nvforever.zip, NVForever.sh
# Native evdev controls and Menu+Start exit are implemented by the engine.
for pm_dir in /mnt/SDCARD/Apps/PortMaster/PortMaster /opt/system/Tools/PortMaster /opt/tools/PortMaster "${XDG_DATA_HOME:-$HOME/.local/share}/PortMaster" /roms/ports/PortMaster; do
    if [ -f "$pm_dir/control.txt" ]; then controlfolder="$pm_dir"; break; fi
done
if [ -z "${controlfolder:-}" ]; then
    echo "PortMaster is required to launch NVForever." >&2
    exit 1
fi
source "$controlfolder/control.txt"
[ ! -f "$controlfolder/mod_${CFW_NAME}.txt" ] || source "$controlfolder/mod_${CFW_NAME}.txt"
if [ "${DEVICE_NAME:-}" != "TrimUI Smart Pro" ] && [ ! -d /usr/trimui ]; then
    echo "This experimental build requires the TrimUI Smart Pro Vulkan driver." >&2
    exit 1
fi
get_controls
export NVF_APP_DIR="/${directory#/}/ports/nvforever"
export SDL_GAMECONTROLLERCONFIG="${sdl_controllerconfig:-}"
pm_platform_helper "$NVF_APP_DIR/nvforever"
/bin/sh "$NVF_APP_DIR/run.sh" "$@"
result=$?
pm_finish
exit "$result"
