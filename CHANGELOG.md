# Release notes

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
