# Release notes

## 0.3.2 — November 17, 2026

- Include full third-party license texts and copyright notices for the locked TrimUI dependency graph, bundled font, native runtimes and optimizer.
- Add pinned supplemental license sources and an inventory; packaging rejects missing notices or stale dependency inputs.
- Correct legal documentation for this source export and remove links to omitted upstream test tools.
- Packaging/documentation correction only; game and helper executables are unchanged from 0.3.1.

## 0.3.1 — November 16, 2026

- First launch automatically creates a 2 GiB swap file on internal UDISK, after checking mounted ext4 storage and leaving a 256 MiB free-space reserve.
- Reuses existing configured or sufficient active internal swap; existing files are not reformatted or overwritten.
- First-run setup and failure cards appear on the handheld. Awake/UI restoration guards cover the setup period; launch stops if swap cannot be prepared.
- Temporary allocations are cleaned after failure/interruption, and recorded stale setup files are recovered after a crash.
- Twelve simulated-utility regression tests cover creation, reuse, insufficient space, existing user files, unsupported filesystems, interruption and stale setup recovery. On-device checks passed for creating/activating a disposable 64 MiB file, active/inactive reuse, space rejection, and reuse of an existing 2 GiB internal swap. A fresh 2 GiB allocation was not repeated.
- Engine rendering and performance settings are unchanged from 0.3.0.

## 0.3.0 — November 16, 2026

Experimental release for TrimUI Smart Pro and PortMaster.

- Optional 13-tab developer/performance menu with object visibility controls, undo, gameplay testing tools and settings. Disabled in the distributed default INI.
- UI layout and stack invalidation guards; developer text/font values update only when changed.
- Cached clock-global identifiers and fewer repeated reference lookups/allocations in enable-parent traversal.
- Active package-expiry and concussion watch lists, reconstructed on save restoration.
- Idle object-animation table construction skipped using the focused upstream optimization.
- Optional CPU-side Vulkan call timings and Linux fault/I/O/thread telemetry; p99 frame-time logging.
- Generic portable release packaging and build paths, complete source archive and two documentation screenshots.

Validation of the engine changes: 944 simulation/game tests passed, five developer-menu model tests passed, and headless UI geometry matched normal Bevy layout through 21 mutations while skipping 20 unchanged frames. The ARM64 release was installed and launched on a TrimUI Smart Pro with save/settings preservation verified. The release packaging is audited separately before upload.

No matched before/after FPS improvement is claimed. Screenshot FPS readings are examples of particular scenes, not a benchmark. Gameplay hides affect rendering only; cheats, time and inventory changes may persist if saved. First startup can compile shaders for several minutes; long loads and scene-dependent stutter remain possible.
