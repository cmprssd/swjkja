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
        Self::create(gpu, extent, format, 1, usage, aspect)
    }

    /// A colour image with `mip_levels` levels (the view covers all of them).
    pub fn new_mipmapped(
        gpu: &Arc<Gpu>,
        extent: vk::Extent2D,
        format: vk::Format,
        mip_levels: u32,
        usage: vk::ImageUsageFlags,
    ) -> Result<Self> {
        Self::create(
            gpu,
            extent,
            format,
            mip_levels,
            usage,
            vk::ImageAspectFlags::COLOR,
        )
    }

    fn create(
        gpu: &Arc<Gpu>,
        extent: vk::Extent2D,
        format: vk::Format,
        mip_levels: u32,
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
                    .mip_levels(mip_levels)
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
                    .subresource_range(vk::ImageSubresourceRange {
                        level_count: mip_levels,
                        ..subresource(aspect)
                    }),
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

/// An offscreen colour (RGBA8) and depth target that can be read back to the CPU,
/// plus a sampleable copy of the depth (for flares' occlusion test).
pub struct Target {
    pub color: Image,
    pub depth: Image,
    /// Depth as it was at the last [`Frame::copy_depth`].
    pub depth_copy: Image,
}

/// One frame being recorded into a [`Target`].
pub struct Frame<'a> {
    pub cmd: vk::CommandBuffer,
    /// Which copy of per-frame data this frame may write (below
    /// [`crate::FRAMES_IN_FLIGHT`]): the GPU may still be reading the others.
    pub slot: usize,
    gpu: &'a Gpu,
    target: &'a Target,
}

impl Frame<'_> {
    /// Pauses rendering, copies the depth buffer to [`Target::depth_copy`] and resumes
    /// with the colour and depth so far, like the original reading the depth buffer
    /// in the middle of a frame.
    pub fn copy_depth(&mut self) {
        let (gpu, t, cmd) = (self.gpu, self.target, self.cmd);
        let depth = vk::ImageAspectFlags::DEPTH;
        unsafe {
            gpu.device.cmd_end_rendering(cmd);
            transition(
                gpu,
                cmd,
                t.depth.image,
                depth,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            );
            transition(
                gpu,
                cmd,
                t.depth_copy.image,
                depth,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            let layers = vk::ImageSubresourceLayers {
                aspect_mask: depth,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };
            gpu.device.cmd_copy_image(
                cmd,
                t.depth.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                t.depth_copy.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy {
                    src_subresource: layers,
                    dst_subresource: layers,
                    extent: vk::Extent3D {
                        width: t.depth.extent.width,
                        height: t.depth.extent.height,
                        depth: 1,
                    },
                    ..Default::default()
                }],
            );
            transition(
                gpu,
                cmd,
                t.depth_copy.image,
                depth,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
            transition(
                gpu,
                cmd,
                t.depth.image,
                depth,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
            );
            t.begin(gpu, cmd, None);
        }
    }
}

impl Target {
    pub const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
    pub const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

    pub fn new(gpu: &Arc<Gpu>, width: u32, height: u32) -> Result<Self> {
        let extent = vk::Extent2D { width, height };
        let target = Self {
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
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                vk::ImageAspectFlags::DEPTH,
            )?,
            depth_copy: Image::new(
                gpu,
                extent,
                Self::DEPTH_FORMAT,
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                vk::ImageAspectFlags::DEPTH,
            )?,
        };
        // The copy starts out at the far plane (depth 0) and stays readable.
        gpu.submit_and_wait(|cmd| unsafe {
            let depth = vk::ImageAspectFlags::DEPTH;
            transition(
                gpu,
                cmd,
                target.depth_copy.image,
                depth,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            gpu.device.cmd_clear_depth_stencil_image(
                cmd,
                target.depth_copy.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearDepthStencilValue {
                    depth: 0.0,
                    stencil: 0,
                },
                &[subresource(depth)],
            );
            transition(
                gpu,
                cmd,
                target.depth_copy.image,
                depth,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        })?;
        Ok(target)
    }

    /// Begins rendering into the target: clearing to `clear` (and depth to 0) if
    /// given, else keeping what is there.
    fn begin(&self, gpu: &Gpu, cmd: vk::CommandBuffer, clear: Option<[f32; 4]>) {
        let extent = self.color.extent;
        let load = if clear.is_some() {
            vk::AttachmentLoadOp::CLEAR
        } else {
            vk::AttachmentLoadOp::LOAD
        };
        let color = [vk::RenderingAttachmentInfo::default()
            .image_view(self.color.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(load)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: clear.unwrap_or_default(),
                },
            })];
        let depth = vk::RenderingAttachmentInfo::default()
            .image_view(self.depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(load)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 0.0,
                    stencil: 0,
                },
            });
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        };
        unsafe {
            gpu.device.cmd_begin_rendering(
                cmd,
                &vk::RenderingInfo::default()
                    .render_area(area)
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
            gpu.device.cmd_set_scissor(cmd, 0, &[area]);
        }
    }

    /// Records one frame: clears to `clear` (and depth to 0), runs `draw` with per-frame
    /// `slot`, and leaves the colour ready to read or copy. Timestamps go to queries
    /// `first` and `first + 1` of `queries`, which must have been reset.
    pub(crate) fn record(
        &self,
        gpu: &Gpu,
        cmd: vk::CommandBuffer,
        clear: [f32; 4],
        slot: usize,
        (queries, first): (vk::QueryPool, u32),
        draw: impl FnOnce(&mut Frame),
    ) {
        let d = &gpu.device;
        unsafe {
            d.cmd_write_timestamp2(cmd, vk::PipelineStageFlags2::TOP_OF_PIPE, queries, first);
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
            self.begin(gpu, cmd, Some(clear));
            draw(&mut Frame {
                cmd,
                slot,
                gpu,
                target: self,
            });
            d.cmd_end_rendering(cmd);
            transition(
                gpu,
                cmd,
                self.color.image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            );
            d.cmd_write_timestamp2(
                cmd,
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                queries,
                first + 1,
            );
        }
    }

    /// Renders into the target: clears it to `clear` (RGBA, 0-1) and depth 0 (depth is
    /// reversed: 1 is nearest, 0 infinitely far), runs `draw`, and leaves the colour
    /// ready to read back.
    pub fn render(&self, gpu: &Gpu, clear: [f32; 4], draw: impl FnOnce(&mut Frame)) -> Result<()> {
        self.render_timed(gpu, clear, draw).map(|_| ())
    }

    /// [`Target::render`], returning the GPU's time for the frame in milliseconds,
    /// from timestamps around everything recorded.
    pub fn render_timed(
        &self,
        gpu: &Gpu,
        clear: [f32; 4],
        draw: impl FnOnce(&mut Frame),
    ) -> Result<f64> {
        let d = &gpu.device;
        let pool = unsafe {
            d.create_query_pool(
                &vk::QueryPoolCreateInfo::default()
                    .query_type(vk::QueryType::TIMESTAMP)
                    .query_count(2),
                None,
            )?
        };
        let result = gpu.submit_and_wait(|cmd| unsafe {
            d.cmd_reset_query_pool(cmd, pool, 0, 2);
            self.record(gpu, cmd, clear, 0, (pool, 0), draw);
        });
        let mut stamps = [0u64; 2];
        let read = unsafe {
            d.get_query_pool_results(
                pool,
                0,
                &mut stamps,
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            )
        };
        unsafe { d.destroy_query_pool(pool, None) };
        result?;
        read?;
        let period = unsafe { gpu.instance.get_physical_device_properties(gpu.physical) }
            .limits
            .timestamp_period;
        Ok(stamps[1].wrapping_sub(stamps[0]) as f64 * f64::from(period) / 1e6)
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
