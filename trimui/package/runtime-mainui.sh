#!/bin/sh
# Automatic MainUI + OSD + musicserver release for an NVForever launch; no firmware
# edits. From the Skyrim port, sharing hakuX's /tmp/hakux-ui-owner files so one game's guard
# never restores another's UI.
nvf_ui_process_start() { awk '{print $22}' "/proc/$1/stat" 2>/dev/null; }
nvf_ui_osd_alive() {
    for osd_check in $(pidof trimui_osdd 2>/dev/null); do
        case "$(awk '/^State:/ {print $2}' "/proc/$osd_check/status" 2>/dev/null)" in Z|X|'') continue;; esac
        return 0
    done
    return 1
}
nvf_mainui_restore() {
    # Share ownership with hakuX so an older guard cannot restore a newer game's UI.
    [ "$(cat /tmp/hakux-ui-owner 2>/dev/null)" = "${NVF_UI_TOKEN:-}" ] && [ -n "${NVF_UI_TOKEN:-}" ] || return 0
    if [ -r /tmp/nvforever-ui.music ]; then
        music_exe=$(cat /tmp/nvforever-ui.music)
        if ! pidof musicserver >/dev/null 2>&1 && [ -x "$music_exe" ]; then
            (
                cd "$(dirname "$music_exe")" || exit
                export LD_LIBRARY_PATH="${music_exe%/*}/../lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
                exec "$music_exe"
            ) </dev/null >/dev/null 2>&1 &
        fi
        rm -f /tmp/nvforever-ui.music
    fi
    if [ "${NVF_UI_OSD_CLOSED:-0}" = 1 ] && [ -n "${NVF_UI_OSD_EXE:-}" ]; then
        if ! nvf_ui_osd_alive; then
            (
                cd "$(dirname "$NVF_UI_OSD_EXE")" || exit
                export LD_LIBRARY_PATH="${NVF_UI_OSD_LIB}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
                exec "$NVF_UI_OSD_EXE"
            ) </dev/null >/dev/null 2>&1 &
            for attempt in 1 2 3 4 5 6 7 8 9 10; do
                nvf_ui_osd_alive && break
                sleep .1
            done
        fi
    fi
    # Share hakuX's recovery file so a guard can resume only the processes
    # paused for this launch, even when the launcher is killed.
    if [ -r /tmp/hakux-ui-owner.music ]; then
        for music_entry in $(cat /tmp/hakux-ui-owner.music); do
            music_pid=${music_entry%%:*}
            [ "$(nvf_ui_process_start "$music_pid")" != "${music_entry#*:}" ] || kill -CONT "$music_pid" 2>/dev/null || :
        done
        rm -f /tmp/hakux-ui-owner.music
    fi
    if [ -n "${NVF_UI_SUPERVISOR:-}" ] &&
       [ "$(cat /tmp/hakux-ui-owner 2>/dev/null)" = "$NVF_UI_TOKEN" ] &&
       [ "$(nvf_ui_process_start "$NVF_UI_SUPERVISOR")" = "$NVF_UI_SUPERVISOR_START" ]; then
        rm -f /tmp/hakux-ui-owner
        kill -CONT "$NVF_UI_SUPERVISOR" 2>/dev/null || :
    fi
}
if [ "${1:-}" = --guard ]; then
    watched=$2; watched_start=$3
    NVF_UI_TOKEN="$watched:$watched_start"
    NVF_UI_SUPERVISOR=$4; NVF_UI_SUPERVISOR_START=$5
    NVF_UI_OSD_EXE=$6; NVF_UI_OSD_LIB=$7; NVF_UI_OSD_CLOSED=$8
    while [ "$(nvf_ui_process_start "$watched")" = "$watched_start" ]; do
        case "$(awk '/^State:/ {print $2}' "/proc/$watched/status" 2>/dev/null)" in Z|X|'') break;; esac
        sleep 1
    done
    nvf_mainui_restore
    exit 0
fi
NVF_UI_SUPERVISOR=""
NVF_UI_SUPERVISOR_START=""
NVF_UI_OSD_EXE=""
NVF_UI_OSD_LIB=""
NVF_UI_OSD_CLOSED=0
NVF_UI_TOKEN="$$:$(nvf_ui_process_start "$$")"
nvf_mainui_apply() {
    [ -z "$NVF_UI_SUPERVISOR" ] || return 0
    # The supervisor also exists when MainUI already exited to launch a game.
    for supervisor in $(pidof runtrimui.sh 2>/dev/null); do
        case "$supervisor" in ''|0|1) continue;; esac
        tr '\000' ' ' < "/proc/$supervisor/cmdline" 2>/dev/null | grep -q '/usr/trimui/bin/runtrimui.sh' || continue
        # Allow an earlier launcher's EXIT cleanup to finish before taking over.
        # A game that just quit may still be restoring the UI (relaunching the OSD, resuming
        # services): wait up to 15 s, else MainUI comes back beside this game.
        retry=0
        while [ $retry -lt 150 ]; do
            state=$(awk '/^State:/ {print $2}' "/proc/$supervisor/status" 2>/dev/null)
            [ "$state" = T ] || break
            sleep .1
            retry=$((retry + 1))
        done
        [ "$state" != T ] || continue
        NVF_UI_SUPERVISOR=$supervisor
        NVF_UI_SUPERVISOR_START=$(nvf_ui_process_start "$supervisor")
        [ -n "$NVF_UI_SUPERVISOR_START" ] || { NVF_UI_SUPERVISOR=""; continue; }
        for osd_pid in $(pidof trimui_osdd 2>/dev/null); do
            osd_exe=$(readlink "/proc/$osd_pid/exe" 2>/dev/null)
            case "$osd_exe" in
                /usr/trimui/osd/trimui_osdd|/mnt/UDISK/trimui/osd/trimui_osdd|/mnt/SDCARD/trimui/osd/trimui_osdd)
                    NVF_UI_OSD_EXE=$osd_exe
                    NVF_UI_OSD_LIB="${osd_exe%/osd/trimui_osdd}/lib"
                    NVF_UI_OSD_CLOSED=1
                    break;;
            esac
        done
        printf '%s\n' "$NVF_UI_TOKEN" > "/tmp/hakux-ui-owner.$$"
        mv "/tmp/hakux-ui-owner.$$" /tmp/hakux-ui-owner
        /bin/sh "$APP/runtime-mainui.sh" --guard "$$" "$(nvf_ui_process_start "$$")" "$supervisor" "$NVF_UI_SUPERVISOR_START" "$NVF_UI_OSD_EXE" "$NVF_UI_OSD_LIB" "$NVF_UI_OSD_CLOSED" </dev/null >/dev/null 2>&1 &
        if ! kill -STOP "$supervisor" 2>/dev/null; then NVF_UI_SUPERVISOR=""; NVF_UI_OSD_CLOSED=0; continue; fi
        echo "NVForever RAM: automatic MainUI + OSD release; supervisor $supervisor resumes after exit" >> "$APP/log.txt"
        for candidate in $(pidof musicserver 2>/dev/null); do
            music_exe=$(readlink "/proc/$candidate/exe" 2>/dev/null)
            case "$music_exe" in
                /usr/trimui/bin/musicserver|/mnt/UDISK/trimui/bin/musicserver|/mnt/SDCARD/trimui/app/musicserver)
                    printf '%s\n' "$music_exe" > /tmp/nvforever-ui.music || continue
                    kill -KILL "$candidate" 2>/dev/null || :
                    continue;;
            esac
            [ "$(awk '/^State:/ {print $2}' "/proc/$candidate/status" 2>/dev/null)" != T ] || continue
            candidate_start=$(nvf_ui_process_start "$candidate")
            [ -n "$candidate_start" ] || continue
            # Record before stopping, so the guard can recover an interrupted launch.
            printf '%s:%s\n' "$candidate" "$candidate_start" >> /tmp/hakux-ui-owner.music || continue
            kill -STOP "$candidate" 2>/dev/null || :
        done
        [ ! -s /tmp/nvforever-ui.music ] || echo 'NVForever RAM: musicserver closed for the game; restarts after exit' >> "$APP/log.txt"
        [ ! -s /tmp/hakux-ui-owner.music ] || echo 'NVForever RAM: musicserver paused for the game; resumes after exit' >> "$APP/log.txt"
        for candidate in $(pidof MainUI 2>/dev/null); do
            parent=$(awk '/^PPid:/ {print $2}' "/proc/$candidate/status" 2>/dev/null)
            [ "$parent" != "$supervisor" ] || kill -KILL "$candidate" 2>/dev/null || :
        done
        if [ "$NVF_UI_OSD_CLOSED" = 1 ]; then
            for candidate in $(pidof trimui_osdd 2>/dev/null); do
                [ "$(readlink "/proc/$candidate/exe" 2>/dev/null)" != "$NVF_UI_OSD_EXE" ] || kill -KILL "$candidate" 2>/dev/null || :
            done
        fi
        return 0
    done
    echo 'NVForever RAM: unsupported or already-paused UI supervisor; retaining normal pause behavior' >> "$APP/log.txt"
    return 0
}
