//! The Vulkan instance, device and memory allocator, shared by every GPU resource.

use std::ffi::CStr;
use std::sync::{Arc, Mutex};

use ash::vk;
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};

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
    pub queue: vk::Queue,
    pub queue_family: u32,
    pub name: String,
    pub kind: DeviceKind,
    pub allocator: Mutex<Option<Allocator>>,
    command_pool: vk::CommandPool,
}

impl Gpu {
    /// A device for rendering without a window (offscreen). Requires Vulkan 1.3 with
    /// dynamic rendering and synchronization2, and partially bound descriptors, which
    /// every target (Intel HD 600 series and newer, Steam Deck, lavapipe) has. `SWJKJA_GPU` picks a device whose
    /// name contains the given text.
    pub fn new_headless() -> Result<Arc<Self>> {
        let entry =
            unsafe { ash::Entry::load() }.map_err(|e| Error(format!("no Vulkan loader: {e}")))?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"swjkja")
            .engine_name(c"swjkja")
            .api_version(vk::API_VERSION_1_3);
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app),
                None,
            )?
        };

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
            let Some(family) = families
                .iter()
                .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            else {
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
            unsafe { instance.destroy_instance(None) };
            return Err(Error(
                "no Vulkan 1.3 device with a graphics queue and update-after-bind texture arrays"
                    .into(),
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
        let device = unsafe {
            instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queue_info)
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
        Ok(Arc::new(Self {
            entry,
            instance,
            physical,
            device,
            queue,
            queue_family,
            name,
            kind,
            allocator: Mutex::new(Some(allocator)),
            command_pool,
        }))
    }

    /// Records commands with `record`, submits them and waits until the GPU is done.
    pub fn submit_and_wait(&self, record: impl FnOnce(vk::CommandBuffer)) -> Result<()> {
        let d = &self.device;
        unsafe {
            let cmd = d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.command_pool)
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
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&cmds)],
                fence,
            )?;
            let waited = d.wait_for_fences(&[fence], true, u64::MAX);
            d.destroy_fence(fence, None);
            d.free_command_buffers(self.command_pool, &cmds);
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
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}
