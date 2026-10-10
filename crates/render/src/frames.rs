//! A frame loop with [`FRAMES_IN_FLIGHT`] frames in flight: the CPU records the next
//! frame while the GPU draws the previous one, and waits only for the frame that
//! last used the same slot. Each slot has its own command buffer, fence and
//! timestamps; per-frame data (uniforms, deformed geometry) is kept per slot by its
//! owner and chosen with [`crate::Frame::slot`].

use std::sync::Arc;

use ash::vk;

use crate::resource::transition;
use crate::{Gpu, Result, Swapchain, Target};

pub const FRAMES_IN_FLIGHT: usize = 2;

struct Slot {
    cmd: vk::CommandBuffer,
    /// Signalled when the slot's last frame is done (created signalled).
    fence: vk::Fence,
    /// Two timestamps around the slot's last frame.
    queries: vk::QueryPool,
    /// Signalled when the swapchain image for the slot's frame is ready to draw to.
    acquired: vk::Semaphore,
    /// Whether the fence and timestamps belong to a submitted frame not yet collected.
    pending: bool,
}

pub struct FrameLoop {
    gpu: Arc<Gpu>,
    pool: vk::CommandPool,
    slots: Vec<Slot>,
    next: usize,
    /// Nanoseconds per timestamp tick.
    period: f64,
}

impl FrameLoop {
    pub fn new(gpu: &Arc<Gpu>) -> Result<Self> {
        let d = &gpu.device;
        let pool = unsafe {
            d.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(gpu.queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?
        };
        let mut frames = Self {
            gpu: gpu.clone(),
            pool,
            slots: Vec::new(),
            next: 0,
            period: f64::from(
                unsafe { gpu.instance.get_physical_device_properties(gpu.physical) }
                    .limits
                    .timestamp_period,
            ),
        };
        let cmds = unsafe {
            d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .command_buffer_count(FRAMES_IN_FLIGHT as u32),
            )?
        };
        for cmd in cmds {
            // Pushed one by one, so Drop frees whatever was created before an error.
            let fence = unsafe {
                d.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )?
            };
            let queries = unsafe {
                d.create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(vk::QueryType::TIMESTAMP)
                        .query_count(2),
                    None,
                )
            };
            let queries = match queries {
                Ok(q) => q,
                Err(e) => {
                    unsafe { d.destroy_fence(fence, None) };
                    return Err(e.into());
                }
            };
            let acquired =
                match unsafe { d.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) } {
                    Ok(s) => s,
                    Err(e) => {
                        unsafe {
                            d.destroy_fence(fence, None);
                            d.destroy_query_pool(queries, None);
                        }
                        return Err(e.into());
                    }
                };
            frames.slots.push(Slot {
                cmd,
                fence,
                queries,
                acquired,
                pending: false,
            });
        }
        Ok(frames)
    }

    /// Records a frame into `target` with `draw` and submits it without waiting for
    /// it. First waits for the frame that last used this slot, [`FRAMES_IN_FLIGHT`]
    /// frames ago, and returns that frame's GPU time in ms (`None` for the first
    /// frames).
    pub fn render(
        &mut self,
        target: &Target,
        clear: [f32; 4],
        draw: impl FnOnce(&mut crate::Frame),
    ) -> Result<Option<f64>> {
        let slot = self.next;
        self.next = (slot + 1) % FRAMES_IN_FLIGHT;
        let gpu_ms = self.collect(slot)?;
        self.submit(slot, target, clear, draw, None)?;
        Ok(gpu_ms)
    }

    /// [`FrameLoop::render`], then copies the target to the window's next image and
    /// presents it. The target should have the swapchain's size (it is scaled if
    /// not). `out_of_date` means the swapchain no longer matches the window: wait
    /// with [`FrameLoop::finish`], then [`Swapchain::resize`]. The frame may then not
    /// have been drawn.
    pub fn present(
        &mut self,
        swapchain: &Swapchain,
        target: &Target,
        clear: [f32; 4],
        draw: impl FnOnce(&mut crate::Frame),
    ) -> Result<Presented> {
        let slot = self.next;
        self.next = (slot + 1) % FRAMES_IN_FLIGHT;
        let gpu_ms = self.collect(slot)?;
        let acquired = unsafe {
            swapchain.swapchain_fn.acquire_next_image(
                swapchain.swapchain,
                u64::MAX,
                self.slots[slot].acquired,
                vk::Fence::null(),
            )
        };
        let (image, suboptimal) = match acquired {
            Ok(a) => a,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                return Ok(Presented {
                    gpu_ms,
                    out_of_date: true,
                });
            }
            Err(e) => return Err(e.into()),
        };
        self.submit(slot, target, clear, draw, Some((swapchain, image)))?;
        let waits = [swapchain.rendered[image as usize]];
        let swapchains = [swapchain.swapchain];
        let images = [image];
        let presented = unsafe {
            swapchain.swapchain_fn.queue_present(
                self.gpu.queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&waits)
                    .swapchains(&swapchains)
                    .image_indices(&images),
            )
        };
        let out_of_date = match presented {
            Ok(suboptimal_now) => suboptimal || suboptimal_now,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => true,
            Err(e) => return Err(e.into()),
        };
        Ok(Presented {
            gpu_ms,
            out_of_date,
        })
    }

    /// Records the slot's frame and submits it; with a swapchain image, also copies
    /// the target to it (waiting for the image, and signalling it rendered).
    fn submit(
        &mut self,
        slot: usize,
        target: &Target,
        clear: [f32; 4],
        draw: impl FnOnce(&mut crate::Frame),
        present: Option<(&Swapchain, u32)>,
    ) -> Result<()> {
        let (gpu, d) = (&self.gpu, &self.gpu.device);
        let s = &mut self.slots[slot];
        unsafe {
            d.reset_fences(&[s.fence])?;
            d.begin_command_buffer(
                s.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            d.cmd_reset_query_pool(s.cmd, s.queries, 0, 2);
            target.record(gpu, s.cmd, clear, slot, (s.queries, 0), draw);
            if let Some((swapchain, image)) = present {
                blit_to(
                    gpu,
                    s.cmd,
                    target,
                    swapchain.images[image as usize],
                    swapchain.extent,
                );
            }
            d.end_command_buffer(s.cmd)?;
            let cmds = [s.cmd];
            let (waits, signals) = match present {
                Some((swapchain, image)) => {
                    (vec![s.acquired], vec![swapchain.rendered[image as usize]])
                }
                None => (Vec::new(), Vec::new()),
            };
            let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
            d.queue_submit(
                gpu.queue,
                &[vk::SubmitInfo::default()
                    .wait_semaphores(&waits)
                    .wait_dst_stage_mask(&stages)
                    .command_buffers(&cmds)
                    .signal_semaphores(&signals)],
                s.fence,
            )?;
        }
        s.pending = true;
        Ok(())
    }

    /// Waits for every frame in flight and returns their GPU times in ms, oldest
    /// first. The target can be read back afterwards.
    pub fn finish(&mut self) -> Result<Vec<f64>> {
        let mut times = Vec::new();
        for i in 0..FRAMES_IN_FLIGHT {
            let slot = (self.next + i) % FRAMES_IN_FLIGHT;
            times.extend(self.collect(slot)?);
        }
        Ok(times)
    }

    /// Waits for the slot's last frame and reads its GPU time, if one is pending.
    fn collect(&mut self, slot: usize) -> Result<Option<f64>> {
        let d = &self.gpu.device;
        let s = &mut self.slots[slot];
        unsafe { d.wait_for_fences(&[s.fence], true, u64::MAX)? };
        if !std::mem::take(&mut s.pending) {
            return Ok(None);
        }
        let mut stamps = [0u64; 2];
        unsafe {
            d.get_query_pool_results(
                s.queries,
                0,
                &mut stamps,
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            )?
        };
        Ok(Some(
            stamps[1].wrapping_sub(stamps[0]) as f64 * self.period / 1e6,
        ))
    }
}

impl Drop for FrameLoop {
    fn drop(&mut self) {
        let d = &self.gpu.device;
        unsafe {
            for s in &self.slots {
                let _ = d.wait_for_fences(&[s.fence], true, u64::MAX);
                d.destroy_fence(s.fence, None);
                d.destroy_query_pool(s.queries, None);
                d.destroy_semaphore(s.acquired, None);
            }
            // Freed with the pool.
            d.destroy_command_pool(self.pool, None);
        }
    }
}

/// What [`FrameLoop::present`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Presented {
    /// The GPU time of the frame [`FRAMES_IN_FLIGHT`] frames ago, in ms.
    pub gpu_ms: Option<f64>,
    /// The swapchain must be resized before the next frame.
    pub out_of_date: bool,
}

/// Copies the target's colour (in `TRANSFER_SRC_OPTIMAL`, as [`Target::record`]
/// leaves it) to a swapchain image and makes the image ready to present.
fn blit_to(gpu: &Gpu, cmd: vk::CommandBuffer, target: &Target, image: vk::Image, to: vk::Extent2D) {
    let color = vk::ImageAspectFlags::COLOR;
    transition(
        gpu,
        cmd,
        image,
        color,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
    );
    let from = target.color.extent;
    let corner = |e: vk::Extent2D| vk::Offset3D {
        x: e.width as i32,
        y: e.height as i32,
        z: 1,
    };
    let layers = vk::ImageSubresourceLayers {
        aspect_mask: color,
        mip_level: 0,
        base_array_layer: 0,
        layer_count: 1,
    };
    unsafe {
        gpu.device.cmd_blit_image(
            cmd,
            target.color.image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[vk::ImageBlit {
                src_subresource: layers,
                src_offsets: [vk::Offset3D::default(), corner(from)],
                dst_subresource: layers,
                dst_offsets: [vk::Offset3D::default(), corner(to)],
            }],
            if from == to {
                vk::Filter::NEAREST
            } else {
                vk::Filter::LINEAR
            },
        );
    }
    transition(
        gpu,
        cmd,
        image,
        color,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        vk::ImageLayout::PRESENT_SRC_KHR,
    );
}
