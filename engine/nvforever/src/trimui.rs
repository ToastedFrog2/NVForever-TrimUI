//! The TrimUI Smart Pro build (feature `trimui`, docs/trimui.md): a handheld with no window
//! system (Allwinner A133P, four Cortex-A53, PowerVR GE8300, 1 GB RAM, a 1280x720 screen).
//!
//! - **The screen**: winit is not used. The primary window gets a raw handle of its own, a
//!   DRM handle with no file descriptor, which the patched wgpu-hal turns into a
//!   `VK_KHR_display` surface on the screen (engine/vendor/wgpu-hal `khr_display.rs`).
//!   Frames are driven by Bevy's schedule runner instead of winit's event loop.
//! - **The controls**: the built-in pad is read from evdev (`/dev/input/event*`, the
//!   devices with gamepad buttons) and handed to Bevy as a gamepad, the way bevy_gilrs
//!   does; the game's own controller support (nvf-play `pad.rs`) does the rest.
//!   `NVF_PAD_SWAP_AB=1` / `NVF_PAD_SWAP_XY=1` swap the face buttons' pairs.
//! - **Quitting**: Menu + Start held for a second.

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::input::gamepad::{GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RawHandleWrapper, WindowWrapper};
use raw_window_handle::{DisplayHandle, DrmDisplayHandle, DrmWindowHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle};

pub struct TrimuiPlugin;

#[derive(Resource, Clone, Default)]
pub(crate) struct RendererState {
    ui_ready: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}

impl RendererState {
    pub(crate) fn world_ready(&self) -> bool {
        self.ui_ready.load(Ordering::Acquire) && self.pending.load(Ordering::Relaxed) == 0
    }
}

/// Meshes, vertices and triangles extracted for drawing in the last frame (any view), for
/// `NVF_GPU_REPORT` ([`draw_report`]); written by [`count_drawn`] in the render world.
/// Triangles of each mesh the render world has drawn, for the probe's `small` / `big` split.
static MESH_TRIANGLES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<AssetId<Mesh>, u32>>> = std::sync::LazyLock::new(Default::default);

static DRAWN: [std::sync::atomic::AtomicU64; 3] = [const { std::sync::atomic::AtomicU64::new(0) }; 3];

fn count_drawn(instances: Option<Res<bevy::pbr::RenderMeshInstances>>, meshes: Res<bevy::render::render_asset::RenderAssets<bevy::render::mesh::RenderMesh>>) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("NVF_GPU_REPORT").is_some()) { return; }
    let Some(instances) = instances else { return };
    let bevy::pbr::RenderMeshInstances::CpuBuilding(cpu) = &*instances else { return };
    let (mut n, mut verts, mut tris) = (0u64, 0u64, 0u64);
    // Kept while it runs (NVF_GPU_REPORT), so the sizes are known before a split hides any.
    let mut sizes = MESH_TRIANGLES.lock().ok();
    for inst in cpu.values() {
        let id = inst.mesh_asset_id();
        let Some(mesh) = meshes.get(id) else { continue };
        n += 1;
        verts += mesh.vertex_count as u64;
        let t = match mesh.buffer_info {
            bevy::render::mesh::RenderMeshBufferInfo::Indexed { count, .. } => count as u64 / 3,
            bevy::render::mesh::RenderMeshBufferInfo::NonIndexed => mesh.vertex_count as u64 / 3,
        };
        tris += t;
        if let Some(sizes) = sizes.as_mut() { sizes.insert(id, t as u32); }
    }
    for (slot, v) in DRAWN.iter().zip([n, verts, tris]) { slot.store(v, Ordering::Relaxed); }
}

fn renderer_state(cache: Res<bevy::render::render_resource::PipelineCache>, state: Res<RendererState>, mut frames: Local<u32>) {
    *frames += 1;
    let pending = cache.waiting_pipelines().count();
    state.pending.store(pending, Ordering::Relaxed);
    if *frames >= 8 && pending == 0 { state.ui_ready.store(true, Ordering::Release); }
}

/// Every `NVF_MALLOC_TRIM` seconds (0: never), glibc gives the free pages inside its heap
/// back to the kernel (`malloc_trim`): the start's tables are built and dropped in pieces,
/// and the freed pages otherwise stay the process's, pushed out to swap or crowding the
/// files' page cache on the 1 GB.
fn start_malloc_trim() {
    let secs: u64 = std::env::var("NVF_MALLOC_TRIM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    if secs == 0 {
        return;
    }
    let spawned = std::thread::Builder::new().name("malloc-trim".into()).spawn(move || {
        let mut last = 0i64;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let t0 = std::time::Instant::now();
            // SAFETY: glibc's malloc_trim takes its own arena locks.
            unsafe { libc::malloc_trim(0) };
            let ms = t0.elapsed().as_millis() as i64;
            if ms > 50 && ms > last * 2 {
                info!("trimui: malloc_trim took {ms} ms");
            }
            last = ms;
        }
    });
    if let Err(e) = spawned {
        warn!("trimui: no malloc-trim thread: {e}");
    }
}

impl Plugin for TrimuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(crate::trimui_admin::AdminPlugin);
        app.add_plugins(crate::trimui_idle::IdleUiPlugin);
        start_malloc_trim();
        if std::env::var_os("NVF_GPU_PASSES").is_some() {
            crate::graphics::ensure_render_diagnostics(app);
        }
        let state = RendererState::default();
        app.insert_resource(state.clone());
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            // Match the CPU mesh builder in every render phase; indirect batch
            // sets are incompatible with the CPU preparation path.
            render.insert_resource(bevy::render::batching::gpu_preprocessing::GpuPreprocessingSupport {
                max_supported_mode: bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode::None,
            });
            // Render recovery reinitializes this resource in RenderStartup,
            // overwriting the build-time value even on the first device.
            render.add_systems(bevy::render::RenderStartup,
                cpu_rendering.after(bevy::render::init_gpu_resource::<bevy::render::batching::gpu_preprocessing::GpuPreprocessingSupport>));
            render.insert_resource(state).add_systems(bevy::render::Render,
                (renderer_state.after(bevy::render::RenderSystems::Render), spin_render.in_set(bevy::render::RenderSystems::Prepare),
                    count_drawn.in_set(bevy::render::RenderSystems::Prepare),
                    hold_one_main_pass.in_set(bevy::render::RenderSystems::Prepare)));
        }
        app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO))
            // Bevy allocates a fallback shadow texture even with no shadowed lights.
            .insert_resource(bevy::light::PointLightShadowMap { size: 128 })
            .insert_resource(bevy::light::DirectionalLightShadowMap { size: 256 })
            // handoff.rs changes winit's update mode; nothing reads it here.
            .insert_resource(bevy::winit::WinitSettings::game())
            .insert_non_send(Pads::default())
            .add_systems(Startup, attach_screen)
            .add_systems(PostStartup, setup_scaled_world)
            .add_systems(PreUpdate, read_pads.before(InputSystems))
            .add_systems(PostUpdate, (drop_depth_prepasses, direct_draw_cameras, screen_cameras_without_msaa))
            .add_systems(PostUpdate, (basic_rendering, hide_glows).before(bevy::camera::CameraUpdateSystems))
            .add_systems(Update, (quit_combo, resize_world, performance, diagnostic_screenshot, camera_report, gpu_report))
            .add_systems(Last, (movement_trace, world_report, ui_changes, draw_report, big_mesh_report))
            .add_systems(Last, pass_report)
            .add_systems(PostUpdate, (menu_world_visibility, limit_world_depth, swap_world_image).before(bevy::camera::CameraUpdateSystems))
            .add_systems(PostUpdate, switch_mesh_lods.after(bevy::transform::TransformSystems::Propagate).before(bevy::camera::visibility::VisibilitySystems::CheckVisibility));
        // The present mode from the start (the tune file, read again every 2 s below, can change it).
        set_present_mode(std::env::var_os("NVF_SCHEDULE_MODE_FILE").and_then(|p| std::fs::read_to_string(p).ok()).as_deref());
        if std::env::var("NVF_SERIAL_SCHEDULES").as_deref() == Ok("1") || std::env::var_os("NVF_SCHEDULE_MODE_FILE").is_some() {
            app.add_systems(Last, schedule_mode).add_systems(Update, spin_main)
                .add_systems(PostUpdate, probe_hide.before(bevy::camera::visibility::VisibilitySystems::CheckVisibility));
        }
    }

    /// `NVF_SERIAL_SCHEDULES=1`: every schedule of the main app on one thread, not only
    /// the frame's four ([`schedule_mode`]): Last, the fixed-step schedules, state
    /// transitions and the like hold few systems each, and the multithreaded executor's
    /// hand-offs cost more than they run (FixedFirst 1.2 ms, StateTransition 0.8 ms,
    /// Last 2.3 ms a frame at Goodsprings with the profile build).
    fn finish(&self, app: &mut App) {
        // No camera draws motion vectors here (no TAA or motion blur; the depth prepass
        // is dropped), yet Bevy copies every mesh's transform for them each frame
        // (0.7 ms at Goodsprings): taken out of the lean renderer. (A run condition on
        // the system's own set is refused: "configuring system type sets is not allowed".)
        if basic_mode() {
            use bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly;
            let report = |what: &str, r: Result<usize, bevy::ecs::schedule::ScheduleError>| match r {
                Ok(n) => info!("trimui: {what}: {n} systems removed"),
                Err(e) => warn!("trimui: {what} kept: {e}"),
            };
            report("motion-vector transform copy", app.remove_systems_in_set(PreUpdate, bevy::pbr::update_mesh_previous_global_transforms, RemoveSystemsOnly));
            // Every material is unlit and no light casts a shadow (`basic_rendering`): which
            // meshes each light sees and the light clusters change nothing drawn (0.4 + 0.15
            // + 0.4 ms a frame at Goodsprings with the profile build).
            report("light visibility", app.remove_systems_in_set(PostUpdate, bevy::light::SimulationLightSystems::CheckLightVisibility, RemoveSystemsOnly));
            report("light clusters", app.remove_systems_in_set(PostUpdate, bevy::light::SimulationLightSystems::AssignLightsToClusters, RemoveSystemsOnly));
            // 2D materials the game never uses (it draws no Mesh2d; the scaled world's
            // Sprite is a SpriteMeshMaterial and keeps its systems): each still walks its
            // changed materials every frame with a parallel loop.
            {
                use bevy::sprite_render::{check_entities_needing_specialization as specialize, mark_2d_meshes_as_changed_if_their_materials_changed as mark, ColorMaterial, TilemapChunkMaterial};
                report("ColorMaterial upkeep", app.remove_systems_in_set(PostUpdate, mark::<ColorMaterial>, RemoveSystemsOnly));
                report("ColorMaterial specialization", app.remove_systems_in_set(PostUpdate, specialize::<ColorMaterial>, RemoveSystemsOnly));
                report("tilemap upkeep", app.remove_systems_in_set(PostUpdate, mark::<TilemapChunkMaterial>, RemoveSystemsOnly));
                report("tilemap specialization", app.remove_systems_in_set(PostUpdate, specialize::<TilemapChunkMaterial>, RemoveSystemsOnly));
            }
        }
        if std::env::var("NVF_SERIAL_SCHEDULES").as_deref() != Ok("1") { return; }
        let mut schedules = app.world_mut().resource_mut::<bevy::ecs::schedule::Schedules>();
        let mut n = 0;
        for (_, schedule) in schedules.iter_mut() {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
            n += 1;
        }
        info!("trimui: {n} main-app schedules single-threaded");
    }
}

/// Optional CPU scheduling comparison. The render app and background asset workers
/// retain their own threads. A test file permits A/B windows in one loaded scene.
/// Microseconds the main thread busy-waits each frame (`spin=` in the tune file).
static SPIN_US: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Microseconds the render thread busy-waits each frame (`spin_render=` in the tune file).
static SPIN_RENDER_US: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn spin_render() {
    let us = SPIN_RENDER_US.load(std::sync::atomic::Ordering::Relaxed);
    if us == 0 { return; }
    let until = std::time::Instant::now() + std::time::Duration::from_micros(us as u64);
    while std::time::Instant::now() < until { std::hint::spin_loop(); }
}

fn spin_main() {
    let us = SPIN_US.load(std::sync::atomic::Ordering::Relaxed);
    if us == 0 { return; }
    let until = std::time::Instant::now() + std::time::Duration::from_micros(us as u64);
    while std::time::Instant::now() < until { std::hint::spin_loop(); }
}

fn schedule_mode(world: &mut World, mut last: Local<Option<[bool; 4]>>, mut next: Local<f64>, mut last_upkeep: Local<Option<f64>>) {
    use bevy::ecs::schedule::{MultiThreadedExecutor, ScheduleLabel, Schedules, SingleThreadedExecutor};
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    if now < *next { return; }
    *next = now + 2.0;
    let file = std::env::var_os("NVF_SCHEDULE_MODE_FILE").and_then(|p| std::fs::read_to_string(p).ok());
    set_present_mode(file.as_deref());
    // `upkeep=<seconds>` in the file: the game's per-reference upkeep interval
    // (`GameConfig::upkeep_interval`, as `NVF_UPKEEP_INTERVAL`), for comparisons in one run.
    let upkeep = file.as_deref().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("upkeep=")?.trim().parse::<f64>().ok())).filter(|v| v.is_finite() && *v >= 0.0);
    if let Some(v) = upkeep.filter(|v| *last_upkeep != Some(*v)) {
        if let Some(rt) = world.get_resource_mut::<nvf_play::script::ScriptState>().and_then(|s| s.into_inner().rt.as_mut()) {
            rt.game.cfg.upkeep_interval = v.min(1.0);
            *last_upkeep = Some(v);
            info!("trimui: game upkeep interval {v} s");
        }
    }
    // `spin=<ms>`: the main thread busy-waits that long each frame ([`spin_main`]), to see
    // how the frame time follows the main thread's work.
    let spin = file.as_deref().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("spin=")?.trim().parse::<f64>().ok())).unwrap_or(0.0);
    let spin_us = (spin.clamp(0.0, 50.0) * 1000.0) as u32;
    if SPIN_US.swap(spin_us, std::sync::atomic::Ordering::Relaxed) != spin_us {
        info!("trimui: main thread spin {spin} ms");
    }
    let spin_r = file.as_deref().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("spin_render=")?.trim().parse::<f64>().ok())).unwrap_or(0.0);
    let spin_r_us = (spin_r.clamp(0.0, 50.0) * 1000.0) as u32;
    if SPIN_RENDER_US.swap(spin_r_us, std::sync::atomic::Ordering::Relaxed) != spin_r_us {
        info!("trimui: render thread spin {spin_r} ms");
    }
    // `hide=<list>`: what the GPU is spared, to see what each part of the frame costs it
    // ([`probe_hide`]).
    let hide = file.as_deref().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("hide="))).map_or(0, probe_hide_mask);
    // `far=<units>` (else `NVF_MODEL_DISTANCE`): placed models beyond that distance are left
    // out ([`probe_hide`]).
    let far = file.as_deref().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("far=")?.trim().parse::<u32>().ok()))
        .or_else(|| std::env::var("NVF_MODEL_DISTANCE").ok()?.trim().parse().ok()).unwrap_or(0);
    if PROBE_FAR.swap(far, Ordering::Relaxed) != far { info!("trimui: probe hides models beyond {far} units"); }
    // `lodpx=<pixels>`: the share of a pixel under which meshes' simpler copies are drawn
    // ([`switch_mesh_lods`]; 0 full meshes only); without it `NVF_MESH_LOD_PIXELS`.
    let lodpx = file.as_deref().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("lodpx=")?.trim().parse::<f32>().ok()))
        .filter(|v| v.is_finite() && *v >= 0.0).map_or(u32::MAX, f32::to_bits);
    if LOD_PIXELS_TUNE.swap(lodpx, Ordering::Relaxed) != lodpx {
        info!("trimui: mesh LOD pixels {}", if lodpx == u32::MAX { "default".to_string() } else { f32::from_bits(lodpx).to_string() });
    }
    if PROBE_HIDE.swap(hide, Ordering::Relaxed) != hide {
        info!("trimui: probe hides {:#x} ({})", hide, file.as_deref().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("hide="))).unwrap_or("nothing"));
    }
    let sched = file.as_deref().map(|t| t.lines().filter(|l| !l.contains('=')).collect::<Vec<_>>().join("\n"));
    let parallel = parallel_schedules(sched.as_deref().filter(|t| !t.trim().is_empty()));
    if *last == Some(parallel) { return; }
    let mut schedules = world.resource_mut::<Schedules>();
    for (label, on) in [First.intern(), PreUpdate.intern(), Update.intern(), PostUpdate.intern()].into_iter().zip(parallel) {
        if let Some(schedule) = schedules.get_mut(label) {
            if on { schedule.set_executor(MultiThreadedExecutor::new()); }
            else { schedule.set_executor(SingleThreadedExecutor::new()); }
        }
    }
    *last = Some(parallel);
    let names: Vec<&str> = ["First", "PreUpdate", "Update", "PostUpdate"].into_iter().zip(parallel).filter(|(_, on)| *on).map(|(n, _)| n).collect();
    info!("trimui: main frame schedules parallel: {}", if names.is_empty() { "none".to_string() } else { names.join(", ") });
}

/// `lodpx=` in the tune file as f32 bits ([`switch_mesh_lods`]); `u32::MAX`: not set.
static LOD_PIXELS_TUNE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// What the tune file's `hide=` spares the GPU, as bits of [`PROBE_HIDE`].
static PROBE_HIDE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const HIDE_LAND: u32 = 1;
const HIDE_SKY: u32 = 2;
const HIDE_SKINNED: u32 = 4;
const HIDE_MODELS: u32 = 8;
const HIDE_UI: u32 = 16;
const HIDE_WORLD: u32 = 32;
/// Placed models' meshes under [`SMALL_MESH`] triangles, or at and above it.
const HIDE_SMALL: u32 = 64;
const HIDE_BIG: u32 = 128;
const HIDE_FAR: u32 = 256;
/// The cloud layers only (`hide=clouds`); `hide=sky` takes them with the dome.
const HIDE_CLOUDS: u32 = 512;
/// `far=` in the tune file, else `NVF_MODEL_DISTANCE`: placed models (not terrain, sky or
/// people) beyond this many units are not drawn (0: no limit). On the handheld the main thread
/// sets the frame outdoors; at the owner's save 12,000 took 17.8-18.9 fps to 19.8-20.0.
static PROBE_FAR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const SMALL_MESH: u32 = 500;

fn probe_hide_mask(list: &str) -> u32 {
    list.split(',').map(|s| match s.trim() {
        "land" => HIDE_LAND,
        "sky" => HIDE_SKY,
        "skinned" => HIDE_SKINNED,
        "models" => HIDE_MODELS,
        "ui" => HIDE_UI,
        "world" => HIDE_WORLD,
        "small" => HIDE_SMALL,
        "big" => HIDE_BIG,
        "clouds" => HIDE_CLOUDS,
        _ => 0,
    }).fold(0, |a, b| a | b)
}

/// A mesh or interface root the probe took out of the frame, and the interface root's own
/// visibility to give back.
#[derive(Component)]
struct ProbeHidden(Option<Visibility>);

/// Layer no camera draws: where [`probe_hide`] puts the meshes it hides.
const PROBE_LAYER: usize = 29;

/// Test aid (`hide=` in the tune file, `NVF_SCHEDULE_MODE_FILE`): leaves out the terrain,
/// the sky, skinned meshes (people and creatures), every other mesh, the interface or the
/// whole world view, so one launch measures what each costs the GPU. Meshes go to a layer no
/// camera draws (those already on a layer of their own are left); interface roots are
/// hidden and given back their visibility.
#[allow(clippy::type_complexity)]
fn probe_hide(
    mut commands: Commands,
    meshes: Query<(Entity, &Mesh3d, &GlobalTransform, Has<crate::terrain::LandMesh>, Has<MeshMaterial3d<crate::sky::SkyMaterial>>, Has<bevy::mesh::skinning::SkinnedMesh>, Has<ProbeHidden>, Has<bevy::camera::visibility::RenderLayers>, Has<crate::sky::CloudLayer>)>,
    mut roots: Query<(Entity, &mut Visibility, Option<&ProbeHidden>), (With<Node>, Without<ChildOf>)>,
    world: Option<Res<ScaledWorld>>,
    mut cameras: Query<&mut Camera>,
    eyes: Query<&GlobalTransform, With<Camera3d>>,
    mut active: Local<bool>,
) {
    let mask = PROBE_HIDE.load(Ordering::Relaxed);
    let far = PROBE_FAR.load(Ordering::Relaxed);
    if mask == 0 && far == 0 && !*active { return; }
    *active = mask != 0 || far != 0;
    let eye = world.as_ref().and_then(|w| eyes.get(w.camera).ok()).map(|g| g.translation());
    let sizes = if mask & (HIDE_SMALL | HIDE_BIG) != 0 { MESH_TRIANGLES.lock().ok().map(|m| m.clone()) } else { None };
    for (entity, mesh, at, land, sky, skinned, hidden, layered, cloud) in &meshes {
        let mut bit = if land { HIDE_LAND } else if sky && cloud { HIDE_SKY | HIDE_CLOUDS } else if sky { HIDE_SKY } else if skinned { HIDE_SKINNED } else {
            // Split by size when asked (a mesh not drawn yet counts as small).
            match sizes.as_ref().map(|s| s.get(&mesh.0.id()).copied().unwrap_or(0)) {
                Some(t) if t >= SMALL_MESH => HIDE_MODELS | HIDE_BIG,
                Some(_) => HIDE_MODELS | HIDE_SMALL,
                None => HIDE_MODELS,
            }
        };
        // `far=<units>`: placed models farther than that from the eye.
        if far != 0 && bit & HIDE_MODELS != 0 && eye.is_some_and(|e| at.translation().distance(e) > far as f32) { bit |= HIDE_FAR; }
        let mask = if far != 0 { mask | HIDE_FAR } else { mask };
        if mask & bit != 0 && !hidden && !layered {
            commands.entity(entity).insert((ProbeHidden(None), bevy::camera::visibility::RenderLayers::layer(PROBE_LAYER)));
        } else if mask & bit == 0 && hidden {
            commands.entity(entity).remove::<(ProbeHidden, bevy::camera::visibility::RenderLayers)>();
        }
    }
    for (entity, mut vis, hidden) in &mut roots {
        match (mask & HIDE_UI != 0, hidden) {
            (true, None) => { commands.entity(entity).insert(ProbeHidden(Some(*vis))); *vis = Visibility::Hidden; }
            (true, Some(_)) => if *vis != Visibility::Hidden { *vis = Visibility::Hidden; },
            (false, Some(ProbeHidden(old))) => { *vis = old.unwrap_or_default(); commands.entity(entity).remove::<ProbeHidden>(); }
            (false, None) => {}
        }
    }
    if let Some(world) = world {
        if let Ok(mut camera) = cameras.get_mut(world.camera) {
            let on = mask & HIDE_WORLD == 0;
            if camera.is_active != on { camera.is_active = on; }
        }
    }
}

/// Which of First, PreUpdate, Update and PostUpdate run on the multithreaded executor:
/// the mode file's text when there is one (`single`, `parallel`, or `parallel:` and a
/// comma list of those names), else `NVF_PARALLEL_SCHEDULES` (the same list) under
/// `NVF_SERIAL_SCHEDULES=1`, else all of them. The game's own systems (Update, hundreds of
/// small ones) cost more to hand between threads than they run; Bevy's own (PostUpdate:
/// transforms, visibility, UI layout) are fewer and larger.
fn parallel_schedules(file: Option<&str>) -> [bool; 4] {
    let named = |list: &str| {
        let has = |n: &str| list.split(',').any(|s| s.trim().eq_ignore_ascii_case(n));
        [has("First"), has("PreUpdate"), has("Update"), has("PostUpdate")]
    };
    match file.map(str::trim) {
        Some("single") => [false; 4],
        Some("parallel") => [true; 4],
        Some(t) if t.starts_with("parallel:") => named(&t["parallel:".len()..]),
        _ if std::env::var("NVF_SERIAL_SCHEDULES").as_deref() == Ok("1") => named(&std::env::var("NVF_PARALLEL_SCHEDULES").unwrap_or_default()),
        _ => [true; 4],
    }
}

fn cpu_rendering(mut support: ResMut<bevy::render::batching::gpu_preprocessing::GpuPreprocessingSupport>) {
    support.max_supported_mode = bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode::None;
    info!("trimui: CPU mesh preparation and direct draw phases selected after render startup");
}

/// Diagnostic only: distinguish collision corrections from animation-driven eye
/// movement during the same sustained controller input. Buffered to avoid a disk
/// write on every frame; normal launches never create or write this file.
fn movement_trace(
    real: Res<Time<Real>>,
    time: Res<Time>,
    player: Option<Res<nvf_play::player::Player>>,
    phys: Option<Res<nvf_play::phys::PhysState>>,
    intent: Option<Res<nvf_play::player::MoveIntent>>,
    seat: Option<Res<nvf_play::furniture::PlayerSeat>>,
    mut output: Local<Option<std::io::BufWriter<File>>>,
) {
    use std::io::Write;
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("NVF_MOVE_TRACE").as_deref() == Ok("1")) { return; }
    let (Some(player), Some(phys), Some(intent)) = (player, phys, intent) else { return };
    let Some(character) = &phys.player else { return };
    if output.is_none() {
        let home = std::env::var_os("NVFOREVER_HOME").unwrap_or_else(|| ".".into());
        let path = std::path::PathBuf::from(home).join("diagnostics/movement.csv");
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() { return; }
        let Ok(file) = File::create(path) else { return };
        let mut writer = std::io::BufWriter::new(file);
        let _ = writeln!(writer, "seconds,dt,feet_x,feet_y,feet_z,physics_x,physics_y,physics_z,eye_x,eye_y,eye_z,heading,pitch,forward,right,seated");
        *output = Some(writer);
    }
    let f = player.feet;
    let p = character.feet;
    let e = player.eye();
    if let Some(writer) = output.as_mut() {
        let _ = writeln!(writer, "{:.5},{:.5},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.5},{:.5},{:.4},{:.4},{}",
            real.elapsed_secs_f64(), time.delta_secs(), f[0], f[1], f[2], p[0], p[1], p[2], e[0], e[1], e[2],
            player.heading, player.pitch, intent.forward, intent.right, u8::from(seat.is_some_and(|s| s.hold.is_some())));
    }
}

/// The asset an event says was added or changed.
fn touched_asset<A: Asset>(e: &AssetEvent<A>) -> Option<AssetId<A>> {
    match e {
        AssetEvent::Added { id } | AssetEvent::Modified { id } => Some(*id),
        _ => None,
    }
}

fn needs_basic(material: &StandardMaterial) -> bool {
    !material.unlit || material.normal_map_texture.is_some()
        || material.metallic_roughness_texture.is_some() || material.occlusion_texture.is_some()
        || material.emissive_texture.is_some() || material.emissive != LinearRgba::BLACK
        || material.specular_texture.is_some() || material.specular_tint_texture.is_some()
        || material.specular_transmission != 0.0 || material.diffuse_transmission != 0.0
}

fn make_basic(material: &mut StandardMaterial) {
    material.unlit = true;
    material.normal_map_texture = None;
    material.metallic_roughness_texture = None;
    material.occlusion_texture = None;
    material.emissive_texture = None;
    material.emissive = LinearRgba::BLACK;
    material.specular_texture = None;
    material.specular_tint_texture = None;
    material.specular_transmission = 0.0;
    material.diffuse_transmission = 0.0;
}

pub(crate) fn basic_mode() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_BASIC_RENDERING").as_deref() == Ok("1"))
}

/// Isolate the mobile driver's advanced shader paths while retaining scene
/// geometry, diffuse textures, alpha tests, skeletal animation and game logic.
fn basic_rendering(
    mut commands: Commands,
    scaled: Option<Res<ScaledWorld>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut nif: ResMut<Assets<nvf_assets::nif_material::NifMaterial>>,
    mut point_maps: ResMut<bevy::light::PointLightShadowMap>,
    mut directional_maps: ResMut<bevy::light::DirectionalLightShadowMap>,
    cameras: Query<(Entity, Option<&bevy::core_pipeline::tonemapping::Tonemapping>, Has<bevy::pbr::DistanceFog>, Option<&bevy::light::cluster::ClusterConfig>, &bevy::camera::RenderTarget), With<Camera3d>>,
    mut standard_events: MessageReader<AssetEvent<StandardMaterial>>,
    mut nif_events: MessageReader<AssetEvent<nvf_assets::nif_material::NifMaterial>>,
    mut reported: Local<bool>,
) {
    if !basic_mode() { return; }
    // No basic material samples these maps. Settings otherwise restore a 2048²
    // directional fallback texture even while all shadows are disabled.
    if point_maps.size != 64 { point_maps.size = 64; }
    if directional_maps.size != 64 { directional_maps.size = 64; }
    let first = !*reported;
    if first { info!("trimui: basic diffuse rendering; unlit materials, no advanced maps, fog or image-space effects"); *reported = true; }
    // Every material once, then only those added or changed since (thousands of NIF
    // materials are loaded; walking them all each frame cost 0.8 % of the main thread).
    // A material this system changes comes back once as modified and then passes.
    let ids: Vec<_> = if first {
        standard_events.clear();
        standard.iter().map(|(id, _)| id).collect()
    } else {
        standard_events.read().filter_map(touched_asset).collect()
    };
    let ids: Vec<_> = ids.into_iter().filter(|id| standard.get(*id).is_some_and(needs_basic)).collect();
    for id in ids { if let Some(mut material) = standard.get_mut(id) { make_basic(&mut material); } }
    let nif_needs = |m: &nvf_assets::nif_material::NifMaterial| needs_basic(&m.base) || m.extension.refract || m.extension.shade_light || m.extension.facegen_offset.is_some();
    let ids: Vec<_> = if first {
        nif_events.clear();
        nif.iter().map(|(id, _)| id).collect()
    } else {
        nif_events.read().filter_map(touched_asset).collect()
    };
    let ids: Vec<_> = ids.into_iter().filter(|id| nif.get(*id).is_some_and(nif_needs)).collect();
    for id in ids {
        if let Some(mut material) = nif.get_mut(id) {
            make_basic(&mut material.base);
            material.extension.refract = false;
            material.extension.refraction = 0.0;
            material.extension.shade_light = false;
            material.extension.facegen_offset = None;
            material.extension.soft = Vec4::ZERO;
            material.extension.hard_edges = true;
        }
    }
    // The world's camera: the scaled world's, or (drawn at the screen's size, `NVF_RENDER_SIZE=native`)
    // every 3D camera drawing to the window.
    for (camera, tone, fog, clusters, target) in &cameras {
        let world = match &scaled {
            Some(scaled) => camera == scaled.camera,
            None => matches!(target, bevy::camera::RenderTarget::Window(_)),
        };
        if !world { continue; }
        if !matches!(clusters, Some(bevy::light::cluster::ClusterConfig::None)) {
            commands.entity(camera).insert(bevy::light::cluster::ClusterConfig::None);
        }
        if fog { commands.entity(camera).remove::<bevy::pbr::DistanceFog>(); }
        if tone != Some(&bevy::core_pipeline::tonemapping::Tonemapping::None) {
            commands.entity(camera).insert(bevy::core_pipeline::tonemapping::Tonemapping::None);
        }
    }
}

/// The one-pass main pass (`mainpass=` / NVF_ONE_MAIN_PASS) is held for a frame with
/// transmissive shapes in any view: bevy_pbr draws those between the opaque and the blended
/// passes, after a copy of the frame, so drawing the blended ones in the opaque pass would
/// put the water over the smoke in front of it.
fn hold_one_main_pass(phases: Res<bevy::render::render_phase::ViewSortedRenderPhases<bevy::pbr::Transmissive3d>>) {
    let hold = phases.values().any(|p| !p.items.is_empty());
    bevy::core_pipeline::core_3d::NVF_ONE_MAIN_PASS_HOLD.store(hold, Ordering::Relaxed);
}

/// Every 10 s (with `NVF_GPU_PASSES` set): each render pass's GPU and CPU time, averaged by
/// Bevy's render diagnostics (timestamp queries, when the driver offers them), largest first.
/// The kernel has no GPU job trace, so this is the one direct per-pass measure on the handheld.
fn pass_report(store: Option<Res<bevy::diagnostic::DiagnosticsStore>>, time: Res<Time<Real>>, mut next: Local<f64>) {
    if std::env::var_os("NVF_GPU_PASSES").is_none() { return; }
    let now = time.elapsed_secs_f64();
    if now < *next { return; }
    *next = now + 10.0;
    let Some(store) = store else { return };
    let mut gpu = Vec::new();
    let mut cpu = Vec::new();
    for d in store.iter() {
        let path = d.path().as_str();
        let Some(rest) = path.strip_prefix("render/") else { continue };
        let Some(v) = d.average() else { continue };
        if let Some(name) = rest.strip_suffix("/elapsed_gpu") { gpu.push((name.to_string(), v)); }
        else if let Some(name) = rest.strip_suffix("/elapsed_cpu") { cpu.push((name.to_string(), v)); }
    }
    for list in [&mut gpu, &mut cpu] { list.sort_by(|a, b| b.1.total_cmp(&a.1)); }
    let fmt = |l: &[(String, f64)]| l.iter().map(|(n, v)| format!("{n} {v:.2}")).collect::<Vec<_>>().join(", ");
    info!("trimui passes gpu ms (sum {:.1}): {}", gpu.iter().map(|x| x.1).sum::<f64>(), fmt(&gpu));
    info!("trimui passes cpu ms (sum {:.1}): {}", cpu.iter().map(|x| x.1).sum::<f64>(), fmt(&cpu));
}

/// Every 10 s (with `NVF_GPU_REPORT` set): the ECS's size, to find per-frame overhead.
/// Archetypes only grow; a count still rising while the view stands still means new
/// component sets every frame, which every query of every system then has to take in.
fn world_report(world: &World, mut next: Local<f64>, mut last: Local<usize>, mut listed: Local<bool>) {
    if std::env::var_os("NVF_GPU_REPORT").is_none() { return; }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    if now < *next { return; }
    *next = now + 10.0;
    let archetypes = world.archetypes().len();
    let systems: Vec<String> = world.get_resource::<bevy::ecs::schedule::Schedules>().map(|s| {
        let mut v: Vec<(String, usize)> = s.iter().map(|(label, sch)| (format!("{label:?}"), sch.systems_len())).filter(|(_, n)| *n > 0).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.into_iter().take(12).map(|(l, n)| format!("{l} {n}")).collect()
    }).unwrap_or_default();
    // The game's per-reference extra state, which several systems scan whole every frame.
    let extra = world.get_resource::<nvf_play::script::ScriptState>().and_then(|s| s.rt.as_ref()).map(|rt| {
        let g = &rt.game;
        let actors = g.extra.values().filter(|e| e.current_package.is_some() || e.knocked_out.is_some() || e.teammate).count();
        format!("{} entries ({actors} actor-like), {} B each", g.extra.len(), std::mem::size_of::<fnv_game::game::RefExtra>())
    }).unwrap_or_default();
    info!("trimui world: {} entities, {} archetypes (+{} in 10 s), {} components; game extra {extra}; systems: {}",
        world.entities().len(), archetypes, archetypes.saturating_sub(*last), world.components().len(), systems.join(", "));
    // Once: the frame schedules' systems by module (where per-system upkeep goes); a
    // schedule whose executor was just swapped (`schedule_mode`) lists after its next run.
    if !*listed {
        use bevy::ecs::schedule::ScheduleLabel;
        if let Some(s) = world.get_resource::<bevy::ecs::schedule::Schedules>() {
            for label in [PreUpdate.intern(), Update.intern(), PostUpdate.intern()] {
                let Some(Ok(list)) = s.get(label).map(|sch| sch.systems()) else { continue };
                *listed = true;
                let mut by: std::collections::BTreeMap<String, usize> = Default::default();
                for (_, sys) in list {
                    let name = format!("{}", sys.name());
                    let name = name.split('<').next().unwrap_or("").to_string();
                    let module: Vec<&str> = name.split("::").collect();
                    let key = module[..module.len().saturating_sub(1).clamp(1, 3)].join("::");
                    *by.entry(key).or_default() += 1;
                }
                let mut v: Vec<(String, usize)> = by.into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                info!("trimui systems in {label:?}: {}", v.iter().map(|(m, n)| format!("{m} {n}")).collect::<Vec<_>>().join(", "));
            }
        }
    }
    *last = archetypes;
}

/// Every 10 s (with `NVF_GPU_REPORT` set): what the world camera draws, to find where the
/// GPU's per-draw cost goes: visible NIF meshes, distinct mesh and material pairs (Bevy
/// draws identical pairs as one instanced batch), triangles, blend modes, how many meshes
/// each placed reference adds, and the meshes drawn most often.
#[allow(clippy::type_complexity)]
fn draw_report(
    shapes: Query<(&Mesh3d, &MeshMaterial3d<nvf_assets::nif_material::NifMaterial>, &ViewVisibility, Option<&nvf_play::RefVisual>)>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<nvf_assets::nif_material::NifMaterial>>,
    time: Res<Time<Real>>,
    mut next: Local<f64>,
) {
    use std::collections::HashMap;
    if std::env::var_os("NVF_GPU_REPORT").is_none() { return; }
    let now = time.elapsed_secs_f64();
    if now < *next { return; }
    *next = now + 10.0;
    // A mesh already handed to the render world has no data left here: 0 triangles.
    let tris = |m: &Mesh3d| {
        meshes.get(&m.0).map_or(0, |m| match m.try_indices_option() {
            Ok(Some(i)) => i.len() / 3,
            Ok(None) => m.count_vertices() / 3,
            Err(_) => 0,
        })
    };
    let (mut shown, mut triangles, mut loose) = (0usize, 0usize, 0usize);
    let mut pairs: HashMap<(AssetId<Mesh>, AssetId<nvf_assets::nif_material::NifMaterial>), usize> = HashMap::new();
    let mut per_mesh: HashMap<AssetId<Mesh>, (usize, usize)> = HashMap::new();
    let mut per_ref: HashMap<u32, usize> = HashMap::new();
    let mut modes: HashMap<&'static str, usize> = HashMap::new();
    for (mesh, material, seen, owner) in &shapes {
        if !seen.get() { continue; }
        shown += 1;
        let t = tris(mesh);
        triangles += t;
        *pairs.entry((mesh.0.id(), material.0.id())).or_default() += 1;
        let e = per_mesh.entry(mesh.0.id()).or_insert((0, t));
        e.0 += 1;
        match owner { Some(r) => *per_ref.entry(r.0).or_default() += 1, None => loose += 1 }
        let mode = match materials.get(&material.0).map(|m| m.base.alpha_mode) {
            Some(AlphaMode::Opaque) => "opaque",
            Some(AlphaMode::Mask(_)) => "mask",
            Some(AlphaMode::Blend) => "blend",
            Some(AlphaMode::Add) => "add",
            Some(_) => "other",
            None => "unmade",
        };
        *modes.entry(mode).or_default() += 1;
    }
    let shared = pairs.values().filter(|n| **n > 1).count();
    let mut buckets = [0usize; 5]; // 1, 2-3, 4-7, 8-15, 16+ meshes per reference
    for n in per_ref.values() { buckets[match *n { 1 => 0, 2..=3 => 1, 4..=7 => 2, 8..=15 => 3, _ => 4 }] += 1; }
    let mut top: Vec<(usize, usize)> = per_mesh.values().copied().collect();
    top.sort_by(|a, b| b.0.cmp(&a.0));
    let top: Vec<String> = top.iter().take(8).map(|(n, t)| format!("{n}x{t}")).collect();
    let mut modes: Vec<String> = modes.into_iter().map(|(k, v)| format!("{k} {v}")).collect();
    modes.sort();
    info!("trimui drawn (render world, last frame): {} meshes, {} vertices, {} triangles",
        DRAWN[0].load(Ordering::Relaxed), DRAWN[1].load(Ordering::Relaxed), DRAWN[2].load(Ordering::Relaxed));
    info!("trimui draws: {shown} meshes shown ({} pairs, {shared} of them shared), {triangles} triangles; {}; {} references (meshes each: 1 {}, 2-3 {}, 4-7 {}, 8-15 {}, 16+ {}), {loose} not of one; most drawn mesh x triangles: {}",
        pairs.len(), modes.join(", "), per_ref.len(), buckets[0], buckets[1], buckets[2], buckets[3], buckets[4], top.join(" "));
}

/// Every 10 s (with `NVF_GPU_REPORT` set): where the drawn triangles are. The GPU's time
/// follows the triangle count (2026-11-10: hiding the ~150 meshes of 500 triangles and up,
/// 323,000 of 389,000, took the GPU from 98 % to 65 % busy), so: triangles by distance from
/// the world camera, the share drawn without back-face culling, and the largest meshes.
#[allow(clippy::type_complexity)]
fn big_mesh_report(
    shapes: Query<(&Mesh3d, &ViewVisibility, &GlobalTransform, Option<&MeshMaterial3d<nvf_assets::nif_material::NifMaterial>>, Option<&Name>, Option<&ChildOf>, Option<&nvf_play::RefVisual>)>,
    names: Query<(Option<&Name>, Option<&ChildOf>)>,
    materials: Res<Assets<nvf_assets::nif_material::NifMaterial>>,
    world: Option<Res<ScaledWorld>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    time: Res<Time<Real>>,
    mut next: Local<f64>,
) {
    if std::env::var_os("NVF_GPU_REPORT").is_none() { return; }
    let now = time.elapsed_secs_f64();
    if now < *next { return; }
    *next = now + 10.0;
    let eye = match world {
        Some(w) => cameras.get(w.camera).ok(),
        None => cameras.iter().next(),
    };
    let Some(eye) = eye.map(|g| g.translation()) else { return };
    let Ok(sizes) = MESH_TRIANGLES.lock().map(|m| m.clone()) else { return };
    const BANDS: [f32; 5] = [1000.0, 2000.0, 4000.0, 8000.0, f32::INFINITY];
    let (mut bands, mut two_sided, mut total) = ([0u64; 5], 0u64, 0u64);
    let mut big: Vec<(u32, f32, String, bool)> = Vec::new();
    for (mesh, seen, at, material, name, parent, owner) in &shapes {
        if !seen.get() { continue; }
        let t = sizes.get(&mesh.0.id()).copied().unwrap_or(0);
        let d = at.translation().distance(eye);
        total += t as u64;
        bands[BANDS.iter().position(|b| d < *b).unwrap_or(4)] += t as u64;
        let culled = material.and_then(|m| materials.get(&m.0)).is_none_or(|m| m.base.cull_mode.is_some());
        if !culled { two_sided += t as u64; }
        if t >= 2000 {
            // The shape's name, then its model's (the nearest ancestor with a name).
            let mut label = name.map(|n| n.as_str().to_string()).unwrap_or_default();
            let mut up = parent.map(|c| c.parent());
            for _ in 0..6 {
                let Some(e) = up else { break };
                let Ok((n, p)) = names.get(e) else { break };
                if let Some(n) = n { label = format!("{}/{}", n.as_str(), label); break; }
                up = p.map(|c| c.parent());
            }
            if let Some(r) = owner { label = format!("{label} ref {:08X}", r.0); }
            big.push((t, d, label, !culled));
        }
    }
    big.sort_by(|a, b| b.0.cmp(&a.0));
    info!("trimui triangles by distance (<1k, <2k, <4k, <8k, beyond): {:?} of {total}; without back-face culling {two_sided}; {} meshes of 2000+: {}",
        bands, big.len(), big.iter().take(14).map(|(t, d, l, two)| format!("{t}@{d:.0}{} {l}", if *two { " 2s" } else { "" })).collect::<Vec<_>>().join(" | "));
}

/// Every 10 s (with `NVF_GPU_REPORT` set): the UI nodes that changed in the most frames,
/// by name or text. A node, text or measured size that changes makes Bevy lay out its
/// whole UI tree again (`ui_layout_system`, 2 ms a frame at Goodsprings).
#[allow(clippy::type_complexity)]
fn ui_changes(
    nodes: Query<(Entity, Option<&Name>, Ref<Node>, Option<Ref<Text>>, Option<Ref<bevy::ui::ContentSize>>, Option<&ChildOf>)>,
    names: Query<&Name>,
    texts: Query<&Text>,
    time: Res<Time<Real>>,
    mut counts: Local<std::collections::HashMap<Entity, u32>>,
    mut frames: Local<u32>,
    mut next: Local<f64>,
) {
    if std::env::var_os("NVF_GPU_REPORT").is_none() { return; }
    *frames += 1;
    for (e, _, node, text, size, _) in &nodes {
        if node.is_changed() || text.is_some_and(|t| t.is_changed()) || size.is_some_and(|c| c.is_changed()) {
            *counts.entry(e).or_default() += 1;
        }
    }
    let now = time.elapsed_secs_f64();
    if now < *next { return; }
    *next = now + 10.0;
    let mut v: Vec<(Entity, u32)> = counts.drain().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    let label = |e: Entity| -> String {
        let Ok((_, name, _, _, _, parent)) = nodes.get(e) else { return format!("{e}") };
        if let Some(n) = name { return n.as_str().to_string(); }
        if let Ok(t) = texts.get(e) { return format!("text {:?}", t.0.chars().take(24).collect::<String>()); }
        match parent.and_then(|p| names.get(p.parent()).ok()) { Some(n) => format!("child of {}", n.as_str()), None => format!("{e}") }
    };
    let top: Vec<String> = v.iter().take(10).map(|(e, n)| format!("{} {}/{}", label(*e), n, *frames)).collect();
    info!("trimui ui: {} of {} nodes changed in 10 s; most: {}", v.len(), nodes.iter().count(), top.join(", "));
    *frames = 0;
}

/// Checked by [`hide_glows`].
#[derive(Component)]
struct GlowChecked;

/// `NVF_NO_GLOWS=1`: the additive shapes of placed models (lamps' glow cards, light beams,
/// window rays; the blend adds to what is behind, nif_spawn `alpha_mode`) are not drawn.
/// Each is a stack of cards that shades every pixel it covers once per card, depth or not,
/// and close to one they cover much of the screen [observed: the fps fell nearing a lit
/// house's lamp in Goodsprings, owner 2026-11-06]. Effects of shots (muzzle flashes) are
/// not placed references and stay.
#[allow(clippy::type_complexity)]
fn hide_glows(
    mut commands: Commands,
    nif: Res<Assets<nvf_assets::nif_material::NifMaterial>>,
    unchecked: Query<(Entity, &MeshMaterial3d<nvf_assets::nif_material::NifMaterial>), (With<nvf_play::RefVisual>, Without<GlowChecked>)>,
    mut hidden: Local<(u32, u32, f64)>,
    time: Res<Time<Real>>,
) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("NVF_NO_GLOWS").as_deref() == Ok("1")) { return; }
    for (e, material) in &unchecked {
        // Not made yet: looked at again next frame.
        let Some(m) = nif.get(&material.0) else { continue };
        let mut entity = commands.entity(e);
        entity.insert(GlowChecked);
        if matches!(m.base.alpha_mode, AlphaMode::Add) {
            entity.insert(Visibility::Hidden);
            hidden.0 += 1;
        }
    }
    if hidden.0 != hidden.1 && time.elapsed_secs_f64() >= hidden.2 {
        info!("trimui: {} glow shapes of placed models not drawn (NVF_NO_GLOWS)", hidden.0);
        *hidden = (hidden.0, hidden.0, time.elapsed_secs_f64() + 30.0);
    }
}

/// Every 10 s: each camera's order, whether it draws, what it draws to and its kind, and
/// whether the main menu is open (the scaled world's camera follows it).
#[allow(clippy::type_complexity)]
/// `NVF_GPU_REPORT=<seconds>`: every so often, the GPU memory the allocator holds (the
/// device's memory is the machine's RAM) by what it holds it for (the allocations' labels,
/// digits folded), the largest first.
fn gpu_report(time: Res<Time<Real>>, mut next: Local<f64>, device: Option<Res<bevy::render::renderer::RenderDevice>>) {
    let every: f64 = std::env::var("NVF_GPU_REPORT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let now = time.elapsed_secs_f64();
    if every <= 0.0 || now < *next {
        return;
    }
    *next = now + every;
    let Some(report) = device.and_then(|d| d.wgpu_device().generate_allocator_report()) else { return };
    let mut by: std::collections::HashMap<String, (u64, usize)> = std::collections::HashMap::new();
    for a in &report.allocations {
        let name: String = a.name.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).take(48).collect();
        let e = by.entry(name).or_default();
        e.0 += a.size;
        e.1 += 1;
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let top: Vec<String> = v.iter().take(14).map(|(n, (b, c))| format!("{n:?} {:.1} MB ({c})", *b as f64 / 1048576.0)).collect();
    info!(
        "trimui gpu: {:.1} MB in {} allocations, {:.1} MB in {} blocks; {}",
        report.total_allocated_bytes as f64 / 1048576.0,
        report.allocations.len(),
        report.total_reserved_bytes as f64 / 1048576.0,
        report.blocks.len(),
        top.join(", ")
    );
}

fn camera_report(
    time: Res<Time<Real>>,
    mut next: Local<f64>,
    cameras: Query<(Entity, &Camera, &bevy::camera::RenderTarget, Has<Camera3d>, Has<Camera2d>, Has<IsDefaultUiCamera>, Option<&Name>)>,
    menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    roots: Query<(&Visibility, Option<&bevy::ui::UiTargetCamera>), (With<Node>, Without<ChildOf>)>,
    lod: Option<Res<crate::lod::LodState>>,
) {
    if time.elapsed_secs_f64() < *next { return; }
    *next = time.elapsed_secs_f64() + 10.0;
    let list: Vec<String> = cameras.iter().map(|(e, c, t, d3, d2, ui, name)| {
        let target = match t {
            bevy::camera::RenderTarget::Window(_) => "window".to_string(),
            bevy::camera::RenderTarget::Image(i) => format!("image {:?}", i.handle.id()),
            other => format!("{other:?}").chars().take(30).collect(),
        };
        format!("{e} {} order {} {} {}{}{}", name.map_or("", |n| n.as_str()), c.order, if c.is_active { "on" } else { "OFF" }, target,
            if d3 { " 3d" } else if d2 { " 2d" } else { "" }, if ui { " ui-default" } else { "" })
    }).collect();
    let shown = roots.iter().filter(|(v, _)| !matches!(v, Visibility::Hidden)).count();
    let targeted = roots.iter().filter(|(_, t)| t.is_some()).count();
    info!("trimui cameras: menu open {:?}; ui roots {} ({} shown, {} with a target camera); {}",
        menu.map(|m| m.open), roots.iter().count(), shown, targeted, list.join(" | "));
    if let Some(lod) = lod {
        info!("trimui lod: {}", lod.diagnose());
    }
}

/// A one-shot request file allows unattended device checks without a keyboard.
fn diagnostic_screenshot(mut commands: Commands, time: Res<Time<Real>>, mut next: Local<f64>,
    world: Option<Res<ScaledWorld>>,
    pending: Query<Option<&bevy::render::view::screenshot::Capturing>, With<bevy::render::view::screenshot::Screenshot>>,
) {
    if time.elapsed_secs_f64() < *next { return; }
    *next = time.elapsed_secs_f64() + 1.0;
    let Ok(home) = std::env::var("NVFOREVER_HOME") else { return };
    let request = std::path::Path::new(&home).join("capture-request");
    let target = std::fs::read_to_string(&request).unwrap_or_default();
    if std::fs::remove_file(request).is_ok() {
        let path = std::path::Path::new(&home).join("capture.png");
        let shot = if target.trim() == "world" {
            let Some(world) = world else { return };
            bevy::render::view::screenshot::Screenshot::image(world.image.clone())
        } else { bevy::render::view::screenshot::Screenshot::primary_window() };
        info!("trimui: screenshot requests pending {}, extracting {}", pending.iter().count(), pending.iter().filter(|p| p.is_some()).count());
        commands.spawn(shot)
            .observe(bevy::render::view::screenshot::save_to_disk(path));
        info!("trimui: capturing displayed game frame");
    }
}

fn menu_world_visibility(menu: Option<Res<nvf_play::main_menu::MainMenu>>, scaled: Option<Res<ScaledWorld>>, state: Res<RendererState>,
    mut cameras: Query<&mut Camera, With<Camera3d>>) {
    let Some(scaled) = scaled else { return };
    // Prepare the native UI/presenter before queueing the world's many pipelines.
    let active = state.ui_ready.load(Ordering::Acquire) && !menu.is_some_and(|m| m.open);
    let Ok(mut camera) = cameras.get_mut(scaled.camera) else { return };
    if camera.is_active != active { camera.is_active = active; }
}

/// Draw each big mesh's simpler copy (nvf-assets `mesh_lod`) where its error covers less
/// than `NVF_MESH_LOD_PIXELS` (default 0.75) of a pixel of the world view, the full mesh
/// nearer; checked every fourth frame, with a tenth of the switching distance as a margin so
/// a shape at the edge does not flip back and forth. The distance is to the nearest point
/// of the shape's bounding sphere.
fn switch_mesh_lods(
    world: Option<Res<ScaledWorld>>,
    cameras: Query<(Entity, &GlobalTransform, &Projection, &Camera, &bevy::camera::RenderTarget), With<Camera3d>>,
    mut shapes: Query<(&nvf_assets::mesh_lod::MeshLod, &mut Mesh3d, &GlobalTransform, Option<&bevy::camera::primitives::Aabb>)>,
    mut frame: Local<u32>,
    mut switched: Local<(u32, u32, f64)>,
    time: Res<Time<Real>>,
) {
    static PIXELS: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    let pixels = *PIXELS.get_or_init(|| std::env::var("NVF_MESH_LOD_PIXELS").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(0.75));
    // `lodpx=` in the tune file overrides it while the game runs (0: full meshes only).
    let tuned = LOD_PIXELS_TUNE.load(Ordering::Relaxed);
    let pixels = if tuned == u32::MAX { pixels } else { f32::from_bits(tuned) };
    *frame = frame.wrapping_add(1);
    if *frame % 4 != 0 || shapes.is_empty() { return; }
    // The world's camera: the scaled world's, else (native size) the 3D camera drawing
    // into the window.
    let found = match &world {
        Some(w) => cameras.get(w.camera).ok(),
        None => cameras.iter().find(|(_, _, _, c, t)| c.is_active && matches!(t, bevy::camera::RenderTarget::Window(_))),
    };
    let Some((_, eye, projection, camera, _)) = found else { return };
    if !camera.is_active { return; }
    let Projection::Perspective(p) = projection else { return };
    let Some(height) = camera.physical_viewport_size().map(|s| s.y).or(world.as_ref().map(|w| w.size.y)) else { return };
    // Pixels per game unit at distance 1: half the view's height over tan(half the fov).
    let focal = height as f32 * 0.5 / (p.fov * 0.5).tan();
    let eye = eye.translation();
    for (lod, mut mesh, at, aabb) in &mut shapes {
        let (scale, _, centre) = at.to_scale_rotation_translation();
        let size = scale.max_element();
        let (centre, radius) = match aabb {
            Some(b) => (at.transform_point(Vec3::from(b.center)), Vec3::from(b.half_extents).length() * size),
            None => (centre, 0.0),
        };
        let distance = (centre.distance(eye) - radius).max(0.0);
        let switch = lod.error * size * focal / pixels;
        let far = mesh.0 == lod.far;
        let want_far = pixels > 0.0 && if far { distance > switch * 0.9 } else { distance > switch * 1.1 };
        if want_far != far {
            mesh.0 = if want_far { lod.far.clone() } else { lod.near.clone() };
            if want_far { switched.0 += 1 } else { switched.1 += 1 }
        }
    }
    let now = time.elapsed_secs_f64();
    if now >= switched.2 && std::env::var_os("NVF_GPU_REPORT").is_some() {
        switched.2 = now + 10.0;
        let far = shapes.iter().filter(|(l, m, ..)| m.0 == l.far).count();
        info!("trimui mesh LOD: {far} of {} shapes drawn simpler; {} switched to the copy, {} back, in 10 s", shapes.iter().count(), switched.0, switched.1);
        switched.0 = 0;
        switched.1 = 0;
    }
}

fn direct_draw_cameras(mut commands: Commands, cameras: Query<Entity, (With<Camera>, Without<bevy::render::view::NoIndirectDrawing>)>) {
    for entity in &cameras { commands.entity(entity).insert(bevy::render::view::NoIndirectDrawing); }
}

fn limit_world_depth(scaled: Option<Res<ScaledWorld>>, mut cameras: Query<(Entity, &mut Projection, &bevy::camera::RenderTarget), With<Camera3d>>) {
    static FAR: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let Some(far) = *FAR.get_or_init(|| std::env::var("NVF_VIEW_DISTANCE").ok()
        .and_then(|s| s.parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 100.0)) else { return };
    for (camera, mut projection, target) in &mut cameras {
        let world = match &scaled {
            Some(scaled) => camera == scaled.camera,
            None => matches!(target, bevy::camera::RenderTarget::Window(_)),
        };
        if world && matches!(&*projection, Projection::Perspective(p) if p.far > far) {
            if let Projection::Perspective(p) = &mut *projection {
                p.far = far;
            }
        }
    }
}

fn drop_depth_prepasses(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, With<bevy::core_pipeline::prepass::DepthPrepass>)>,
    mut points: Query<&mut PointLight>,
    mut spots: Query<&mut SpotLight>,
    mut suns: Query<&mut DirectionalLight>,
) {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*OFF.get_or_init(|| std::env::var("NVF_NO_DEPTH_PREPASS").as_deref() == Ok("1")) { return; }
    for entity in &cameras {
        commands.entity(entity).remove::<bevy::core_pipeline::prepass::DepthPrepass>();
    }
    for mut light in &mut points { if light.shadow_maps_enabled { light.shadow_maps_enabled = false; } }
    for mut light in &mut spots { if light.shadow_maps_enabled { light.shadow_maps_enabled = false; } }
    for mut light in &mut suns { if light.shadow_maps_enabled { light.shadow_maps_enabled = false; } }
}

/// The 2D cameras that draw interface screens into images (the Pip-Boy screen, 1280x960, and
/// the terminal screen) without multisampling. They take Bevy's default 4x, which on opening
/// the Pip-Boy allocated two 18.8 MiB multisampled targets (colour and depth) and kept the GPU
/// at 100 % while it was up; interface text and boxes look the same without it. On
/// 2026-11-09 the device rebooted seconds after the Pip-Boy opened, at 93 % RAM, with that
/// allocation taking 330 ms. `NVF_SCREEN_MSAA=1` keeps the default.
fn screen_cameras_without_msaa(mut cameras: Query<(&bevy::camera::RenderTarget, &mut Msaa, Option<&Name>), With<Camera2d>>, mut done: Local<u32>) {
    static KEEP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *KEEP.get_or_init(|| std::env::var("NVF_SCREEN_MSAA").as_deref() == Ok("1")) { return; }
    for (target, mut msaa, name) in &mut cameras {
        if *msaa == Msaa::Off || !matches!(target, bevy::camera::RenderTarget::Image(_)) { continue; }
        *msaa = Msaa::Off;
        *done += 1;
        info!("trimui: {} without multisampling ({} so far)", name.map_or("an image camera", |n| n.as_str()), *done);
    }
}

const PRESENT_LAYER: usize = 30;

#[derive(Resource)]
struct ScaledWorld {
    camera: Entity,
    image: Handle<Image>,
    /// `NVF_PRESENT_SWAP`: a second world image, the sprite showing it, and which image
    /// the world draws to this frame.
    second: Option<(Handle<Image>, Entity)>,
    flip: bool,
    size: UVec2,
    next_check: f64,
}

/// `NVF_PRESENT_SWAP=delay` (or `present=delay` in the tune file): the screen shows the world
/// image drawn the frame before while the world draws the other one.
static PRESENT_DELAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `present=<none|wait|prev|delay>` in the tune file, else `NVF_PRESENT_WAIT` (0: none, 1: wait,
/// 2 or unset: prev)
/// and `NVF_PRESENT_SWAP=delay`: how frames reach the display (bevy_render
/// `NVF_PRESENT_WAIT_MODE`, [`PRESENT_DELAY`]).
fn set_present_mode(file: Option<&str>) {
    let live = file.and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("present=").map(|v| v.trim().to_string())));
    let alt = match live.as_deref() {
        Some(m) => m.starts_with("alt"),
        None => std::env::var("NVF_PRESENT_SWAP").as_deref() == Ok("alt"),
    };
    let (wait, delay) = match live.as_deref() {
        Some("alt") => (3, false),
        Some("altnone") => (0, false),
        Some("altprev") => (2, false),
        Some("wait") => (1, false),
        Some("prev") => (2, false),
        // Only the passes into the screen's image wait for the previous frame; the world
        // is submitted first and runs meanwhile.
        Some("split") => (3, false),
        Some("delay") => (0, true),
        Some(_) => (0, false),
        // Default 2: on 2026-11-10 the owner saw blocks of the previous frame in the Jean Sky
        // Diving shack with no wait and none with this one, which costs 0.3-0.5 fps there.
        None => (match std::env::var("NVF_PRESENT_WAIT").as_deref() { Ok("0") => 0, Ok("1") => 1, _ => 2 },
            std::env::var("NVF_PRESENT_SWAP").as_deref() == Ok("delay")),
    };
    let old_wait = bevy::render::renderer::NVF_PRESENT_WAIT_MODE.swap(wait, Ordering::Relaxed);
    let old_delay = PRESENT_DELAY.swap(delay, Ordering::Relaxed);
    let old_alt = PRESENT_ALT.swap(alt, Ordering::Relaxed);
    if old_wait != wait || old_delay != delay || old_alt != alt {
        info!("trimui: present mode {}{}", if alt { "alternating images + " } else { "" },
            match (wait, delay) { (1, _) => "wait", (2, _) => "prev", (3, _) => "split", (_, true) => "delay", _ => "none" });
    }
    // `gpusync=<0-7>` in the tune file, else NVF_GPU_SYNC: GPU-side ordering of submissions
    // (vendored wgpu-hal `NVF_GPU_SYNC`; 1 acquire wait at all commands, 2 each submission
    // after the one before has finished, 4 a full barrier ahead of each).
    let live = file.and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("gpusync=").and_then(|v| v.trim().parse::<u8>().ok())));
    let sync = live.or_else(|| std::env::var("NVF_GPU_SYNC").ok().and_then(|v| v.trim().parse().ok())).unwrap_or(0) & 7;
    let old = wgpu::hal::vulkan::NVF_GPU_SYNC.swap(sync, Ordering::Relaxed);
    if old != sync {
        info!("trimui: gpu sync {sync}");
    }
    // `mainpass=<0-2>` in the tune file, else NVF_ONE_MAIN_PASS: the 3D scene in one render
    // pass (1), and its depth not written out at the end (2) (vendored bevy_core_pipeline
    // `NVF_ONE_MAIN_PASS`).
    let live = file.and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("mainpass=").and_then(|v| v.trim().parse::<u8>().ok())));
    let one = live.or_else(|| std::env::var("NVF_ONE_MAIN_PASS").ok().and_then(|v| v.trim().parse().ok())).unwrap_or(0).min(2);
    let old = bevy::core_pipeline::core_3d::NVF_ONE_MAIN_PASS.swap(one, Ordering::Relaxed);
    if old != one {
        info!("trimui: main pass mode {one}");
    }
}

/// Rows the second world image has beyond the first. Bevy's texture cache hands a view its
/// colour and depth textures by size, so a world drawn into the taller image gets its own set:
/// with [`PRESENT_ALT`] consecutive frames share no texture the world writes. The camera's
/// viewport and the sprite's rect keep to the first `size` rows, so the picture is the same.
const SECOND_EXTRA_ROWS: u32 = 2;

fn second_world_image(first: &Image, size: UVec2) -> Image {
    let mut image = first.clone();
    image.resize(bevy::render::render_resource::Extent3d { width: size.x, height: size.y + SECOND_EXTRA_ROWS, depth_or_array_layers: 1 });
    image
}

/// `present=alt` / `present=altnone` (or `NVF_PRESENT_SWAP=alt`): the world draws into the
/// two world images in turn and the screen shows the one drawn this frame. For the PowerVR
/// "blocks": the next frame's world overlapping the frame before on the GPU (docs/trimui.md).
static PRESENT_ALT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn swap_world_image(world: Option<ResMut<ScaledWorld>>, mut targets: Query<(&mut bevy::camera::RenderTarget, &mut Camera)>, mut sprites: Query<&mut Sprite>) {
    let Some(mut world) = world else { return };
    let Some((second, sprite)) = world.second.clone() else { return };
    let delay = PRESENT_DELAY.load(Ordering::Relaxed);
    let alt = PRESENT_ALT.load(Ordering::Relaxed);
    if !delay && !alt && !world.flip { return; }
    world.flip = (delay || alt) && !world.flip;
    let (draw, other) = if world.flip { (second.clone(), world.image.clone()) } else { (world.image.clone(), second.clone()) };
    let show = if delay { other } else { draw.clone() };
    let size = world.size;
    if let Ok((mut target, mut camera)) = targets.get_mut(world.camera) {
        camera.viewport = (draw == second).then(|| bevy::camera::Viewport { physical_position: UVec2::ZERO, physical_size: size, ..default() });
        *target = bevy::camera::RenderTarget::Image(draw.into());
    }
    if let Ok(mut s) = sprites.get_mut(sprite) {
        s.rect = (show == second).then(|| Rect::new(0.0, 0.0, size.x as f32, size.y as f32));
        s.image = show;
    }
}

fn requested_render_size() -> Option<UVec2> {
    let value = std::env::var("NVF_RENDER_SIZE_FILE").ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .or_else(|| std::env::var("NVF_RENDER_SIZE").ok())?;
    let (w, h) = value.trim().split_once('x')?;
    let size = UVec2::new(w.parse().ok()?, h.parse().ok()?);
    (size.x >= 160 && size.y >= 90 && size.x <= 1280 && size.y <= 720).then_some(size)
}

/// Render the world to a smaller image, then present it under the native-size UI.
/// This follows Bevy's render-to-texture canvas path, keeping all 3D post-processing
/// at the smaller size too. Auxiliary image cameras (Pip-Boy, reflections) are untouched.
fn setup_scaled_world(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(Entity, &mut Camera, &bevy::camera::RenderTarget), With<Camera3d>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Some(size) = requested_render_size() else { return };
    let Ok(window) = windows.single() else { return };
    for (entity, mut camera, target) in &mut cameras {
        if !matches!(target, bevy::camera::RenderTarget::Window(_)) { continue; }
        let mut image = Image::new_target_texture(size.x, size.y,
            bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb, None);
        image.sampler = bevy::image::ImageSampler::linear();
        let second_image = Some(images.add(second_world_image(&image, size)));
        let image = images.add(image);
        camera.order = -1;
        commands.entity(entity).insert(bevy::camera::RenderTarget::Image(image.clone().into()));
        let screen = Vec2::new(window.physical_width() as f32, window.physical_height() as f32);
        let sprite = commands.spawn((
            Sprite { image: image.clone(), custom_size: Some(screen), ..default() },
            bevy::camera::visibility::RenderLayers::layer(PRESENT_LAYER),
        )).id();
        commands.spawn((
            Camera2d,
            Camera { order: 1, ..default() },
            Msaa::Off,
            IsDefaultUiCamera,
            bevy::camera::visibility::RenderLayers::layer(PRESENT_LAYER),
        ));
        let second = second_image.map(|i| (i, sprite));
        commands.insert_resource(ScaledWorld { camera: entity, image, second, flip: false, size, next_check: 0.0 });
        info!("trimui: world render {}x{}, interface {}x{}", size.x, size.y, screen.x, screen.y);
        break;
    }
}

fn resize_world(world: Option<ResMut<ScaledWorld>>, time: Res<Time<Real>>, mut images: ResMut<Assets<Image>>) {
    let Some(mut world) = world else { return };
    if time.elapsed_secs_f64() < world.next_check { return; }
    world.next_check = time.elapsed_secs_f64() + 2.0;
    let Some(size) = requested_render_size() else { return };
    if size != world.size {
        let extent = bevy::render::render_resource::Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 };
        if let Some((second, _)) = world.second.clone() {
            let extent = bevy::render::render_resource::Extent3d { height: size.y + SECOND_EXTRA_ROWS, ..extent };
            if let Some(mut image) = images.get_mut(&second) { image.resize(extent); }
        }
        if let Some(mut image) = images.get_mut(&world.image) {
            image.resize(extent);
            world.size = size;
            info!("trimui: world render changed to {}x{}", size.x, size.y);
        }
    }
}

fn performance(time: Res<Time<Real>>, state: Res<RendererState>, mut samples: Local<Vec<f32>>, mut elapsed: Local<f32>, mut events: MessageReader<AssetEvent<nvf_assets::NifMaterial>>, mut changed: Local<u32>) {
    *changed += events.read().filter(|e| matches!(e, AssetEvent::Modified { .. })).count() as u32;
    let dt = time.delta_secs();
    if dt <= 0.0 { return; }
    *elapsed += dt;
    samples.push(dt);
    if *elapsed < 10.0 { return; }
    let fps = samples.len() as f32 / *elapsed;
    samples.sort_unstable_by(f32::total_cmp);
    let p95 = samples[((samples.len() as f32 * 0.95) as usize).min(samples.len() - 1)] * 1000.0;
    let p99 = samples[((samples.len() as f32 * 0.99) as usize).min(samples.len() - 1)] * 1000.0;
    info!("trimui performance: {:.1} fps, {:.1} ms mean, {:.1} ms p95, {:.1} ms p99, {} frames in {:.1} s, {} pipelines pending, {:.1} material changes/frame",
        fps, *elapsed * 1000.0 / samples.len() as f32, p95, p99, samples.len(), *elapsed, state.pending.load(Ordering::Relaxed), *changed as f32 / samples.len() as f32);
    samples.clear();
    *elapsed = 0.0;
    *changed = 0;
}

/// The screen as a window: what the raw handles stand for (nothing to keep alive).
struct Screen;

impl HasWindowHandle for Screen {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: a DRM handle holds no pointer; plane 0 (unused by khr_display.rs).
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Drm(DrmWindowHandle::new(0))) })
    }
}

impl HasDisplayHandle for Screen {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: fd -1 asks the patched wgpu-hal for VK_KHR_display, not a DRM device.
        Ok(unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Drm(DrmDisplayHandle::new(-1))) })
    }
}

fn attach_screen(mut commands: Commands, mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>) {
    for (e, mut w) in &mut windows {
        match RawHandleWrapper::new(&WindowWrapper::new(Screen)) {
            Ok(h) => {
                commands.entity(e).insert(h);
            }
            Err(err) => error!("trimui: no screen handle: {err}"),
        }
        w.focused = true;
        w.visible = true;
        info!("trimui: screen {}x{} through VK_KHR_display", w.physical_width(), w.physical_height());
    }
}

// Linux input event codes [documented: linux/input-event-codes.h].
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const BTN_SOUTH: u16 = 0x130;
const BTN_MODE: u16 = 0x13c;
const KEY_MENU: u16 = 139;
const ABS_CNT: usize = 0x40;

/// `struct input_absinfo` [documented: linux/input.h].
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

const fn ioc_read(nr: u32, size: u32) -> libc::c_ulong {
    ((2u32 << 30) | (size << 16) | ((b'E' as u32) << 8) | nr) as libc::c_ulong
}

struct PadDevice {
    file: File,
    name: String,
    abs: Vec<AbsInfo>,
}

#[derive(Default)]
struct Pads {
    devices: Vec<PadDevice>,
    gamepad: Option<Entity>,
    next_scan: f64,
    seen: std::collections::HashSet<(u16, u16)>,
}

fn has_bit(bits: &[u8], n: usize) -> bool {
    bits.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

/// The evdev devices that have gamepad buttons (or the Menu key, which TrimUI's firmware
/// may report from a device of its own).
fn scan() -> Vec<PadDevice> {
    let mut out = Vec::new();
    for i in 0..32 {
        let path = format!("/dev/input/event{i}");
        let Ok(file) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&path) else { continue };
        let fd = file.as_raw_fd();
        let mut keys = [0u8; 0x300 / 8];
        let mut name = [0u8; 128];
        // SAFETY: EVIOCGBIT(EV_KEY) and EVIOCGNAME write at most the given lengths.
        unsafe {
            libc::ioctl(fd, ioc_read(0x20 + EV_KEY as u32, keys.len() as u32), keys.as_mut_ptr());
            libc::ioctl(fd, ioc_read(0x06, name.len() as u32), name.as_mut_ptr());
        }
        if !has_bit(&keys, BTN_SOUTH as usize) && !has_bit(&keys, BTN_MODE as usize) && !has_bit(&keys, KEY_MENU as usize) {
            continue;
        }
        let mut abs = vec![AbsInfo::default(); ABS_CNT];
        for (a, info) in abs.iter_mut().enumerate() {
            // SAFETY: EVIOCGABS(a) writes one input_absinfo.
            unsafe { libc::ioctl(fd, ioc_read(0x40 + a as u32, size_of::<AbsInfo>() as u32), info as *mut AbsInfo) };
        }
        let name = String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap_or(0)]).into_owned();
        info!("trimui: controls from {path} ({name})");
        out.push(PadDevice { file, name, abs });
    }
    out
}

fn env_on(k: &str) -> bool {
    std::env::var(k).is_ok_and(|v| v == "1")
}

fn button(code: u16) -> Option<GamepadButton> {
    let (swap_ab, swap_xy) = (env_on("NVF_PAD_SWAP_AB"), env_on("NVF_PAD_SWAP_XY"));
    Some(match code {
        0x130 if swap_ab => GamepadButton::East,
        0x130 => GamepadButton::South,
        0x131 if swap_ab => GamepadButton::South,
        0x131 => GamepadButton::East,
        // BTN_X / BTN_Y: Xbox-style pads report X (left) as BTN_X and Y (top) as BTN_Y.
        0x133 if swap_xy => GamepadButton::North,
        0x133 => GamepadButton::West,
        0x134 if swap_xy => GamepadButton::West,
        0x134 => GamepadButton::North,
        0x136 => GamepadButton::LeftTrigger,
        0x137 => GamepadButton::RightTrigger,
        0x138 => GamepadButton::LeftTrigger2,
        0x139 => GamepadButton::RightTrigger2,
        0x13a => GamepadButton::Select,
        0x13b => GamepadButton::Start,
        BTN_MODE | KEY_MENU => GamepadButton::Mode,
        0x13d => GamepadButton::LeftThumb,
        0x13e => GamepadButton::RightThumb,
        0x220 => GamepadButton::DPadUp,
        0x221 => GamepadButton::DPadDown,
        0x222 => GamepadButton::DPadLeft,
        0x223 => GamepadButton::DPadRight,
        _ => return None,
    })
}

/// Raw evdev value as -1..1 for a stick (`stick`, y up) or 0..1 for a trigger.
fn scaled(info: &AbsInfo, v: i32, flip: bool) -> f32 {
    let (lo, hi) = (info.minimum as f32, info.maximum as f32);
    if hi <= lo {
        return v.signum() as f32;
    }
    let s = ((v as f32 - lo) / (hi - lo) * 2.0 - 1.0).clamp(-1.0, 1.0);
    if flip {
        -s
    } else {
        s
    }
}

enum Change {
    Button(GamepadButton, f32),
    Axis(GamepadAxis, f32),
}

fn changes(dev: &PadDevice, kind: u16, code: u16, value: i32) -> Vec<Change> {
    let abs = |a: u16| dev.abs.get(a as usize).copied().unwrap_or_default();
    let trigger = |a: u16| (scaled(&abs(a), value, false) + 1.0) * 0.5;
    match (kind, code) {
        (EV_KEY, c) => button(c).map(|b| Change::Button(b, if value != 0 { 1.0 } else { 0.0 })).into_iter().collect(),
        (EV_ABS, 0x00) => vec![Change::Axis(GamepadAxis::LeftStickX, scaled(&abs(code), value, false))],
        (EV_ABS, 0x01) => vec![Change::Axis(GamepadAxis::LeftStickY, scaled(&abs(code), value, true))],
        (EV_ABS, 0x03) => vec![Change::Axis(GamepadAxis::RightStickX, scaled(&abs(code), value, false))],
        (EV_ABS, 0x04) => vec![Change::Axis(GamepadAxis::RightStickY, scaled(&abs(code), value, true))],
        // ABS_Z / ABS_RZ (or ABS_BRAKE / ABS_GAS): analogue triggers.
        (EV_ABS, 0x02 | 0x0a) => vec![Change::Button(GamepadButton::LeftTrigger2, trigger(code))],
        (EV_ABS, 0x05 | 0x09) => vec![Change::Button(GamepadButton::RightTrigger2, trigger(code))],
        (EV_ABS, 0x10) => vec![
            Change::Button(GamepadButton::DPadLeft, if value < 0 { 1.0 } else { 0.0 }),
            Change::Button(GamepadButton::DPadRight, if value > 0 { 1.0 } else { 0.0 }),
        ],
        (EV_ABS, 0x11) => vec![
            Change::Button(GamepadButton::DPadUp, if value < 0 { 1.0 } else { 0.0 }),
            Change::Button(GamepadButton::DPadDown, if value > 0 { 1.0 } else { 0.0 }),
        ],
        _ => Vec::new(),
    }
}

fn read_pads(
    mut pads: NonSendMut<Pads>,
    time: Res<Time<Real>>,
    mut commands: Commands,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut buttons: MessageWriter<RawGamepadButtonChangedEvent>,
    mut axes: MessageWriter<RawGamepadAxisChangedEvent>,
) {
    let now = time.elapsed_secs_f64();
    let pads = &mut *pads;
    if pads.devices.is_empty() && now >= pads.next_scan {
        pads.devices = scan();
        pads.next_scan = now + 2.0;
        if pads.devices.is_empty() {
            return;
        }
    }
    let gamepad = *pads.gamepad.get_or_insert_with(|| {
        let e = commands.spawn_empty().id();
        let name = pads.devices.first().map_or_else(|| "TrimUI".to_string(), |d| d.name.clone());
        connections.write(GamepadConnectionEvent::new(e, GamepadConnection::Connected { name, vendor_id: None, product_id: None }));
        e
    });
    const SIZE: usize = 24; // struct input_event on 64-bit Linux
    let mut buf = [0u8; SIZE * 64];
    let mut lost = false;
    for dev in &mut pads.devices {
        loop {
            let n = match dev.file.read(&mut buf) {
                Ok(0) => {
                    lost = true;
                    break;
                }
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    lost = true;
                    break;
                }
            };
            for ev in buf[..n].chunks_exact(SIZE) {
                let kind = u16::from_ne_bytes([ev[16], ev[17]]);
                let code = u16::from_ne_bytes([ev[18], ev[19]]);
                let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                if kind == EV_KEY && value == 1 && pads.seen.insert((kind, code)) {
                    info!("trimui: {} key code {code:#x} -> {:?}", dev.name, button(code));
                }
                for c in changes(dev, kind, code, value) {
                    match c {
                        Change::Button(b, v) => {
                            let e = RawGamepadButtonChangedEvent::new(gamepad, b, v);
                            raw.write(e.into());
                            buttons.write(e);
                        }
                        Change::Axis(a, v) => {
                            let e = RawGamepadAxisChangedEvent::new(gamepad, a, v);
                            raw.write(e.into());
                            axes.write(e);
                        }
                    }
                }
            }
        }
    }
    if lost {
        warn!("trimui: a controls device went away; scanning again");
        pads.devices.clear();
    }
}

/// Menu + Start held for a second quits.
fn quit_combo(pads: Query<&Gamepad>, time: Res<Time<Real>>, mut held: Local<f32>, mut exit: MessageWriter<AppExit>) {
    let both = pads.iter().any(|p| p.pressed(GamepadButton::Mode) && p.pressed(GamepadButton::Start));
    *held = if both { *held + time.delta_secs() } else { 0.0 };
    if *held > 1.0 {
        info!("trimui: Menu + Start, quitting");
        exit.write(AppExit::Success);
        *held = f32::MIN;
    }
}
