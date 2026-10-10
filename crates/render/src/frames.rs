//! A frame loop with [`FRAMES_IN_FLIGHT`] frames in flight: the CPU records the next
//! frame while the GPU draws the previous one, and waits only for the frame that
//! last used the same slot. Each slot has its own command buffer, fence and
//! timestamps; per-frame data (uniforms, deformed geometry) is kept per slot by its
//! owner and chosen with [`crate::Frame::slot`].

use std::sync::Arc;
use std::time::Instant;

use ash::vk;

use crate::resource::transition;
use crate::{Gpu, Result, Swapchain, Target};

pub const FRAMES_IN_FLIGHT: usize = 2;

struct Slot {
    cmd: vk::CommandBuffer,
    /// Signalled when the slot's last frame is done (created signalled).
    fence: vk::Fence,
    /// Timestamps of the slot's last frame: start, world drawn, and copied to the
    /// swapchain image if it was.
    queries: vk::QueryPool,
    /// Signalled when the swapchain image for the slot's frame is ready to draw to.
    acquired: vk::Semaphore,
    /// The submitted frame the fence and timestamps belong to, not yet collected.
    pending: Option<Kind>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Drawn into the target, or straight into a swapchain image.
    Drawn,
    /// Drawn into the target and copied to a swapchain image.
    Copied,
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
                        .query_count(3),
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
                pending: None,
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
        let gpu = self.collect(slot)?;
        self.submit(slot, target, clear, draw, None)?;
        Ok(gpu.map(|g| g.world_ms))
    }

    /// [`FrameLoop::render`] into the window's next image, and presents it. With
    /// [`Swapchain::direct`] and the swapchain's size and format, the target only
    /// lends its depth and the frame is drawn straight into the image; otherwise it is
    /// drawn into the target and copied (scaled if the sizes differ). `out_of_date`
    /// means the swapchain no longer matches the window: wait with
    /// [`FrameLoop::finish`], then [`Swapchain::resize`]. The frame may then not have
    /// been drawn.
    pub fn present(
        &mut self,
        swapchain: &Swapchain,
        target: &Target,
        clear: [f32; 4],
        draw: impl FnOnce(&mut crate::Frame),
    ) -> Result<Presented> {
        let slot = self.next;
        self.next = (slot + 1) % FRAMES_IN_FLIGHT;
        let t0 = Instant::now();
        let gpu = self.collect(slot)?;
        let t1 = Instant::now();
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
                    gpu,
                    cpu: Steps {
                        wait_ms: ms(t1 - t0),
                        acquire_ms: ms(t1.elapsed()),
                        ..Steps::default()
                    },
                    out_of_date: true,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let t2 = Instant::now();
        self.submit(slot, target, clear, draw, Some((swapchain, image)))?;
        let t3 = Instant::now();
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
            gpu,
            cpu: Steps {
                wait_ms: ms(t1 - t0),
                acquire_ms: ms(t2 - t1),
                submit_ms: ms(t3 - t2),
                present_ms: ms(t3.elapsed()),
            },
            out_of_date,
        })
    }

    /// Records the slot's frame and submits it; with a swapchain image, draws into it
    /// or copies the target to it (waiting for the image, and signalling it
    /// rendered).
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
        let direct = present
            .filter(|(swapchain, _)| {
                swapchain.direct
                    && swapchain.format == Target::COLOR_FORMAT
                    && swapchain.extent == target.color.extent
            })
            .map(|(swapchain, i)| (swapchain.images[i as usize], swapchain.views[i as usize]));
        let copy = present.filter(|_| direct.is_none());
        unsafe {
            d.reset_fences(&[s.fence])?;
            d.begin_command_buffer(
                s.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            d.cmd_reset_query_pool(s.cmd, s.queries, 0, 3);
            target.record(gpu, s.cmd, clear, slot, (s.queries, 0), direct, draw);
            if let Some((swapchain, image)) = copy {
                blit_to(
                    gpu,
                    s.cmd,
                    target,
                    swapchain.images[image as usize],
                    swapchain.extent,
                );
                d.cmd_write_timestamp2(
                    s.cmd,
                    vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                    s.queries,
                    2,
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
        s.pending = Some(match copy {
            Some(_) => Kind::Copied,
            None => Kind::Drawn,
        });
        Ok(())
    }

    /// Waits for every frame in flight and returns their GPU times in ms, oldest
    /// first. The target can be read back afterwards.
    pub fn finish(&mut self) -> Result<Vec<f64>> {
        let mut times = Vec::new();
        for i in 0..FRAMES_IN_FLIGHT {
            let slot = (self.next + i) % FRAMES_IN_FLIGHT;
            times.extend(self.collect(slot)?.map(|g| g.world_ms));
        }
        Ok(times)
    }

    /// Waits for the slot's last frame and reads its GPU times, if one is pending.
    fn collect(&mut self, slot: usize) -> Result<Option<GpuTimes>> {
        let d = &self.gpu.device;
        let s = &mut self.slots[slot];
        unsafe { d.wait_for_fences(&[s.fence], true, u64::MAX)? };
        let Some(kind) = s.pending.take() else {
            return Ok(None);
        };
        let mut stamps = [0u64; 3];
        let n = if kind == Kind::Copied { 3 } else { 2 };
        unsafe {
            d.get_query_pool_results(
                s.queries,
                0,
                &mut stamps[..n],
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            )?
        };
        let between = |a: u64, b: u64| b.wrapping_sub(a) as f64 * self.period / 1e6;
        Ok(Some(GpuTimes {
            start_ms: stamps[0] as f64 * self.period / 1e6,
            world_ms: between(stamps[0], stamps[1]),
            blit_ms: (kind == Kind::Copied).then(|| between(stamps[1], stamps[2])),
        }))
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
    /// The GPU times of the frame [`FRAMES_IN_FLIGHT`] frames ago.
    pub gpu: Option<GpuTimes>,
    /// How long each step of this call took on the CPU.
    pub cpu: Steps,
    /// The swapchain must be resized before the next frame.
    pub out_of_date: bool,
}

/// One frame's GPU timestamps, in ms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuTimes {
    /// When the GPU started the frame, on the GPU's clock (only differences between
    /// frames mean anything: the gap from the previous frame's end is time the GPU
    /// spent on other work, such as presenting, or idle).
    pub start_ms: f64,
    /// Drawing the world into the target.
    pub world_ms: f64,
    /// Copying the target to the swapchain image (only when it is not drawn into
    /// directly).
    pub blit_ms: Option<f64>,
}

/// The CPU time of each step of [`FrameLoop::present`], in ms.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Steps {
    /// Waiting for the frame [`FRAMES_IN_FLIGHT`] frames ago.
    pub wait_ms: f64,
    /// `vkAcquireNextImageKHR`.
    pub acquire_ms: f64,
    /// Recording and submitting the frame.
    pub submit_ms: f64,
    /// `vkQueuePresentKHR`.
    pub present_ms: f64,
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
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
