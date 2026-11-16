NVForever 0.3 for the TrimUI Smart Pro (PortMaster)
===================================================

NVForever is an open-source reimplementation of the Fallout: New Vegas engine. This is
a native ARM64 build for the TrimUI Smart Pro. It needs the data files from your own
copy of Fallout: New Vegas (PC); no game files are included.

This is an early, experimental release with native 1280x720 output and sound.
Performance varies with scene, settings and temperature. No matched before/after
FPS improvement is claimed for this release.


What you need
-------------
- A TrimUI Smart Pro (other devices are not supported) with PortMaster installed.
- Your own Fallout: New Vegas PC install, to copy its Data folder from.
- Room on the SD card for the game's Data folder.
- About 2 GB free on the handheld's internal storage (/mnt/UDISK) for a swap file.


Install
-------
1. Copy nvforever.zip to the SD card's PortMaster autoinstall folder, then open
   PortMaster to install it automatically. On the TrimUI layout this is normally
   /mnt/SDCARD/Apps/PortMaster/PortMaster/autoinstall/.
   Alternatively use PortMaster's local/offline installer, or copy manually:
     NVForever.sh  ->  /mnt/SDCARD/Roms/PORTS/NVForever.sh
     nvforever/    ->  /mnt/SDCARD/Data/ports/nvforever/

2. Copy your game's Data folder into
     /mnt/SDCARD/Data/ports/nvforever/game/Data/
   so that this file exists:
     /mnt/SDCARD/Data/ports/nvforever/game/Data/FalloutNV.esm
   Copy the whole Data folder: FalloutNV.esm, all the "Fallout - *.bsa" archives,
   Update.bsa, and the Music and Sound folders (without them there is no music or
   radio). DLC files are optional.
   (Data elsewhere on the card: write its full path on the first line of
   /mnt/SDCARD/Data/ports/nvforever/data-path.)
   Eject the SD card properly before taking it out: a card pulled while files are still
   being written can lose the copied Data folder.

3. Make the swap file (once). The handheld has 1 GB of RAM and the game needs more,
   so it is required, not optional. Over SSH (or a terminal app on the device), run:
     sh /mnt/SDCARD/Data/ports/nvforever/setup-swap.sh 2048
   This makes a 2 GB swap file at /mnt/UDISK/nvforever.swap. It takes a few minutes,
   and the game turns it on by itself at every launch after that.

4. Start NVForever from the Ports list.
   The first launch is slow: the GPU's shaders are compiled and saved, and the screen
   can stay on the loading picture for several minutes. Later launches are faster.


Updating from an earlier build
------------------------------
Copy the new NVForever.sh and the nvforever/ folder over the old ones. Your saves and
settings (nvforever/home/), your game Data (nvforever/game/) and the swap file are kept.


Controls
--------
The built-in pad works as an Xbox controller.
- Start: pause menu
- A: confirm, B: cancel (to match the handheld's labels)
- Hold Menu + Start for a second: quit back to the TrimUI menu


Settings (optional)
-------------------
Developer menu: disabled by default in this package. To enable it, create
nvforever/home/dev-menu.ini containing:
  [DeveloperMenu]
  Enabled=1
Use Enabled=0 to disable it again. Restart the game after changing it.
The home/ override survives port updates; nvforever/dev-menu.ini is the shipped
default. If disabled, the admin panel's input, drawing and scene-filter systems
are not registered. Menu+Start still quits. When enabled: tap Menu during play,
LB/RB tabs, D-pad select/adjust, A apply, B/Menu close, X undo a hidden object,
Y reset tuning. Profiles are applied only when explicitly loaded.
Render-only object hides leave collision and quests intact. Developer cheats,
clock and travel changes can enter a later game save.

These files go in /mnt/SDCARD/Data/ports/nvforever/:
- extra-env: one KEY=VALUE per line, for example:
    NVF_NO_AUDIO=1          sound off (about 1.4 fps faster)
    NVF_RENDER_SIZE=640x360 draw the world smaller and scale it up (faster, blurrier)
    NVF_PRESENT_WAIT=2      wait for each frame's GPU work: removes the occasional
                            flickering blocks in some interiors, but costs several fps
    NVF_PAD_SWAP_AB=0       A and B the other way round
- render-size: one line (e.g. 960x540 or native) for the world's resolution; it is
  read again while the game runs. Delete the file to go back to the default.
- extra-args: one line of extra command-line options for the game.
The game's own settings menu (view distance, music, voices and the like) is saved in
nvforever/home/NVForever.ini.


What is reduced on this device
------------------------------
To fit the handheld's GPU and 1 GB of RAM:
- Simplified lighting: diffuse textures without normal maps, shadows or post-processing
  (the full renderer crashes the GPU driver).
- Textures limited to 128 pixels a side.
- No grass, no clouds, no particles or weather effects.
- Desert bushes, tumbleweeds, dust devils, mist and dust storms are hidden.
- Only the cells around the player are loaded in full, with the game's distant LOD
  beyond.


Known issues
------------
- Blocks of the previous frame can flicker briefly in some interiors (see
  NVF_PRESENT_WAIT=2 above).
- Loads are long and can stutter while the game reads from swap.
- The handheld gets hot after about 20 minutes of play and the CPU slows down. Playing
  while charging makes it hotter.
- The Pip-Boy screen is heavy for the GPU and runs slower than the world.
- NVForever is still in development on PC too: some quests, scripts and game systems
  are incomplete.


Troubleshooting
---------------
- Log: /mnt/SDCARD/Data/ports/nvforever/log.txt (rewritten at every launch).
- "FalloutNV.esm not found" in the log: the Data folder is in the wrong place (step 2).
- The game closes during loading: check that the swap file exists (step 3).
- No sound: start the game from the Ports list (the TrimUI menu has to release the
  sound chip first) and check the device volume.


Credits and source
------------------
NVForever engine: https://github.com/LootJunkieAzza/NVForever
Thanks to LootJunkieAzza and NVForever's contributors.
TrimUI port: NVForever TrimUI Maintainers.
Port source: https://github.com/ToastedFrog2/NVForever-TrimUI
The source revision for this build is in build-info.txt. Licences are in the
licenses/ folder.
NVForever is free software under the GNU GPL version 3 (licenses/LICENSE). The complete
source code of this build, with all the TrimUI changes, is NVForever-0.3-source.zip,
distributed alongside this package; if you did not get it, ask whoever gave you this
package for it.
Fallout: New Vegas is a trademark of Bethesda Softworks / ZeniMax Media. NVForever is
not affiliated with or endorsed by them, and contains no Bethesda code or game data.
