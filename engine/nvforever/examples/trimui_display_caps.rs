//! Read-only display queries; creates temporary surfaces but never presents to them.
use ash::{vk, Entry};

fn main() {
    unsafe {
        let entry = Entry::load().expect("Vulkan loader");
        let extensions = [ash::khr::surface::NAME.as_ptr(), ash::khr::display::NAME.as_ptr()];
        let info = vk::InstanceCreateInfo::default().enabled_extension_names(&extensions);
        let instance = entry.create_instance(&info, None).expect("instance");
        let display = ash::khr::display::Instance::new(&entry, &instance);
        let surface = ash::khr::surface::Instance::new(&entry, &instance);
        for pd in instance.enumerate_physical_devices().unwrap() {
            for ext in instance.enumerate_device_extension_properties(pd).unwrap() {
                let name = std::ffi::CStr::from_ptr(ext.extension_name.as_ptr()).to_string_lossy();
                if name.contains("swapchain") { println!("extension {name}"); }
            }
            let planes = display.get_physical_device_display_plane_properties(pd).unwrap();
            for d in display.get_physical_device_display_properties(pd).unwrap() {
                for mode in display.get_display_mode_properties(pd, d.display).unwrap() {
                    println!("mode {:?}, {} mHz", mode.parameters.visible_region, mode.parameters.refresh_rate);
                    for (plane, property) in planes.iter().enumerate() {
                        let caps = display.get_display_plane_capabilities(pd, mode.display_mode, plane as u32).unwrap();
                        println!("plane {plane}: src {:?}..{:?}; dst {:?}..{:?}", caps.min_src_extent, caps.max_src_extent, caps.min_dst_extent, caps.max_dst_extent);
                        for (width, height) in [(1280, 720), (640, 360)] {
                            let create = vk::DisplaySurfaceCreateInfoKHR::default()
                                .display_mode(mode.display_mode).plane_index(plane as u32)
                                .plane_stack_index(property.current_stack_index)
                                .transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                                .alpha_mode(vk::DisplayPlaneAlphaFlagsKHR::OPAQUE)
                                .image_extent(vk::Extent2D { width, height });
                            match display.create_display_plane_surface(&create, None) {
                                Ok(s) => {
                                    println!("surface {width}x{height}: {:?}", surface.get_physical_device_surface_capabilities(pd, s));
                                    println!("formats {:?}", surface.get_physical_device_surface_formats(pd, s));
                                    surface.destroy_surface(s, None);
                                }
                                Err(e) => println!("surface {width}x{height}: {e:?}"),
                            }
                        }
                    }
                }
            }
        }
        instance.destroy_instance(None);
    }
}
