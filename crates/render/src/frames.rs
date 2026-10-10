//! A frame loop with [`FRAMES_IN_FLIGHT`] frames in flight: the CPU records the next
//! frame while the GPU draws the previous one, and waits only for the frame that
//! last used the same slot. Each slot has its own command buffer, fence and
//! timestamps; per-frame data (uniforms, deformed geometry) is kept per slot by its
//! owner and chosen with [`crate::Frame::slot`].

use std::sync::Arc;

use ash::vk;

use crate::{Gpu, Result, Target};

pub const FRAMES_IN_FLIGHT: usize = 2;

struct Slot {
    cmd: vk::CommandBuffer,
    /// Signalled when the slot's last frame is done (created signalled).
    fence: vk::Fence,
    /// Two timestamps around the slot's last frame.
    queries: vk::QueryPool,
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
            frames.slots.push(Slot {
                cmd,
                fence,
                queries,
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
        let d = &self.gpu.device;
        let s = &mut self.slots[slot];
        unsafe {
            d.reset_fences(&[s.fence])?;
            d.begin_command_buffer(
                s.cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            d.cmd_reset_query_pool(s.cmd, s.queries, 0, 2);
            target.record(&self.gpu, s.cmd, clear, slot, (s.queries, 0), draw);
            d.end_command_buffer(s.cmd)?;
            d.queue_submit(
                self.gpu.queue,
                &[vk::SubmitInfo::default().command_buffers(&[s.cmd])],
                s.fence,
            )?;
        }
        s.pending = true;
        Ok(gpu_ms)
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
            }
            // Freed with the pool.
            d.destroy_command_pool(self.pool, None);
        }
    }
}
