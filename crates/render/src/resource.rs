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

/// An offscreen colour (10-bit RGB, [`Target::COLOR_FORMAT`]) and depth target
/// that can be read back to the CPU, plus a sampleable copy of the depth (for
/// flares' occlusion test).
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
    /// The colour image drawn to: the target's own, or a swapchain image.
    color: vk::ImageView,
}

impl Frame<'_> {
    /// Fills a rectangle (x, y, width, height in pixels) with a colour, without a
    /// pipeline: for the loading screen's progress bar.
    pub fn fill(&mut self, rect: [u32; 4], color: [f32; 4]) {
        let [x, y, width, height] = rect;
        if width == 0 || height == 0 {
            return; // Vulkan forbids empty rectangles.
        }
        unsafe {
            self.gpu.device.cmd_clear_attachments(
                self.cmd,
                &[vk::ClearAttachment {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    color_attachment: 0,
                    clear_value: vk::ClearValue {
                        color: vk::ClearColorValue { float32: color },
                    },
                }],
                &[vk::ClearRect {
                    rect: vk::Rect2D {
                        offset: vk::Offset2D {
                            x: x as i32,
                            y: y as i32,
                        },
                        extent: vk::Extent2D { width, height },
                    },
                    base_array_layer: 0,
                    layer_count: 1,
                }],
            )
        };
    }

    /// Clears the depth to the far plane (0), keeping the colour: a second scene
    /// drawn over the first (the view over its sky portal's).
    pub fn clear_depth(&mut self) {
        let extent = self.target.depth.extent;
        unsafe {
            self.gpu.device.cmd_clear_attachments(
                self.cmd,
                &[vk::ClearAttachment {
                    aspect_mask: vk::ImageAspectFlags::DEPTH,
                    color_attachment: 0,
                    clear_value: vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue {
                            depth: 0.0,
                            stencil: 0,
                        },
                    },
                }],
                &[vk::ClearRect {
                    rect: vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                            width: extent.width,
                            height: extent.height,
                        },
                    },
                    base_array_layer: 0,
                    layer_count: 1,
                }],
            )
        }
    }

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
            t.begin(gpu, cmd, self.color, None, false);
        }
    }
}

impl Target {
    /// 10 bits a channel (and 2 of alpha), as the original's framebuffer on the
    /// reference PC: its Intel OpenGL driver reports 8-bit colour but blends in 10,
    /// so each blend rounds to a 1023rd and its screenshots round that to 8 bits.
    /// With 8-bit blending, additive layers and fog round differently (half the
    /// pixels a level off on the academy classes' light shafts).
    pub const COLOR_FORMAT: vk::Format = vk::Format::A2B10G10R10_UNORM_PACK32;
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
        // The copy starts out at the far plane (depth 0) and stays readable; the colour
        // starts black, in the layout every frame leaves it in.
        gpu.submit_and_wait(|cmd| unsafe {
            let color = vk::ImageAspectFlags::COLOR;
            transition(
                gpu,
                cmd,
                target.color.image,
                color,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            gpu.device.cmd_clear_color_image(
                cmd,
                target.color.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue {
                    float32: [0.0, 0.0, 0.0, 1.0],
                },
                &[subresource(color)],
            );
            transition(
                gpu,
                cmd,
                target.color.image,
                color,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            );
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

    /// Begins rendering into `color` (of the target's size and format) and the
    /// target's depth: the colour cleared to `clear` if given, else kept; the depth
    /// cleared to 0 with `clear_depth`, else kept.
    fn begin(
        &self,
        gpu: &Gpu,
        cmd: vk::CommandBuffer,
        color: vk::ImageView,
        clear: Option<[f32; 4]>,
        clear_depth: bool,
    ) {
        let extent = self.color.extent;
        let load = |clear: bool| {
            if clear {
                vk::AttachmentLoadOp::CLEAR
            } else {
                vk::AttachmentLoadOp::LOAD
            }
        };
        let color = [vk::RenderingAttachmentInfo::default()
            .image_view(color)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(load(clear.is_some()))
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: clear.unwrap_or_default(),
                },
            })];
        let depth = vk::RenderingAttachmentInfo::default()
            .image_view(self.depth.view)
            .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
            .load_op(load(clear_depth))
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

    /// Records one frame: clears the colour to `clear` if given, else keeps the last
    /// frame's (see [`crate::WorldRenderer::clear_color`]), clears the depth to 0,
    /// runs `draw` with per-frame `slot`, and leaves the colour ready to read or
    /// copy. With `window`, draws into that swapchain image (and view) instead of the
    /// target's colour and leaves it ready to present; the image holds the last frame
    /// drawn into it if its flag is set, else it is cleared to black. Timestamps go to
    /// queries `first` and `first + 1` of `queries`, which must have been reset.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &self,
        gpu: &Gpu,
        cmd: vk::CommandBuffer,
        clear: Option<[f32; 4]>,
        slot: usize,
        (queries, first): (vk::QueryPool, u32),
        window: Option<(vk::Image, vk::ImageView, bool)>,
        draw: impl FnOnce(&mut Frame),
    ) {
        let d = &gpu.device;
        let present = vk::ImageLayout::PRESENT_SRC_KHR;
        let (image, view, last, done) = match window {
            Some((image, view, true)) => (image, view, present, present),
            Some((image, view, false)) => (image, view, vk::ImageLayout::UNDEFINED, present),
            None => (
                self.color.image,
                self.color.view,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            ),
        };
        let clear = match last {
            vk::ImageLayout::UNDEFINED => Some(clear.unwrap_or([0.0, 0.0, 0.0, 1.0])),
            _ => clear,
        };
        unsafe {
            d.cmd_write_timestamp2(cmd, vk::PipelineStageFlags2::TOP_OF_PIPE, queries, first);
            transition(
                gpu,
                cmd,
                image,
                vk::ImageAspectFlags::COLOR,
                last,
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
            self.begin(gpu, cmd, view, clear, true);
            draw(&mut Frame {
                cmd,
                slot,
                gpu,
                target: self,
                color: view,
            });
            d.cmd_end_rendering(cmd);
            transition(
                gpu,
                cmd,
                image,
                vk::ImageAspectFlags::COLOR,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                done,
            );
            d.cmd_write_timestamp2(
                cmd,
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                queries,
                first + 1,
            );
        }
    }

    /// Renders into the target: clears its colour to `clear` (RGBA, 0-1) if given, else
    /// keeps the last frame's, and its depth to 0 (depth is reversed: 1 is nearest, 0
    /// infinitely far), runs `draw`, and leaves the colour ready to read back.
    pub fn render(
        &self,
        gpu: &Gpu,
        clear: Option<[f32; 4]>,
        draw: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        self.render_timed(gpu, clear, draw).map(|_| ())
    }

    /// [`Target::render`], returning the GPU's time for the frame in milliseconds,
    /// from timestamps around everything recorded.
    pub fn render_timed(
        &self,
        gpu: &Gpu,
        clear: Option<[f32; 4]>,
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
            self.record(gpu, cmd, clear, 0, (pool, 0), None, draw);
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
    /// first, each 10-bit channel rounded to 8 bits (as `glReadPixels` does) and
    /// alpha opaque.
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
        let mut rgba = staging.mapped()?.to_vec();
        for p in rgba.as_chunks_mut::<4>().0 {
            let packed = u32::from_le_bytes(*p);
            let channel = |shift: u32| (((packed >> shift) & 1023) * 255 + 511) / 1023;
            *p = [channel(0), channel(10), channel(20), 255].map(|c| c as u8);
        }
        Ok(rgba)
    }
}
