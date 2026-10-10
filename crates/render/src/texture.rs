//! Textures and the one texture array every draw indexes into.

use std::sync::Arc;

use ash::vk;

use crate::gpu::{Gpu, Result};
use crate::resource::{Buffer, Image};

/// Slots in the texture array; a draw picks one with a push constant.
pub const MAX_TEXTURES: u32 = 4096;

pub struct Texture {
    pub image: Image,
    pub mip_levels: u32,
}

/// Pixels waiting to be copied into textures: queued by [`Texture::queued`], sent
/// to the GPU together by [`Uploads::flush`].
#[derive(Default)]
pub struct Uploads {
    pending: Vec<Pending>,
}

struct Pending {
    image: vk::Image,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    mipmaps: bool,
}

/// Staging memory per submit: large enough that a map's textures go in a few
/// submits, small enough not to matter next to the textures themselves.
const STAGING_BYTES: usize = 64 << 20;

impl Texture {
    /// A texture of RGBA8 pixels (top row first), uploaded now. With `mipmaps`, the
    /// smaller levels are made as the original makes them (`swjkja_image::mip`).
    pub fn new(
        gpu: &Arc<Gpu>,
        width: u32,
        height: u32,
        rgba: &[u8],
        mipmaps: bool,
    ) -> Result<Self> {
        let mut uploads = Uploads::default();
        let texture = Self::queued(gpu, width, height, rgba.to_vec(), mipmaps, &mut uploads)?;
        uploads.flush(gpu)?;
        Ok(texture)
    }

    /// A texture whose pixels are copied at the next `uploads.flush`; it must not be
    /// drawn before then.
    pub fn queued(
        gpu: &Arc<Gpu>,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        mipmaps: bool,
        uploads: &mut Uploads,
    ) -> Result<Self> {
        let mip_levels = if mipmaps {
            32 - width.max(height).leading_zeros()
        } else {
            1
        };
        let image = Image::new_mipmapped(
            gpu,
            vk::Extent2D { width, height },
            vk::Format::R8G8B8A8_UNORM,
            mip_levels,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
        )?;
        uploads.pending.push(Pending {
            image: image.image,
            width,
            height,
            rgba,
            mipmaps,
        });
        Ok(Self { image, mip_levels })
    }
}

impl Uploads {
    /// Copies every queued texture to the GPU, mip levels included, and leaves them
    /// ready for sampling. Levels are made in parallel straight into the staging
    /// memory, in batches of about `STAGING_BYTES` (a bigger texture goes alone).
    pub fn flush(&mut self, gpu: &Arc<Gpu>) -> Result<()> {
        let pending = std::mem::take(&mut self.pending);
        let size = |p: &Pending| {
            if p.mipmaps {
                swjkja_image::mip::chain_size(p.width, p.height)
            } else {
                p.rgba.len()
            }
        };
        let mut start = 0;
        while start < pending.len() {
            let (mut end, mut bytes) = (start, 0);
            while end < pending.len()
                && (end == start || bytes + size(&pending[end]) <= STAGING_BYTES)
            {
                bytes += size(&pending[end]);
                end += 1;
            }
            upload_batch(gpu, &pending[start..end], bytes, size)?;
            start = end;
        }
        Ok(())
    }
}

fn upload_batch(
    gpu: &Arc<Gpu>,
    pending: &[Pending],
    bytes: usize,
    size: impl Fn(&Pending) -> usize,
) -> Result<()> {
    let mut staging = Buffer::new(
        gpu,
        vk::BufferUsageFlags::TRANSFER_SRC,
        bytes as u64,
        gpu_allocator::MemoryLocation::CpuToGpu,
    )?;
    // Each texture's slice of the staging memory, and its copy regions.
    let mut regions: Vec<Vec<vk::BufferImageCopy>> = Vec::with_capacity(pending.len());
    let mut offset = 0u64;
    for p in pending {
        let (mut w, mut h) = (p.width, p.height);
        let mut r = Vec::new();
        loop {
            r.push(vk::BufferImageCopy {
                buffer_offset: offset,
                image_subresource: layers(r.len() as u32),
                image_extent: vk::Extent3D {
                    width: w,
                    height: h,
                    depth: 1,
                },
                ..Default::default()
            });
            offset += u64::from(w) * u64::from(h) * 4;
            if !p.mipmaps || (w == 1 && h == 1) {
                break;
            }
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        regions.push(r);
    }
    let mut slices: Vec<(&Pending, &mut [u8])> = Vec::with_capacity(pending.len());
    let mut rest = staging.mapped_mut()?;
    for p in pending {
        let (slice, tail) = rest.split_at_mut(size(p));
        slices.push((p, slice));
        rest = tail;
    }
    // Staging memory is write-combined: reading it back is very slow, so each chain is
    // made in ordinary memory and written to staging in one pass.
    crate::parallel::each(slices, |(p, out)| {
        if p.mipmaps {
            let mut chain = vec![0; out.len()];
            swjkja_image::mip::mip_chain_into(&p.rgba, p.width, p.height, &mut chain);
            out.copy_from_slice(&chain);
        } else {
            out.copy_from_slice(&p.rgba);
        }
    });
    let d = &gpu.device;
    let barriers = |from, to| -> Vec<vk::ImageMemoryBarrier2> {
        pending
            .iter()
            .map(|p| {
                vk::ImageMemoryBarrier2::default()
                    .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                    .src_access_mask(vk::AccessFlags2::MEMORY_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                    .dst_access_mask(vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE)
                    .old_layout(from)
                    .new_layout(to)
                    .image(p.image)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: vk::REMAINING_MIP_LEVELS,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
            })
            .collect()
    };
    let to_dst = barriers(
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
    );
    let to_read = barriers(
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    );
    gpu.submit_and_wait(|cmd| unsafe {
        d.cmd_pipeline_barrier2(
            cmd,
            &vk::DependencyInfo::default().image_memory_barriers(&to_dst),
        );
        for (p, r) in pending.iter().zip(&regions) {
            d.cmd_copy_buffer_to_image(
                cmd,
                staging.buffer,
                p.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                r,
            );
        }
        d.cmd_pipeline_barrier2(
            cmd,
            &vk::DependencyInfo::default().image_memory_barriers(&to_read),
        );
    })
}

fn layers(level: u32) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        mip_level: level,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// The descriptor set every pipeline uses: binding 0 is an array of
/// [`MAX_TEXTURES`] textures, binding 1 a repeating sampler with mipmaps
/// (`GL_LINEAR_MIPMAP_NEAREST`, the original's default `r_textureMode`), binding 2 the
/// same clamping at the edges (`clampMap` textures and lightmaps, which have one level).
pub struct TextureSet {
    gpu: Arc<Gpu>,
    pub layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    pub set: vk::DescriptorSet,
    samplers: [vk::Sampler; 2],
    textures: Vec<Option<Texture>>,
}

impl TextureSet {
    pub fn new(gpu: &Arc<Gpu>) -> Result<Self> {
        let d = &gpu.device;
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(MAX_TEXTURES)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];
        let flags = [
            vk::DescriptorBindingFlags::PARTIALLY_BOUND
                | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND,
            vk::DescriptorBindingFlags::empty(),
            vk::DescriptorBindingFlags::empty(),
        ];
        let mut binding_flags =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&flags);
        let layout = unsafe {
            d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
                    .bindings(&bindings)
                    .push_next(&mut binding_flags),
                None,
            )?
        };
        let sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: MAX_TEXTURES,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: 2,
            },
        ];
        let pool = unsafe {
            d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )?
        };
        let layouts = [layout];
        let set = unsafe {
            d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )?[0]
        };
        let sampler = |repeat: bool| unsafe {
            let mode = if repeat {
                vk::SamplerAddressMode::REPEAT
            } else {
                vk::SamplerAddressMode::CLAMP_TO_EDGE
            };
            d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                    .address_mode_u(mode)
                    .address_mode_v(mode)
                    .address_mode_w(mode)
                    // r_ext_texture_filter_anisotropic 16, the original's default; it
                    // applies to mipmapped images, the only ones with more than one level.
                    .anisotropy_enable(true)
                    .max_anisotropy(16.0)
                    .max_lod(vk::LOD_CLAMP_NONE),
                None,
            )
        };
        let samplers = [sampler(true)?, sampler(false)?];
        for (binding, s) in [(1, samplers[0]), (2, samplers[1])] {
            let info = [vk::DescriptorImageInfo::default().sampler(s)];
            let write = vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .image_info(&info);
            unsafe { d.update_descriptor_sets(&[write], &[]) };
        }
        Ok(Self {
            gpu: gpu.clone(),
            layout,
            pool,
            set,
            samplers,
            textures: Vec::new(),
        })
    }

    /// Adds a texture and returns its slot.
    pub fn add(&mut self, texture: Texture) -> u32 {
        let slot = self.textures.len() as u32;
        assert!(slot < MAX_TEXTURES, "texture array full");
        let info = [vk::DescriptorImageInfo::default()
            .image_view(texture.image.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(0)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(&info);
        unsafe { self.gpu.device.update_descriptor_sets(&[write], &[]) };
        self.textures.push(Some(texture));
        slot
    }

    /// Puts an image view the caller owns (such as a target's depth copy, in
    /// `SHADER_READ_ONLY_OPTIMAL`) into a slot. The view must outlive this set.
    pub fn add_view(&mut self, view: vk::ImageView) -> u32 {
        let slot = self.textures.len() as u32;
        assert!(slot < MAX_TEXTURES, "texture array full");
        self.textures.push(None);
        self.set_view(slot, view);
        slot
    }

    /// Points a slot from [`TextureSet::add_view`] at another view (a resized
    /// target's), when no frame in flight uses the set.
    pub fn set_view(&mut self, slot: u32, view: vk::ImageView) {
        assert!(
            self.textures[slot as usize].is_none(),
            "slot {slot} holds a texture, not a view"
        );
        let info = [vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(0)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(&info);
        unsafe { self.gpu.device.update_descriptor_sets(&[write], &[]) };
    }

    pub fn len(&self) -> usize {
        self.textures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
}

impl Drop for TextureSet {
    fn drop(&mut self) {
        unsafe {
            self.gpu.wait_idle();
            self.textures.clear();
            for s in self.samplers {
                self.gpu.device.destroy_sampler(s, None);
            }
            self.gpu.device.destroy_descriptor_pool(self.pool, None);
            self.gpu
                .device
                .destroy_descriptor_set_layout(self.layout, None);
        }
    }
}
