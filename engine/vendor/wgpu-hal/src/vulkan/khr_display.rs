//! NVForever: a surface straight on the screen through `VK_KHR_display`, for devices with
//! no window system (the TrimUI Smart Pro's PowerVR GE8300 driver offers it, and no
//! X11, Wayland or DRM display extension). Asked for with a `Drm` window handle whose
//! display handle has no file descriptor (`fd < 0`); `plane` is unused.
//!
//! The mode: the one `NVF_DISPLAY_SIZE` (`<w>x<h>`) names, else the largest the display
//! offers. The plane: the first that can show that display with opaque alpha.

use alloc::{format, string::ToString};

use ash::{khr, vk};

impl super::Instance {
    pub fn create_surface_from_khr_display(&self, _plane: u32) -> Result<super::Surface, crate::InstanceError> {
        if !self.shared.extensions.contains(&khr::display::NAME) {
            return Err(crate::InstanceError::new("Vulkan driver does not support VK_KHR_display".to_string()));
        }
        let want = std::env::var("NVF_DISPLAY_SIZE").ok().and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
        });
        let display_instance = khr::display::Instance::new(&self.shared.entry, &self.shared.raw);
        let devices = unsafe { self.shared.raw.enumerate_physical_devices() }
            .map_err(|e| crate::InstanceError::with_source("enumerate physical devices".to_string(), e))?;
        for pd in devices {
            let Ok(displays) = (unsafe { display_instance.get_physical_device_display_properties(pd) }) else { continue };
            let planes = unsafe { display_instance.get_physical_device_display_plane_properties(pd) }.unwrap_or_default();
            for d in &displays {
                let Ok(mut modes) = (unsafe { display_instance.get_display_mode_properties(pd, d.display) }) else { continue };
                modes.sort_by_key(|m| core::cmp::Reverse(m.parameters.visible_region.width * m.parameters.visible_region.height));
                let Some(mode) = modes.iter().find(|m| want.is_none_or(|(w, h)| m.parameters.visible_region.width == w && m.parameters.visible_region.height == h)) else { continue };
                for (p, plane) in planes.iter().enumerate() {
                    let p = p as u32;
                    let Ok(supported) = (unsafe { display_instance.get_display_plane_supported_displays(pd, p) }) else { continue };
                    if !supported.contains(&d.display) {
                        continue;
                    }
                    let Ok(caps) = (unsafe { display_instance.get_display_plane_capabilities(pd, mode.display_mode, p) }) else { continue };
                    if !caps.supported_alpha.contains(vk::DisplayPlaneAlphaFlagsKHR::OPAQUE) {
                        continue;
                    }
                    let ext = |e: vk::Extent2D| format!("{}x{}", e.width, e.height);
                    log::info!("VK_KHR_display plane {p}: source {}..{}, destination {}..{}; modes {}",
                        ext(caps.min_src_extent), ext(caps.max_src_extent), ext(caps.min_dst_extent), ext(caps.max_dst_extent),
                        modes.iter().map(|m| format!("{}@{}", ext(m.parameters.visible_region), m.parameters.refresh_rate)).collect::<alloc::vec::Vec<_>>().join(" "));
                    // NVF_SURFACE_SIZE=<w>x<h>: a smaller image the display plane scales up to the mode.
                    let image = std::env::var("NVF_SURFACE_SIZE").ok().and_then(|s| {
                        let (w, h) = s.split_once('x')?;
                        Some(vk::Extent2D { width: w.trim().parse().ok()?, height: h.trim().parse().ok()? })
                    }).unwrap_or(mode.parameters.visible_region);
                    let info = vk::DisplaySurfaceCreateInfoKHR::default()
                        .display_mode(mode.display_mode)
                        .plane_index(p)
                        .plane_stack_index(plane.current_stack_index)
                        .transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                        .global_alpha(1.0)
                        .alpha_mode(vk::DisplayPlaneAlphaFlagsKHR::OPAQUE)
                        .image_extent(image);
                    let surface = unsafe { display_instance.create_display_plane_surface(&info, None) }
                        .map_err(|e| crate::InstanceError::with_source("vkCreateDisplayPlaneSurfaceKHR".to_string(), e))?;
                    let e = mode.parameters.visible_region;
                    log::info!("VK_KHR_display surface: {}x{} @ {} mHz, plane {p}, image {}", e.width, e.height, mode.parameters.refresh_rate, ext(image));
                    return Ok(self.create_surface_from_vk_surface_khr(surface, None));
                }
            }
        }
        Err(crate::InstanceError::new(format!("no VK_KHR_display mode and plane found (NVF_DISPLAY_SIZE {want:?})")))
    }
}
