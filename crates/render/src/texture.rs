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

impl Texture {
    /// Uploads RGBA8 pixels (top row first). With `mipmaps`, the smaller levels are
    /// made on the GPU by halving with a linear filter. (The original makes them on
    /// the CPU with its own filter; matching that comes with texture fidelity.)
    pub fn new(
        gpu: &Arc<Gpu>,
        width: u32,
        height: u32,
        rgba: &[u8],
        mipmaps: bool,
    ) -> Result<Self> {
        let mip_levels = if mipmaps {
            32 - width.max(height).leading_zeros()
        } else {
            1
        };
        let format = vk::Format::R8G8B8A8_UNORM;
        let image = Image::new_mipmapped(
            gpu,
            vk::Extent2D { width, height },
            format,
            mip_levels,
            vk::ImageUsageFlags::SAMPLED
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::TRANSFER_SRC,
        )?;
        let staging = Buffer::with_data(gpu, vk::BufferUsageFlags::TRANSFER_SRC, rgba)?;
        let d = &gpu.device;
        let color = vk::ImageAspectFlags::COLOR;
        let level_range = |base: u32, count: u32| vk::ImageSubresourceRange {
            aspect_mask: color,
            base_mip_level: base,
            level_count: count,
            base_array_layer: 0,
            layer_count: 1,
        };
        let barrier = |cmd, base, count, from, to| {
            let b = [vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                .src_access_mask(vk::AccessFlags2::MEMORY_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                .dst_access_mask(vk::AccessFlags2::MEMORY_READ | vk::AccessFlags2::MEMORY_WRITE)
                .old_layout(from)
                .new_layout(to)
                .image(image.image)
                .subresource_range(level_range(base, count))];
            unsafe {
                d.cmd_pipeline_barrier2(
                    cmd,
                    &vk::DependencyInfo::default().image_memory_barriers(&b),
                )
            };
        };
        gpu.submit_and_wait(|cmd| unsafe {
            barrier(
                cmd,
                0,
                mip_levels,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            d.cmd_copy_buffer_to_image(
                cmd,
                staging.buffer,
                image.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy {
                    image_subresource: layers(0),
                    image_extent: vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    },
                    ..Default::default()
                }],
            );
            let (mut w, mut h) = (width as i32, height as i32);
            for level in 1..mip_levels {
                barrier(
                    cmd,
                    level - 1,
                    1,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                );
                let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
                d.cmd_blit_image(
                    cmd,
                    image.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::ImageBlit {
                        src_subresource: layers(level - 1),
                        src_offsets: [vk::Offset3D::default(), vk::Offset3D { x: w, y: h, z: 1 }],
                        dst_subresource: layers(level),
                        dst_offsets: [vk::Offset3D::default(), vk::Offset3D { x: nw, y: nh, z: 1 }],
                    }],
                    vk::Filter::LINEAR,
                );
                barrier(
                    cmd,
                    level - 1,
                    1,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
                (w, h) = (nw, nh);
            }
            barrier(
                cmd,
                mip_levels - 1,
                1,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        })?;
        Ok(Self { image, mip_levels })
    }
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
        self.textures.push(None);
        slot
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
            let _ = self.gpu.device.device_wait_idle();
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
