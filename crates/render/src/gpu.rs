//! The Vulkan instance, device and memory allocator, shared by every GPU resource.

use std::ffi::CStr;
use std::sync::{Arc, Mutex};

use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<vk::Result> for Error {
    fn from(r: vk::Result) -> Self {
        Self(format!("Vulkan: {r:?}"))
    }
}

impl From<gpu_allocator::AllocationError> for Error {
    fn from(e: gpu_allocator::AllocationError) -> Self {
        Self(format!("GPU memory: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What kind of device was picked, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeviceKind {
    Discrete,
    Integrated,
    Virtual,
    Cpu,
    Other,
}

pub struct Gpu {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    /// Locked for every submit and present: a loading thread uploads while the window
    /// presents, and Vulkan requires one thread at a time on a queue.
    pub queue: Mutex<vk::Queue>,
    pub queue_family: u32,
    pub name: String,
    pub kind: DeviceKind,
    pub allocator: Mutex<Option<Allocator>>,
    /// For [`Gpu::submit_and_wait`], which holds it while recording.
    command_pool: Mutex<vk::CommandPool>,
}

impl Gpu {
    /// A device for rendering without a window (offscreen). Requires Vulkan 1.3 with
    /// dynamic rendering and synchronization2, and partially bound descriptors, which
    /// every target (Intel HD 600 series and newer, Steam Deck, lavapipe) has. `SWJKJA_GPU` picks a device whose
    /// name contains the given text.
    pub fn new_headless() -> Result<Arc<Self>> {
        Ok(Self::create(None)?.0)
    }

    /// A device that can also present to a window, and the window's surface (for
    /// [`crate::Swapchain`], which destroys it). Only devices that can present to the
    /// surface from their graphics queue are considered.
    pub fn new_for_window(
        display: RawDisplayHandle,
        window: RawWindowHandle,
    ) -> Result<(Arc<Self>, vk::SurfaceKHR)> {
        let (gpu, surface) = Self::create(Some((display, window)))?;
        Ok((gpu, surface.expect("created with a window")))
    }

    fn create(
        window: Option<(RawDisplayHandle, RawWindowHandle)>,
    ) -> Result<(Arc<Self>, Option<vk::SurfaceKHR>)> {
        let entry =
            unsafe { ash::Entry::load() }.map_err(|e| Error(format!("no Vulkan loader: {e}")))?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"swjkja")
            .engine_name(c"swjkja")
            .api_version(vk::API_VERSION_1_3);
        let extensions = match window {
            Some((display, _)) => ash_window::enumerate_required_extensions(display)?,
            None => &[],
        };
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app)
                    .enabled_extension_names(extensions),
                None,
            )?
        };
        let surface = match window {
            Some((display, window)) => Some(
                match unsafe {
                    ash_window::create_surface(&entry, &instance, display, window, None)
                } {
                    Ok(s) => s,
                    Err(e) => {
                        unsafe { instance.destroy_instance(None) };
                        return Err(e.into());
                    }
                },
            ),
            None => None,
        };
        let surface_fn = ash::khr::surface::Instance::new(&entry, &instance);

        let wanted = std::env::var("SWJKJA_GPU")
            .ok()
            .map(|s| s.to_ascii_lowercase());
        let mut candidates = Vec::new();
        for physical in unsafe { instance.enumerate_physical_devices()? } {
            let props = unsafe { instance.get_physical_device_properties(physical) };
            let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            // The texture array needs this many images per stage; drivers allow far
            // more in update-after-bind sets (the Intel HD 630: 200 normally, 1048576
            // update-after-bind).
            let mut p12 = vk::PhysicalDeviceVulkan12Properties::default();
            let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut p12);
            unsafe { instance.get_physical_device_properties2(physical, &mut props2) };
            if p12.max_per_stage_descriptor_update_after_bind_sampled_images
                < crate::texture::MAX_TEXTURES
            {
                continue;
            }
            if props.api_version < vk::API_VERSION_1_3
                || wanted
                    .as_ref()
                    .is_some_and(|w| !name.to_ascii_lowercase().contains(w))
            {
                continue;
            }
            let families =
                unsafe { instance.get_physical_device_queue_family_properties(physical) };
            let Some(family) = families.iter().enumerate().position(|(i, f)| {
                f.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                    && surface.is_none_or(|s| unsafe {
                        surface_fn
                            .get_physical_device_surface_support(physical, i as u32, s)
                            .unwrap_or(false)
                    })
            }) else {
                continue;
            };
            let kind = match props.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => DeviceKind::Discrete,
                vk::PhysicalDeviceType::INTEGRATED_GPU => DeviceKind::Integrated,
                vk::PhysicalDeviceType::VIRTUAL_GPU => DeviceKind::Virtual,
                vk::PhysicalDeviceType::CPU => DeviceKind::Cpu,
                _ => DeviceKind::Other,
            };
            candidates.push((kind, physical, family as u32, name));
        }
        candidates.sort_by_key(|c| c.0);
        let Some((kind, physical, queue_family, name)) = candidates.into_iter().next() else {
            unsafe {
                if let Some(s) = surface {
                    surface_fn.destroy_surface(s, None);
                }
                instance.destroy_instance(None);
            }
            return Err(Error(
                "no Vulkan 1.3 device with a graphics queue and update-after-bind texture arrays"
                    .to_string()
                    + if surface.is_some() {
                        " that can present to the window"
                    } else {
                        ""
                    },
            ));
        };

        let priorities = [1.0];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&priorities)];
        let mut features13 = vk::PhysicalDeviceVulkan13Features::default()
            .dynamic_rendering(true)
            .synchronization2(true);
        // One big texture array, indexed by a push constant; unused slots stay empty.
        let mut features12 = vk::PhysicalDeviceVulkan12Features::default()
            .descriptor_binding_partially_bound(true)
            .descriptor_binding_sampled_image_update_after_bind(true);
        let device_extensions = match surface {
            Some(_) => &[ash::khr::swapchain::NAME.as_ptr()][..],
            None => &[],
        };
        let device = unsafe {
            instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queue_info)
                    .enabled_extension_names(device_extensions)
                    .push_next(&mut features13)
                    .push_next(&mut features12)
                    .enabled_features(
                        &vk::PhysicalDeviceFeatures::default().sampler_anisotropy(true),
                    ),
                None,
            )?
        };
        let queue = unsafe { device.get_device_queue(queue_family, 0) };
        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device: physical,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })?;
        let command_pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?
        };
        let gpu = Arc::new(Self {
            entry,
            instance,
            physical,
            device,
            queue: Mutex::new(queue),
            queue_family,
            name,
            kind,
            allocator: Mutex::new(Some(allocator)),
            command_pool: Mutex::new(command_pool),
        });
        Ok((gpu, surface))
    }

    /// Waits until the GPU is idle (`vkDeviceWaitIdle`, which needs the queue to
    /// itself).
    pub fn wait_idle(&self) {
        let _queue = self.queue.lock().unwrap();
        let _ = unsafe { self.device.device_wait_idle() };
    }

    /// Records commands with `record`, submits them and waits until the GPU is done.
    pub fn submit_and_wait(&self, record: impl FnOnce(vk::CommandBuffer)) -> Result<()> {
        let d = &self.device;
        let pool = self.command_pool.lock().unwrap();
        unsafe {
            let cmd = d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(*pool)
                    .command_buffer_count(1),
            )?[0];
            d.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            record(cmd);
            d.end_command_buffer(cmd)?;
            let fence = d.create_fence(&vk::FenceCreateInfo::default(), None)?;
            let cmds = [cmd];
            d.queue_submit(
                *self.queue.lock().unwrap(),
                &[vk::SubmitInfo::default().command_buffers(&cmds)],
                fence,
            )?;
            let waited = d.wait_for_fences(&[fence], true, u64::MAX);
            d.destroy_fence(fence, None);
            d.free_command_buffers(*pool, &cmds);
            waited?;
        }
        Ok(())
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            // The allocator must go before the device it allocates from.
            self.allocator.lock().unwrap().take();
            self.device
                .destroy_command_pool(*self.command_pool.get_mut().unwrap(), None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}
