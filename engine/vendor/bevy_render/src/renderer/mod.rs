#[cfg(feature = "raw_vulkan_init")]
pub mod raw_vulkan_init;
mod render_context;
mod render_device;
mod wgpu_wrapper;

pub use render_context::{
    CurrentView, FlushCommands, PendingCommandBuffers, RenderContext, RenderContextState, ViewQuery,
};
pub use render_device::*;

pub(crate) use wgpu_wrapper::wgpu_wrapper;

use crate::{
    settings::{RenderResources, WgpuSettings, WgpuSettingsPriority},
    sync_world::MainEntity,
    view::{screenshot::SubmitScreenshotCommandsState, ExtractedWindow, ViewTarget},
};
use bevy_camera::NormalizedRenderTarget;
use bevy_ecs::schedule::ScheduleLabel;
use bevy_ecs::{prelude::*, system::SystemState};
#[cfg(feature = "trace")]
use bevy_log::info_span;
use bevy_log::{debug, info, warn};
use bevy_render::camera::ExtractedCamera;
use bevy_window::RawHandleWrapperHolder;
use wgpu::{
    Adapter, AdapterInfo, Backends, DeviceType, ForceShaderModelToken, Instance, Queue,
    RequestAdapterOptions, Trace,
};

/// Schedule label for the root render graph schedule. This schedule runs once per frame
/// in the [`render_system`] system and is responsible for driving the entire rendering process.
///
/// The default bevy render graph is executed as follows:
/// ```ignore
/// RenderApp (App)
///   Render (Schedule)
///     RenderSystems::Render (SystemSet)
///       render_system (System)
///         RenderGraph (Schedule)
///           camera_driver (System)
///             Core2d (Schedule)
///             Core3d (Schedule)
///             CUSTOM_PIPELINE (Schedule)
/// ```
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct RenderGraph;

impl RenderGraph {
    pub fn base_schedule() -> Schedule {
        let mut schedule = Schedule::new(Self);
        schedule.configure_sets(
            (
                RenderGraphSystems::Begin,
                RenderGraphSystems::Render,
                RenderGraphSystems::Submit,
                RenderGraphSystems::Finish,
            )
                .chain_weak(),
        );
        schedule
    }
}

/// System sets for the root [`RenderGraph`] schedule.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RenderGraphSystems {
    /// Runs before rendering. Used for per-frame setup.
    Begin,
    /// The main rendering phase.
    Render,
    /// Submits pending command buffers generated during [`RenderGraphSystems::Render`]
    Submit,
    /// Runs after rendering and submit. Used for per-frame finalization.
    Finish,
}

/// NVForever (TrimUI): how frames wait for the GPU before they reach the display. 0: no wait;
/// 1: the render thread waits for the frame's GPU work before presenting it (`render_system`);
/// 2: it waits for the previous frame's GPU work before submitting this one
/// (`bevy_core_pipeline` `submit_pending_command_buffers`). Set by the game (`NVF_PRESENT_WAIT`,
/// or live from its tune file); the PowerVR display swapchain showed frames still being drawn.
pub static NVF_PRESENT_WAIT_MODE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

fn nvf_present_wait() -> bool {
    NVF_PRESENT_WAIT_MODE.load(core::sync::atomic::Ordering::Relaxed) == 1
}

/// NVForever (TrimUI), mode 3 of [`NVF_PRESENT_WAIT_MODE`] ("split"): the submission that
/// closes everything submitted before this frame's rendering began.
static NVF_SPLIT_MARK: std::sync::Mutex<Option<wgpu::SubmissionIndex>> = std::sync::Mutex::new(None);

/// Mode 3: at the start of the frame's camera rendering, mark the end of the GPU work
/// submitted so far (the previous frame) with an empty submission. Returns whether mode 3 is on.
pub fn nvf_split_mark(queue: &RenderQueue) -> bool {
    if NVF_PRESENT_WAIT_MODE.load(core::sync::atomic::Ordering::Relaxed) != 3 {
        return false;
    }
    let index = queue.submit(core::iter::empty::<wgpu::CommandBuffer>());
    *NVF_SPLIT_MARK.lock().unwrap_or_else(|e| e.into_inner()) = Some(index);
    true
}

/// Mode 3: before the first pass that draws into a window's swapchain image, submit the frame's
/// work recorded so far (the off-screen world) so the GPU can start it, then wait until the
/// previous frame's GPU work has finished. Only the swapchain passes wait, not the world.
pub fn nvf_split_wait(pending: &mut PendingCommandBuffers, queue: &RenderQueue, device: &RenderDevice) {
    let Some(index) = NVF_SPLIT_MARK.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let buffers: Vec<_> = pending.finish().collect();
    if !buffers.is_empty() {
        queue.submit(buffers);
    }
    let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: None });
}

/// The main render system that drives the rendering process. This system runs the [`RenderGraph`]
/// schedule, runs any finalization commands like screenshot captures and GPU readbacks, and
/// calls present on swap chains that need to be presented.
pub fn render_system(
    world: &mut World,
    present_state: &mut SystemState<(
        Query<(&ViewTarget, &ExtractedCamera)>,
        Query<(MainEntity, &mut ExtractedWindow)>,
        Res<RenderQueue>,
    )>,
    screenshot_state: &mut SystemState<SubmitScreenshotCommandsState>,
) {
    #[cfg(feature = "trace")]
    let _span = info_span!("main_render_schedule").entered();

    world.run_schedule(RenderGraph);

    {
        let render_device = world.resource::<RenderDevice>();
        let render_queue = world.resource::<RenderQueue>();

        let mut encoder =
            render_device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        crate::view::screenshot::submit_screenshot_commands(world, screenshot_state, &mut encoder);
        crate::gpu_readback::submit_readback_commands(world, &mut encoder);

        render_queue.submit([encoder.finish()]);
        // NVForever (TrimUI): `NVF_PRESENT_WAIT=1` waits for the frame's GPU work to finish
        // before presenting; the PowerVR display swapchain showed frames still being drawn.
        if nvf_present_wait() {
            let _ = render_device.poll(wgpu::PollType::wait_indefinitely());
        }
    }

    {
        #[cfg(feature = "trace")]
        let _span = info_span!("present_frames").entered();

        if let Ok((views, mut windows, render_queue)) = present_state.get_mut(world) {
            for (window_entity, mut window) in &mut windows {
                let view_needs_present = views.iter().any(|(view_target, camera)| {
                    matches!(
                        camera.target,
                        Some(NormalizedRenderTarget::Window(w)) if w.entity() == window_entity
                    ) && view_target.needs_present()
                });

                if view_needs_present || window.needs_initial_present {
                    window.present(&render_queue);
                    window.needs_initial_present = false;
                }
            }
        }

        #[cfg(feature = "tracing-tracy")]
        bevy_log::event!(
            bevy_log::Level::INFO,
            message = "finished frame",
            tracy.frame_mark = true
        );
    }

    crate::view::screenshot::collect_screenshots(world);
}

wgpu_wrapper! {
    /// This queue is used to enqueue tasks for the GPU to execute asynchronously.
    #[derive(Resource, Clone)]
    pub struct RenderQueue(Queue);

    /// The handle to the physical device being used for rendering.
    /// See [`Adapter`] for more info.
    #[derive(Resource, Clone, Debug)]
    pub struct RenderAdapter(Adapter);

    /// The GPU instance is used to initialize the [`RenderQueue`] and [`RenderDevice`].
    #[derive(Resource, Clone)]
    pub struct RenderInstance(Instance);

    /// The [`AdapterInfo`] of the adapter in use by the renderer.
    #[derive(Resource, Clone)]
    pub struct RenderAdapterInfo(AdapterInfo);
}

const GPU_NOT_FOUND_ERROR_MESSAGE: &str = if cfg!(target_os = "linux") {
    "Unable to find a GPU! Make sure you have installed required drivers! For extra information, see: https://github.com/bevyengine/bevy/blob/latest/docs/linux_dependencies.md"
} else {
    "Unable to find a GPU! Make sure you have installed required drivers!"
};

#[cfg(not(target_family = "wasm"))]
async fn find_adapter_by_name(
    instance: &Instance,
    options: &WgpuSettings,
    compatible_surface: Option<&wgpu::Surface<'_>>,
    adapter_name: &str,
) -> Option<Adapter> {
    for adapter in instance
        .enumerate_adapters(options.backends.expect(
            "The `backends` field of `WgpuSettings` must be set to use a specific adapter.",
        ))
        .await
    {
        bevy_log::trace!("Checking adapter: {:?}", adapter.get_info());
        let info = adapter.get_info();
        if let Some(surface) = compatible_surface
            && !adapter.is_surface_supported(surface)
        {
            continue;
        }

        if info
            .name
            .to_lowercase()
            .contains(&adapter_name.to_lowercase())
        {
            return Some(adapter);
        }
    }
    None
}

/// Initializes the renderer by retrieving and preparing the GPU instance, device and queue
/// for the specified backend.
pub async fn initialize_renderer(
    backends: Backends,
    primary_window: Option<RawHandleWrapperHolder>,
    options: &WgpuSettings,
    #[cfg(feature = "raw_vulkan_init")]
    raw_vulkan_init_settings: raw_vulkan_init::RawVulkanInitSettings,
) -> RenderResources {
    let instance_descriptor = wgpu::InstanceDescriptor {
        backends,
        flags: options.instance_flags,
        memory_budget_thresholds: options.instance_memory_budget_thresholds,
        display: None,
        backend_options: wgpu::BackendOptions {
            gl: wgpu::GlBackendOptions {
                gles_minor_version: options.gles3_minor_version,
                fence_behavior: wgpu::GlFenceBehavior::Normal,
                debug_fns: wgpu::GlDebugFns::Auto,
            },
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: options.dx12_shader_compiler.clone(),
                presentation_system: wgpu::wgt::Dx12SwapchainKind::from_env().unwrap_or_default(),
                latency_waitable_object: wgpu::wgt::Dx12UseFrameLatencyWaitableObject::from_env()
                    .unwrap_or_default(),
                force_shader_model: ForceShaderModelToken::default(),
                agility_sdk: None,
            },
            noop: wgpu::NoopBackendOptions {
                enable: false,
                ..Default::default()
            },
        },
    };

    #[cfg(not(feature = "raw_vulkan_init"))]
    let instance = Instance::new(instance_descriptor);
    #[cfg(feature = "raw_vulkan_init")]
    let mut additional_vulkan_features = raw_vulkan_init::AdditionalVulkanFeatures::default();
    #[cfg(feature = "raw_vulkan_init")]
    let instance = raw_vulkan_init::create_raw_vulkan_instance(
        instance_descriptor,
        &raw_vulkan_init_settings,
        &mut additional_vulkan_features,
    );

    let surface = primary_window.and_then(|wrapper| {
        let maybe_handle = wrapper
            .0
            .lock()
            .expect("Couldn't get the window handle in time for renderer initialization");
        if let Some(wrapper) = maybe_handle.as_ref() {
            // SAFETY: Plugins should be set up on the main thread.
            let handle = unsafe { wrapper.get_handle() };
            Some(
                instance
                    .create_surface(handle)
                    .expect("Failed to create wgpu surface"),
            )
        } else {
            None
        }
    });

    let force_fallback_adapter = std::env::var("WGPU_FORCE_FALLBACK_ADAPTER")
        .map_or(options.force_fallback_adapter, |v| {
            !(v.is_empty() || v == "0" || v == "false")
        });

    let desired_adapter_name = std::env::var("WGPU_ADAPTER_NAME")
        .as_deref()
        .map_or(options.adapter_name.clone(), |x| Some(x.to_lowercase()));

    let request_adapter_options = RequestAdapterOptions {
        power_preference: options.power_preference,
        compatible_surface: surface.as_ref(),
        force_fallback_adapter,
        apply_limit_buckets: false,
    };

    #[cfg(not(target_family = "wasm"))]
    let mut selected_adapter = if let Some(adapter_name) = desired_adapter_name {
        find_adapter_by_name(
            &instance,
            options,
            request_adapter_options.compatible_surface,
            &adapter_name,
        )
        .await
    } else {
        None
    };
    #[cfg(target_family = "wasm")]
    let mut selected_adapter = None;

    #[cfg(target_family = "wasm")]
    if desired_adapter_name.is_some() {
        warn!("Choosing an adapter is not supported on wasm.");
    }

    if selected_adapter.is_none() {
        debug!(
            "Searching for adapter with options: {:?}",
            request_adapter_options
        );
        selected_adapter = instance
            .request_adapter(&request_adapter_options)
            .await
            .ok();
    }

    let adapter = selected_adapter.expect(GPU_NOT_FOUND_ERROR_MESSAGE);
    let adapter_info = adapter.get_info();
    info!("{:?}", adapter_info);

    if adapter_info.device_type == DeviceType::Cpu {
        warn!(
            "The selected adapter is using a driver that only supports software rendering. \
             This is likely to be very slow. See https://bevy.org/learn/errors/b0006/"
        );
    }

    // Maybe get features and limits based on what is supported by the adapter/backend
    let mut features = wgpu::Features::empty();
    let mut limits = options.limits.clone();
    if matches!(options.priority, WgpuSettingsPriority::Functionality) {
        features = adapter.features();
        // Keep the handheld's actual (below WebGPU-default) limits, while avoiding
        // optional features exposed by its old PowerVR driver.
        if std::env::var("NVF_VK_MINIMAL_FEATURES").as_deref() == Ok("1") {
            features = wgpu::Features::empty();
        }
        if adapter_info.device_type == DeviceType::DiscreteGpu {
            // `MAPPABLE_PRIMARY_BUFFERS` can have a significant, negative performance impact for
            // discrete GPUs due to having to transfer data across the PCI-E bus and so it
            // should not be automatically enabled in this case. It is however beneficial for
            // integrated GPUs.
            features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        }

        limits = adapter.limits();
    }

    // Enforce the disabled features
    if let Some(disabled_features) = options.disabled_features {
        features.remove(disabled_features);
    }
    // NOTE: |= is used here to ensure that any explicitly-enabled features are respected.
    features |= options.features;

    // Enforce the limit constraints
    if let Some(constrained_limits) = options.constrained_limits.as_ref() {
        // NOTE: Respect the configured limits as an 'upper bound'. This means for 'max' limits, we
        // take the minimum of the calculated limits according to the adapter/backend and the
        // specified max_limits. For 'min' limits, take the maximum instead. This is intended to
        // err on the side of being conservative. We can't claim 'higher' limits that are supported
        // but we can constrain to 'lower' limits.
        limits = limits.or_worse_values_from(constrained_limits);
    }

    let device_descriptor = wgpu::DeviceDescriptor {
        label: options.device_label.as_ref().map(AsRef::as_ref),
        required_features: features,
        required_limits: limits,
        // SAFETY: TODO, see https://github.com/bevyengine/bevy/issues/22082
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        memory_hints: options.memory_hints.clone(),
        // See https://github.com/gfx-rs/wgpu/issues/5974
        trace: Trace::Off,
    };

    #[cfg(not(feature = "raw_vulkan_init"))]
    let (device, queue) = adapter.request_device(&device_descriptor).await.unwrap();

    #[cfg(feature = "raw_vulkan_init")]
    let (device, queue) = raw_vulkan_init::create_raw_device(
        &adapter,
        &device_descriptor,
        &raw_vulkan_init_settings,
        &mut additional_vulkan_features,
    )
    .await
    .unwrap();

    debug!("Configured wgpu adapter Limits: {:#?}", device.limits());
    debug!("Configured wgpu adapter Features: {:#?}", device.features());

    RenderResources(
        RenderDevice::from(device),
        RenderQueue::new(queue),
        RenderAdapterInfo::new(adapter_info),
        RenderAdapter::new(adapter),
        RenderInstance::new(instance),
        #[cfg(feature = "raw_vulkan_init")]
        additional_vulkan_features,
    )
}
