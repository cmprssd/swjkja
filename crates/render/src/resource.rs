//! GPU buffers and the offscreen render target. Each owns its memory and frees it when
//! dropped.

use std::sync::Arc;

use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme};

use crate::gpu::{Error, Gpu, Result};

pub struct Buffer {
    gpu: Arc<Gpu>,
    pub buffer: vk::Buffer,
    pub size: u64,
    allocation: Option<Allocation>,
}

impl Buffer {
    /// A buffer the CPU can write and the GPU can read, filled with `data`.
    pub fn with_data(gpu: &Arc<Gpu>, usage: vk::BufferUsageFlags, data: &[u8]) -> Result<Self> {
        let mut b = Self::new(gpu, usage, data.len() as u64, MemoryLocation::CpuToGpu)?;
        b.mapped_mut()?[..data.len()].copy_from_slice(data);
        Ok(b)
    }

    pub fn new(
        gpu: &Arc<Gpu>,
        usage: vk::BufferUsageFlags,
        size: u64,
        location: MemoryLocation,
    ) -> Result<Self> {
        let d = &gpu.device;
        let buffer = unsafe {
            d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size.max(1))
                    .usage(usage),
                None,
            )?
        };
        let requirements = unsafe { d.get_buffer_memory_requirements(buffer) };
        let allocation =
            gpu.allocator
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .allocate(&AllocationCreateDesc {
                    name: "buffer",
                    requirements,
                    location,
                    linear: true,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })?;
        unsafe { d.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())? };
        Ok(Self {
            gpu: gpu.clone(),
            buffer,
            size,
            allocation: Some(allocation),
        })
    }

    pub fn mapped(&self) -> Result<&[u8]> {
        let a = self.allocation.as_ref().unwrap();
        a.mapped_slice()
            .map(|s| &s[..self.size as usize])
            .ok_or_else(|| Error("buffer is not CPU-visible".into()))
    }

    pub fn mapped_mut(&mut self) -> Result<&mut [u8]> {
        let size = self.size as usize;
        let a = self.allocation.as_mut().unwrap();
        a.mapped_slice_mut()
            .map(|s| &mut s[..size])
            .ok_or_else(|| Error("buffer is not CPU-visible".into()))
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe { self.gpu.device.destroy_buffer(self.buffer, None) };
        if let Some(a) = self.allocation.take() {
            let _ = self.gpu.allocator.lock().unwrap().as_mut().unwrap().free(a);
        }
    }
}

pub struct Image {
    gpu: Arc<Gpu>,
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    allocation: Option<Allocation>,
}

impl Image {
    pub fn new(
        gpu: &Arc<Gpu>,
        extent: vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<Self> {
        let d = &gpu.device;
        let image = unsafe {
            d.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(usage),
                None,
            )?
        };
        let requirements = unsafe { d.get_image_memory_requirements(image) };
        let allocation =
            gpu.allocator
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .allocate(&AllocationCreateDesc {
                    name: "image",
                    requirements,
                    location: MemoryLocation::GpuOnly,
                    linear: false,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })?;
        unsafe { d.bind_image_memory(image, allocation.memory(), allocation.offset())? };
        let view = unsafe {
            d.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(subresource(aspect)),
                None,
            )?
        };
        Ok(Self {
            gpu: gpu.clone(),
            image,
            view,
            format,
            extent,
            allocation: Some(allocation),
        })
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            self.gpu.device.destroy_image_view(self.view, None);
            self.gpu.device.destroy_image(self.image, None);
        }
        if let Some(a) = self.allocation.take() {
            let _ = self.gpu.allocator.lock().unwrap().as_mut().unwrap().free(a);
        }
    }
}

pub fn subresource(aspect: vk::ImageAspectFlags) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: aspect,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// An image layout transition with a full barrier (fine for setup and readback).
pub fn transition(
    gpu: &Gpu,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    aspect: vk::ImageAspectFlags,
    from: vk::ImageLayout,
    to: vk::ImageLayout,
) {
    let barrier = [vk::ImageMemoryBarrier2::default()
        .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
        .src_access_mask(vk::AccessFlags2::MEMORY_WRITE)
        .dst_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
        .dst_access_mask(vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE)
        .old_layout(from)
        .new_layout(to)
        .image(image)
        .subresource_range(subresource(aspect))];
    unsafe {
        gpu.device.cmd_pipeline_barrier2(
            cmd,
            &vk::DependencyInfo::default().image_memory_barriers(&barrier),
        )
    };
}

/// An offscreen colour (RGBA8) and depth target that can be read back to the CPU.
pub struct Target {
    pub color: Image,
    pub depth: Image,
}

impl Target {
    pub const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
    pub const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

    pub fn new(gpu: &Arc<Gpu>, width: u32, height: u32) -> Result<Self> {
        let extent = vk::Extent2D { width, height };
        Ok(Self {
            color: Image::new(
                gpu,
                extent,
                Self::COLOR_FORMAT,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                vk::ImageAspectFlags::COLOR,
            )?,
            depth: Image::new(
                gpu,
                extent,
                Self::DEPTH_FORMAT,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
            )?,
        })
    }

    /// Renders into the target: clears it to `clear` (RGBA, 0-1) and depth 1, runs
    /// `draw` inside dynamic rendering, and leaves the colour ready to read back.
    pub fn render(
        &self,
        gpu: &Gpu,
        clear: [f32; 4],
        draw: impl FnOnce(vk::CommandBuffer),
    ) -> Result<()> {
        let extent = self.color.extent;
        gpu.submit_and_wait(|cmd| unsafe {
            transition(
                gpu,
                cmd,
                self.color.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            );
            transition(
                gpu,
                cmd,
                self.depth.image,
                vk::ImageAspectFlags::DEPTH,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
            );
            let color = [vk::RenderingAttachmentInfo::default()
                .image_view(self.color.view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue { float32: clear },
                })];
            let depth = vk::RenderingAttachmentInfo::default()
                .image_view(self.depth.view)
                .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                .clear_value(vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                });
            gpu.device.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D::default(),
                        extent,
                    })
                    .layer_count(1)
                    .color_attachments(&color)
                    .depth_attachment(&depth),
            );
            let viewport = [vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: extent.width as f32,
                height: extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }];
            gpu.device.cmd_set_viewport(cmd, 0, &viewport);
            gpu.device.cmd_set_scissor(
                cmd,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                }],
            );
            draw(cmd);
            gpu.device.cmd_end_rendering(cmd);
            transition(
                gpu,
                cmd,
                self.color.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            );
        })
    }

    /// Copies the colour image (after [`Target::render`]) to the CPU: RGBA8, top row
    /// first.
    pub fn read_rgba(&self, gpu: &Arc<Gpu>) -> Result<Vec<u8>> {
        let extent = self.color.extent;
        let size = u64::from(extent.width) * u64::from(extent.height) * 4;
        let staging = Buffer::new(
            gpu,
            vk::BufferUsageFlags::TRANSFER_DST,
            size,
            MemoryLocation::GpuToCpu,
        )?;
        gpu.submit_and_wait(|cmd| unsafe {
            gpu.device.cmd_copy_image_to_buffer(
                cmd,
                self.color.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                staging.buffer,
                &[vk::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    },
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    },
                }],
            )
        })?;
        Ok(staging.mapped()?.to_vec())
    }
}
