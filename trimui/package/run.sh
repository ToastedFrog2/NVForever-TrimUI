#!/bin/sh
# NVForever on the TrimUI Smart Pro (TG5040: A133P, PowerVR GE8300, 1 GB RAM, 1280x720).
# The game's files go in $APP/game/Data (FalloutNV.esm and the .bsa archives) or the
# folder named in $APP/data-path. Settings, saves and logs: $APP/home. Log: $APP/log.txt.
# Quit: hold Menu + Start for a second.
set -u
APP=${NVF_APP_DIR:-/mnt/SDCARD/Data/ports/nvforever}
cd "$APP" || exit 1
if pidof nvforever >/dev/null 2>&1; then
    echo "NVForever is already running." >&2
    exit 1
fi
DATA="$APP/game/Data"
[ -r "$APP/data-path" ] && DATA=$(head -n 1 "$APP/data-path")
: > "$APP/log.txt"
[ ! -r "$APP/build-info.txt" ] || cat "$APP/build-info.txt" >> "$APP/log.txt"
if [ ! -f "$DATA/FalloutNV.esm" ]; then
    echo "FalloutNV.esm not found in $DATA: copy your game's Data folder there (docs: README.txt)" >> "$APP/log.txt"
    exit 2
fi
mkdir -p "$APP/home"
# First start: the built-in pad works the controls (Settings > Controls > Input Device).
if [ ! -f "$APP/home/NVForever.ini" ]; then
    printf '[Controls]\nInput Device=Controller\n[Settings]\nframe-cap=0.20\ndisplay-sync=off\n' > "$APP/home/NVForever.ini"
fi
# extra-env: KEY=VALUE lines exported for the game (NVF_PAD_SWAP_AB=1 and the like).
if [ -r "$APP/extra-env" ]; then
    while IFS= read -r line; do
        case "$line" in [A-Za-z_]*=*) export "$line";; esac
    done < "$APP/extra-env"
fi

# Swap early rather than kill: the game's data is far larger than the RAM (put back on exit).
# NVF_SWAPPINESS=<0-100> sets another value while the game runs; NVF_PAGE_CLUSTER=<n> the
# pages read per swap-in (2^n), both put back on exit (for measuring paging stalls).
SWAPPINESS=$(cat /proc/sys/vm/swappiness 2>/dev/null)
[ -n "$SWAPPINESS" ] && echo "${NVF_SWAPPINESS:-100}" > /proc/sys/vm/swappiness
PAGE_CLUSTER=$(cat /proc/sys/vm/page-cluster 2>/dev/null)
[ -n "$PAGE_CLUSTER" ] && [ -n "${NVF_PAGE_CLUSTER:-}" ] && echo "$NVF_PAGE_CLUSTER" > /proc/sys/vm/page-cluster

# The CPU at its full 2 GHz while the game runs (the other ports here do the same), put
# back on exit: after a fresh boot the ondemand governor held it near 1.2 GHz, 10-12 fps
# where 2 GHz gave 16 (NVF_CPU_PERFORMANCE=0 leaves it alone).
CPU_POLICY=/sys/devices/system/cpu/cpu0/cpufreq
CPU_OLD_GOVERNOR=""
CPU_OLD_MAX=""
# TrimUI's CPU Performance switch, off, runs /usr/trimui/scene/com.trimui.cpuperformance.sh 0,
# which puts ondemand back every 5 s: it waits while the game runs (resumed on exit).
CPU_SCENE_STOPPED=""
if [ "${NVF_CPU_PERFORMANCE:-1}" = 1 ]; then
    for p in $(pgrep -f "cpuperformance.sh 0" 2>/dev/null); do
        kill -STOP "$p" 2>/dev/null && CPU_SCENE_STOPPED="$CPU_SCENE_STOPPED $p"
    done
fi
if [ "${NVF_CPU_PERFORMANCE:-1}" = 1 ] && [ -w "$CPU_POLICY/scaling_governor" ]; then
    CPU_OLD_GOVERNOR=$(cat "$CPU_POLICY/scaling_governor")
    CPU_OLD_MAX=$(cat "$CPU_POLICY/scaling_max_freq")
    cat "$CPU_POLICY/cpuinfo_max_freq" > "$CPU_POLICY/scaling_max_freq" 2>/dev/null
    echo performance > "$CPU_POLICY/scaling_governor" 2>/dev/null || CPU_OLD_GOVERNOR=""
fi

# No keep-awake holds (/tmp/stay_awake, /tmp/stay_alive, a kernel wake lock): with them the
# power button only turned the screen off and the game ran on. Without them TrimUI's keymon
# puts the handheld to sleep about 10 s after the screen goes off, the game with it, and the
# power button wakes it [observed 2026-11-17]. Testing over SSH can still create them by hand.
# Free the screen and RAM: TrimUI's menu, OSD and music service are closed for
# the game and restored after it, also if the launcher is killed (runtime-mainui.sh, from
# the Skyrim and hakuX ports; they share one owner file so none restores another's UI).
[ -r "$APP/runtime-mainui.sh" ] && . "$APP/runtime-mainui.sh"
MAINUI_OWNED=""
for p in $(pidof MainUI 2>/dev/null); do
    state=$(awk '/^State:/ {print $2}' "/proc/$p/status" 2>/dev/null)
    if [ "$state" != T ] && kill -STOP "$p" 2>/dev/null; then MAINUI_OWNED="$MAINUI_OWNED $p"; fi
done
cleanup() {
    command -v nvf_mainui_restore >/dev/null 2>&1 && nvf_mainui_restore
    for p in $MAINUI_OWNED; do kill -CONT "$p" 2>/dev/null || :; done
    [ -z "$SWAPPINESS" ] || echo "$SWAPPINESS" > /proc/sys/vm/swappiness
    [ -z "$PAGE_CLUSTER" ] || echo "$PAGE_CLUSTER" > /proc/sys/vm/page-cluster
    if [ -n "$CPU_OLD_GOVERNOR" ]; then
        echo "$CPU_OLD_GOVERNOR" > "$CPU_POLICY/scaling_governor" 2>/dev/null
        echo "$CPU_OLD_MAX" > "$CPU_POLICY/scaling_max_freq" 2>/dev/null
    fi
    for p in $CPU_SCENE_STOPPED; do kill -CONT "$p" 2>/dev/null || :; done
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM HUP
command -v nvf_mainui_apply >/dev/null 2>&1 && nvf_mainui_apply
# First-run swap setup happens after the UI guards, before starting the game.
if NVF_APP_DIR="$APP" /bin/sh "$APP/setup-swap.sh" --ensure >> "$APP/log.txt" 2>&1; then
    :
else
    swap_error=$?
    if [ -x "$APP/show-loading" ]; then
        if [ "$swap_error" = 20 ]; then "$APP/show-loading" swap-space-error; else "$APP/show-loading" swap-error; fi
        sleep 12
    fi
    exit "$swap_error"
fi
[ ! -x "$APP/show-loading" ] || "$APP/show-loading" >> "$APP/log.txt" 2>&1

export HOME="$APP/home"
export NVFOREVER_HOME="$APP/home"
export XDG_DATA_HOME="$APP/home/.local/share"
export XDG_CACHE_HOME="$APP/home/.cache"
export WGPU_BACKEND=vulkan
export WGPU_SETTINGS_PRIO=${WGPU_SETTINGS_PRIO:-functionality}
export WGPU_POWER_PREF=high
export NVF_DISPLAY_SIZE=1280x720
# The PowerVR display swapchain as the Skyrim port gets it: B8G8R8A8_UNORM, plain views
# (engine/vendor/wgpu-hal swapchain/native.rs `nvf_unorm`).
export NVF_SWAPCHAIN_UNORM=1
# The firmware's PowerVR driver crashes submitting mixed binary/timeline semaphores.
# wgpu's Vulkan fence pool supports the same queue ordering without timeline semaphores.
export NVF_VK_NO_TIMELINE=1
# Use only Bevy's explicitly required features, with the adapter's actual limits.
export NVF_VK_MINIMAL_FEATURES=1
export NVF_SPIRV_LEGACY=1
# Normalize Naga's fragment code before the stock driver's native compiler sees it.
export NVF_SPIRV_OPT="$APP/spirv-normalize"
export NVF_VK_SERIAL_PIPELINES=1
export NVF_LOADING_TIMEOUT=300
export NVF_NO_DEPTH_PREPASS=1
# Advanced full-scene shaders fault on the stock PowerVR driver. The reduced
# diffuse path completed a bounded in-game movement test without GPU recovery.
export NVF_BASIC_RENDERING=${NVF_BASIC_RENDERING:-1}
# The world drawn straight to the 1280x720 screen (native; 0.1 release default, the owner's
# tested setup: about 20-22 fps outdoors with sound). NVF_RENDER_SIZE=640x360 draws the world
# smaller and scales it up, the interface still at 720p (lighter on the GPU, blurrier).
# A render-size file (e.g. 960x540, or native) overrides both and is polled while running.
export NVF_RENDER_SIZE=${NVF_RENDER_SIZE:-native}
# How a frame reaches the display: 0 (none, default here) does not wait for the previous
# frame's GPU work, several fps faster at native than 2 (prev), which waits; with 0, blocks of
# the previous frame can flicker in some interiors. `present=prev` in a tune file, or
# NVF_PRESENT_WAIT=2 in extra-env, brings the wait back (nvforever trimui.rs `set_present_mode`).
export NVF_PRESENT_WAIT=${NVF_PRESENT_WAIT:-0}
# NVF_VIEW_DISTANCE=<units>: caps the far plane (0, the default: the game's own, which the
# sky dome needs; a cap below it left the sky and distant LOD black).
export NVF_VIEW_DISTANCE=${NVF_VIEW_DISTANCE:-0}
# NVF_SKY_HOUR=<0-24>: the sky always shows this hour (the game's clock runs on). Every
# material is unlit here, so the world looks like day at any hour; a dawn or night sky over
# it was black (main.rs `sky_follows_game`). Empty: the sky follows the game's clock.
export NVF_SKY_HOUR=${NVF_SKY_HOUR-12}
# The camera's cell is loaded in full (--radius 0), and each neighbouring cell once the
# camera is within this many units of it (a cell is 4096 wide; world.rs `Streamer::lead`).
# 6000 reaches every neighbour: the 3 x 3 cells around the player, let go one step beyond
# (Goodsprings: 524 MB of heap, 202 MB of GPU blocks, 16-18 fps, against 21.5 for the one
# cell). 1500: the next cell only as the player nears it
# (two loaded on most ground, four at a corner). 0: the one cell alone.
export NVF_CELL_LEAD=${NVF_CELL_LEAD:-6000}
# NVF_TERRAIN_LAYER_DISTANCE=<units>: a quadrant's terrain texture layers drawn only within
# this distance, its base texture beyond (terrain.rs `layer_distance`). Off: with nine cells
# loaded, 3000 and everywhere both ran 16.3-16.4 fps (the CPU sets the frame).
export NVF_TERRAIN_LAYER_DISTANCE=${NVF_TERRAIN_LAYER_DISTANCE:-0}
export NVF_RENDER_SIZE_FILE="$APP/render-size"
# Match the handheld's A/B labels to the game's confirm/cancel buttons.
export NVF_PAD_SWAP_AB=${NVF_PAD_SWAP_AB:-1}
# glibc's malloc keeps an arena per thread; two keep the heap from spreading over the 1 GB.
export MALLOC_ARENA_MAX=2
# Every 20 s the heap's free pages go back to the kernel (trimui.rs `start_malloc_trim`): the
# start's tables are built and dropped in pieces; kept, their pages went to swap (about
# 160 MB less in swap).
export NVF_MALLOC_TRIM=${NVF_MALLOC_TRIM:-20}
# Bevy's mesh slabs make their allocation bookkeeping for 128 Ki meshes up front, about 4 MB
# a slab; 16 Ki is plenty here (a full slab is passed over for another).
export NVF_SLAB_MAX_ALLOCS=${NVF_SLAB_MAX_ALLOCS:-16384}
# At most one empty GPU memory block kept for reuse (the PC build keeps two, against block
# churn); a block's allocation takes up to 0.8 s on this driver, so not none.
export NVF_GPU_KEEP_EMPTY_BLOCKS=${NVF_GPU_KEEP_EMPTY_BLOCKS:-1}
# Particles, decals, weather effects, ripples, window glow and flickering lights are not
# run (nvforever main.rs `effects_on`); physics steps 30 times a second, not 60.
export NVF_LITE_EFFECTS=${NVF_LITE_EFFECTS:-1}
export NVF_PHYS_HZ=${NVF_PHYS_HZ:-30}
# Actors further than this from the camera (game units, at least 3000: beyond foot placement
# and head turns) are posed every other frame (nvforever locomotion.rs `pose_skip_radius`;
# 0: every frame).
export NVF_POSE_SKIP_RADIUS=${NVF_POSE_SKIP_RADIUS:-3000}
# Lamps' glow cards, light beams and window rays (additive shapes of placed models) are not
# drawn: close to a lit lamp they filled the screen and the fps fell (trimui.rs `hide_glows`).
export NVF_NO_GLOWS=${NVF_NO_GLOWS:-1}
# NVF_SIZE_CULL=<n>: an object or item mesh is drawn no farther than n times its bounds'
# radius (draw_distance.rs `size_cull`); 0: off. With nine cells loaded the frame is set by
# how many meshes are drawn: 480x270 ran as fast as 640x360, the GPU busy either way.
export NVF_SIZE_CULL=${NVF_SIZE_CULL:-0}
# NVF_HIDE_MODELS=<texts>: models whose path contains any of these comma-separated texts are
# not drawn (--hide; SpeedTree plants by their .spt, creatures by their skeleton's folder).
# By default the desert's bushes and tumbleweeds (owner request 2026-11-06): many meshes for
# what they add on the small screen. Also the placed weather-like effects (owner request
# 2026-11-09): dust devils (fxdustwhirlwind01), drifting and hanging dust (fxambdust,
# fxdustsimple, fxdustmesh, fxdustmyst, fxlightdust) and mist (fxmist, fxmountmist,
# fxrivermist, fxcloudside); smoke, fire, explosions and the radiation dust stay. The dust
# storm columns (nvsandstorm, nvlimestoneduststorm) and circling vultures (fxvultures) too
# (owner request 2026-11-11).
# NVF_HIDE_MODELS=none draws everything.
NVF_HIDE_MODELS=${NVF_HIDE_MODELS:-creosotebush,sagebrush,wastelandshrub,euonymusbush,tumbleweed,fxdustwhirlwind,fxambdust,fxdustsimple,fxdustmesh,fxdustmyst,fxlightdust,fxmist,fxmountmist,fxrivermist,fxcloudside,nvsandstorm,nvlimestoneduststorm,fxvultures}
if [ "$NVF_HIDE_MODELS" = none ]; then NVF_HIDE_MODELS=""; fi
# At most two texture layers blended over each terrain quadrant (each is a pass of the
# whole quadrant); the two that cover the most ground.
export NVF_TERRAIN_LAYERS=${NVF_TERRAIN_LAYERS:-2}
# Each opaque or alpha-tested shape's triangles reordered for the GPU's vertex cache when
# loaded: 1.25 vertices shaded per triangle become 1.01, about +0.4 fps at native 1280x720
# where the town models' geometry is a large part of the frame (2026-11-11); the picture
# is the same (nif_spawn.rs `vertex_cache_order`). NVF_VCACHE=0 in extra-env turns it off.
export NVF_VCACHE=${NVF_VCACHE:-1}
# The 3D scene drawn in one render pass with its depth not written out at the end (vendored
# bevy_core_pipeline `NVF_ONE_MAIN_PASS`): the tile-based GPU keeps colour and depth on chip
# between the opaque and the blended draws. 15.0 -> 15.7 fps at the native town view
# (2026-11-11). NVF_ONE_MAIN_PASS=0 in extra-env (or mainpass=0 in the tune file) is Bevy's two.
export NVF_ONE_MAIN_PASS=${NVF_ONE_MAIN_PASS:-2}
# No cloud layers (owner's choice 2026-11-11): about 1.5 fps in town at 1280x720.
# NVF_NO_CLOUDS=0 in extra-env brings them back.
export NVF_NO_CLOUDS=${NVF_NO_CLOUDS:-1}
# The frame's schedules run on one thread: on four Cortex-A53s handing hundreds of small
# systems to worker threads cost more than it saved (outdoors 21-22 fps against 17-18).
export NVF_SERIAL_SCHEDULES=${NVF_SERIAL_SCHEDULES:-1}
# The compass gathers what is near four times a second, not ten.
export NVF_COMPASS_POLL=${NVF_COMPASS_POLL:-0.25}
# Actors' AI packages are looked over once a second, not four times (the choice itself is
# due every few game minutes).
export NVF_AI_SCAN_STEP=${NVF_AI_SCAN_STEP:-1}
# Sound on (0.1 release default; it costs about 1.4 fps). NVF_NO_AUDIO=1 in extra-env turns
# it off: the sound systems then do not run at all and the voice archive is not mounted.
export NVF_NO_AUDIO=${NVF_NO_AUDIO:-0}
# Without sound the voice archive is not mounted: its 105,517 files were two thirds of the
# archive index's memory. A line without its voice file lasts as long as its text
# (nvf-play voice.rs) and the lips stay still. The list is the base game's (with DLC, set
# NVF_ARCHIVE_ORDER in extra-env, or NVF_NO_AUDIO=0).
if [ "$NVF_NO_AUDIO" = 1 ] && [ -z "${NVF_ARCHIVE_ORDER:-}" ]; then
    export NVF_ARCHIVE_ORDER="Fallout - Textures.bsa, Fallout - Textures2.bsa, Fallout - Meshes.bsa, Fallout - Sound.bsa, Fallout - Misc.bsa, Update.bsa"
fi
if [ "$NVF_NO_AUDIO" != 1 ]; then
    # The system's mixer (dmix) leaves cpal 0.17 silent: its stream is prepared but not
    # started, and cpal polls before its first write, so the first write never comes. The
    # game's own ALSA setting goes past dmix to the chip at 16-bit, the system mixer's
    # format: the chip played cpal's own pick, 24-bit, inaudibly [observed 2026-11-13]. The
    # rate is left to the game (below); a fixed 48 kHz here made cpal's open fail, so the game
    # took the silent "null" device. The device volume ("Soft Volume Master") still applies.
    if [ ! -e "$HOME/.asoundrc" ]; then
        cat > "$HOME/.asoundrc" <<'ASOUNDRC'
# Written by run.sh (NVForever): straight to the chip at 16-bit, the device volume kept.
pcm.!default {
    type plug
    slave {
        pcm "nvfSoftvol"
        format S16_LE
        channels 2
    }
}
pcm.nvfSoftvol {
    type softvol
    slave.pcm "hw:audiocodec,0"
    control {
        name "Soft Volume Master"
        card audiocodec
    }
    min_dB -51.0
    max_dB 0.0
    resolution 256
}
ctl.!default {
    type hw
    card audiocodec
}
ASOUNDRC
    fi
    # The menu closed above can hold the chip a moment longer. The game opens its sound once
    # at start and keeps the silent "null" device if the chip is busy, so wait up to 5 s.
    snd_busy() {
        for fd in /proc/[0-9]*/fd/*; do
            case "$(readlink "$fd" 2>/dev/null)" in /dev/snd/pcmC*p) return 0;; esac
        done
        return 1
    }
    # The chip is opened at the mixer's 48 kHz, not cpal's default 44.1 kHz, so the mix is
    # not resampled on the way out and the layered music (48 kHz files) not on the way in
    # (nvf-play audio_out.rs `device_rate`; sound thread 25 % → 22 % of a core, 2026-11-13).
    export NVF_AUDIO_DEVICE_RATE=${NVF_AUDIO_DEVICE_RATE:-mixer}
    tries=0
    while snd_busy && [ $tries -lt 25 ]; do sleep .2; tries=$((tries + 1)); done
    [ $tries = 0 ] || echo "NVForever sound: waited $((tries * 2 / 10)) s for the sound chip to be free" >> "$APP/log.txt"
fi
# The scripts' source text and compiled bytes are let go once decoded (fnv-game
# `GameData::drop_script_text`).
export NVF_LEAN_SCRIPTS=${NVF_LEAN_SCRIPTS:-1}
# World textures at most 128 texels a side: the GPU cannot sample the game's DXT textures,
# so they are decoded to RGBA8 (4-8 times larger); leaving out the largest levels keeps
# them near their original size in memory.
export NVF_TEXTURE_MAX_SIDE=${NVF_TEXTURE_MAX_SIDE:-128}
# Every BC texture decoded on the CPU, also where a caller does not say what the GPU takes.
export NVF_NO_BC_TEXTURES=1
# Compiled GPU pipelines kept between runs: the driver takes seconds for each and the world
# is not drawn until they are ready (engine/vendor/wgpu-hal device.rs `nvf_disk_cache`).
mkdir -p "$APP/home/cache"
# The pipeline cache, rewritten after each new pipeline, on the internal ext4 storage.
export NVF_VK_PIPELINE_CACHE=/mnt/UDISK/nvforever-vk-pipelines.bin
# NVF_BIN_DIR=<dir>: the program from there instead (e.g. /mnt/UDISK/nvforever-bin, with
# schemas/ beside it). Not the default: on the internal storage the program's pages compete
# with the swap file for one flash chip, and loads took minutes longer.
GAME=./nvforever
BIN_DIR=${NVF_BIN_DIR:-}
if [ -n "$BIN_DIR" ] && [ -x "$BIN_DIR/nvforever" ]; then
    GAME="$BIN_DIR/nvforever"
    [ -e "$BIN_DIR/schemas" ] || ln -s "$APP/schemas" "$BIN_DIR/schemas"

fi
# Raise it in a launcher-local file (one line: extra arguments), e.g. "--radius 2".
EXTRA=""
[ -r "$APP/extra-args" ] && EXTRA=$(head -n 1 "$APP/extra-args")
# Distant LOD's reach in game units (NVF_LOD_DISTANCE=0: none).
LOD_DISTANCE=${NVF_LOD_DISTANCE:-30000}
if [ "$LOD_DISTANCE" = 0 ]; then
    LOD="--no-lod"
else
    LOD="--lod-distance $LOD_DISTANCE --lod-near-distance $((LOD_DISTANCE * 2 / 5))"
fi
export RUST_LOG=${RUST_LOG:-info,wgpu_core=warn,wgpu_hal=warn,wgpu_hal::vulkan::khr_display=info,naga=warn}
export RUST_BACKTRACE=1
# Low-spec defaults: the current cell in full; beyond it the game's own distant LOD terrain and
# object blocks out to 30,000 units (cheap for the GPU, little memory); no grass, shadows or
# multisampling; the frame rate shown.
# shellcheck disable=SC2086
"$GAME" --data "$DATA" --size 1280x720 --radius 0 --no-grass --no-lodgen --fog 6000,30000 \
    $LOD --tree-distance 0 ${NVF_HIDE_MODELS:+--hide "$NVF_HIDE_MODELS"} \
    --no-sun-shadows --msaa 1 --graphics-only no-glow,no-eye-adaptation,no-neon-glow,no-water-mirror \
    --no-head-tracking --no-foot-ik --fps $EXTRA "$@" >> "$APP/log.txt" 2>&1
RESULT=$?
echo "exit $RESULT" >> "$APP/log.txt"
sync
exit "$RESULT"
