# Patched dependencies

Crates copied from crates.io with a small change, used through `[patch.crates-io]` in
`engine/Cargo.toml`. Each keeps its own licence files. When the crate is upgraded, copy
the new version here and apply the change again (or drop the patch if upstream fixed it).

## TrimUI display (Bevy Render/Core Pipeline 0.20.1 and wgpu-hal 30.0.1)

`wgpu-hal` supports `VK_KHR_display` for the handheld's raw DRM handle without a file
descriptor. `NVF_SWAPCHAIN_UNORM=1` offers plain UNORM swapchain formats and views.
`bevy_render` keeps that same format for the window view and final blit pipeline;
changing only the Vulkan image view leaves the pipeline incompatible.
`bevy_core_pipeline` explicitly encodes linear RGB to sRGB in the final BGRA UNORM
blit when this switch is on. The plain scanout attachment does not perform the
conversion automatically; omitting it makes the world much too dark. Blits into
sRGB image targets retain the normal hardware conversion.

`NVF_VK_NO_TIMELINE=1` uses wgpu's Vulkan fence-pool fallback. The stock PowerVR
1.19@6345021 driver segfaults in queue submission with mixed binary/timeline semaphore
lists. With the fallback and plain formats, the color-screen example completed its
30-second device test and the user confirmed visible changing colors on 2026-11-03.
These switches are set by the TrimUI launcher; desktop launches retain their defaults.
`NVF_VK_TRACE=1` adds bounded command diagnostics when investigating native crashes.

`NVF_SPIRV_LEGACY=1` emits SPIR-V 1.0. `NVF_SPIRV_OPT=<path>` normalizes generated modules
with SPIRV-Tools `-O` before handing them to Vulkan: a captured NIF fragment reproduced a
null-pointer crash in the firmware's `libufwriter.so`, while the normalized fragment
passed the same standalone pipeline test. The TrimUI package builds and includes the
optimizer (its own Apache-2.0 license is retained). `NVF_VK_PIPELINE_CACHE=<file>` enables
a native disk cache for pipelines without a caller-supplied cache. It is destroyed before
its Vulkan device. `NVF_VK_SHADER_DUMP_DIR=<directory>` retains only the latest module per
stage and graphics state for diagnostics. All switches are opt-in outside the launcher.

`NVF_VK_SERIAL_PIPELINES=1` serializes native graphics/compute pipeline creation on the
stock driver. The packaged `spirv-normalize` helper removes layout annotations only
from workgroup-exclusive arrays/structures before invoking the optimizer; shared
host-visible buffer types are preserved. This handles Naga workgroup array strides
rejected by SPIRV-Tools validation without altering uniform/storage buffer layouts.

## gpu-allocator 0.28.0 (MIT OR Apache-2.0, Traverse-Research/gpu-allocator)

Used by wgpu for Vulkan memory. Change, in `src/vulkan/mod.rs` `MemoryType::free`: an
emptied general memory block is kept for reuse while fewer than
`KEEP_EMPTY_GENERAL_BLOCKS` (2) other empty general blocks of that memory type exist;
upstream destroys every emptied block except the last.

Still needed with Bevy 0.20.1 (#934): its wgpu-hal 30 uses gpu-allocator 0.28, and 0.28.0
is still the latest release, with the same free policy.

Why (#897): late in long sessions the block wgpu keeps fills with long-lived data, so each
frame's short-lived staging memory spills into a new block, which empties when the frame
is done and was destroyed at once. An Nsight Systems capture of the owner's game showed
about 285 blocks of 128 MiB made and freed a second, each zeroed and paged in by Windows,
and the render thread stalling 100-800 ms in the driver meanwhile.

Second change (#1429), in `src/vulkan/mod.rs` `MemoryBlock::new` and `src/lib.rs`: a
`vkAllocateMemory` that takes 1 ms or more logs `gpu memory: vkAllocateMemory of <n> MiB
(memory type <t>, host-visible|device-local, a general block|dedicated) took <ms> ms`, so a
single slow frame charged to a render system that only writes a buffer
(`upload_light_probes`, `write_binned_instance_buffers`) can be matched to a new memory
block. `std` is linked for the clock (wgpu-hal turns the crate's `std` feature off).

## Dropped

- bevy_pbr 0.19.1 (#903, PR #905): the bindless index-table comparator in
  `MaterialBindlessSlab::get_bindless_index_table`, reversed in 0.19.1, which kept freed
  `ExtendedMaterial`s' textures in the slabs. Fixed upstream (bevyengine/bevy#25848);
  Bevy 0.20 has the fix, in `bevy_render::material_bind_groups` (#934).
