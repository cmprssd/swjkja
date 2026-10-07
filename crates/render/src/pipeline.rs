//! Graphics pipelines. Shaders are compiled to SPIR-V by build.rs.

use std::sync::Arc;

use ash::vk;

use crate::gpu::{Error, Gpu, Result};
use crate::resource::Target;

/// SPIR-V for each shader in shaders/, compiled at build time.
pub mod spirv {
    pub const FLAT_VERT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flat.vert.spv"));
    pub const FLAT_FRAG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flat.frag.spv"));
}

fn shader_module(gpu: &Gpu, spirv: &[u8]) -> Result<vk::ShaderModule> {
    let words = ash::util::read_spv(&mut std::io::Cursor::new(spirv))
        .map_err(|e| Error(format!("bad SPIR-V: {e}")))?;
    Ok(unsafe {
        gpu.device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)?
    })
}

/// A vertex attribute: location, format, byte offset.
pub type Attribute = (u32, vk::Format, u32);

pub struct Pipeline {
    gpu: Arc<Gpu>,
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
}

impl Pipeline {
    /// A triangle-list pipeline drawing into a [`Target`], with depth test and write,
    /// no blending and no culling, and push constants of `push_size` bytes for both
    /// stages.
    pub fn new(
        gpu: &Arc<Gpu>,
        vert: &[u8],
        frag: &[u8],
        stride: u32,
        attributes: &[Attribute],
        push_size: u32,
    ) -> Result<Self> {
        let d = &gpu.device;
        let (vs, fs) = (shader_module(gpu, vert)?, shader_module(gpu, frag)?);
        let ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            offset: 0,
            size: push_size,
        }];
        let layout =
            unsafe {
                d.create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .push_constant_ranges(if push_size > 0 { &ranges } else { &[] }),
                    None,
                )?
            };
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vs)
                .name(c"vs_main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fs)
                .name(c"fs_main"),
        ];
        let bindings = [vk::VertexInputBindingDescription {
            binding: 0,
            stride,
            input_rate: vk::VertexInputRate::VERTEX,
        }];
        let attrs: Vec<_> = attributes
            .iter()
            .map(
                |&(location, format, offset)| vk::VertexInputAttributeDescription {
                    location,
                    binding: 0,
                    format,
                    offset,
                },
            )
            .collect();
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attrs);
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
        let blend_attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let color_formats = [Target::COLOR_FORMAT];
        let mut rendering = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&color_formats)
            .depth_attachment_format(Target::DEPTH_FORMAT);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(layout)
            .push_next(&mut rendering);
        let pipeline =
            unsafe { d.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) };
        unsafe {
            d.destroy_shader_module(vs, None);
            d.destroy_shader_module(fs, None);
        }
        let pipeline = pipeline.map_err(|(_, e)| Error::from(e))?[0];
        Ok(Self {
            gpu: gpu.clone(),
            pipeline,
            layout,
        })
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        unsafe {
            self.gpu.device.destroy_pipeline(self.pipeline, None);
            self.gpu.device.destroy_pipeline_layout(self.layout, None);
        }
    }
}
