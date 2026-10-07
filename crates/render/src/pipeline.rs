//! Graphics pipelines. Shaders are compiled to SPIR-V by build.rs.

use std::sync::Arc;

use ash::vk;

use crate::gpu::{Error, Gpu, Result};
use crate::resource::Target;

/// SPIR-V for each shader in shaders/, compiled at build time.
pub mod spirv {
    pub const FLAT_VERT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flat.vert.spv"));
    pub const FLAT_FRAG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flat.frag.spv"));
    pub const WORLD_VERT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/world.vert.spv"));
    pub const WORLD_FRAG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/world.frag.spv"));
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

/// Fixed-function state a pipeline is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct State {
    /// Source and destination blend factors; `None` draws without blending.
    pub blend: Option<(vk::BlendFactor, vk::BlendFactor)>,
    pub depth_write: bool,
    /// `None` turns the depth test off.
    pub depth_compare: Option<vk::CompareOp>,
    pub cull: vk::CullModeFlags,
    pub front_face: vk::FrontFace,
    /// Polygon offset (decals): pulls the surface towards the viewer.
    pub depth_bias: bool,
    /// Which colour channels are written.
    pub write_mask: vk::ColorComponentFlags,
}

impl Default for State {
    /// Depth test and write (reversed depth), no blending, no culling.
    fn default() -> Self {
        Self {
            blend: None,
            depth_write: true,
            depth_compare: Some(vk::CompareOp::GREATER_OR_EQUAL),
            cull: vk::CullModeFlags::NONE,
            front_face: vk::FrontFace::COUNTER_CLOCKWISE,
            depth_bias: false,
            write_mask: vk::ColorComponentFlags::RGBA,
        }
    }
}

pub struct Pipeline {
    gpu: Arc<Gpu>,
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
}

impl Pipeline {
    /// A triangle-list pipeline drawing into a [`Target`], with depth test and write,
    /// no blending and no culling, push constants of `push_size` bytes for both
    /// stages, and the given descriptor set layouts.
    pub fn new(
        gpu: &Arc<Gpu>,
        vert: &[u8],
        frag: &[u8],
        stride: u32,
        attributes: &[Attribute],
        push_size: u32,
        set_layouts: &[vk::DescriptorSetLayout],
    ) -> Result<Self> {
        Self::with_state(
            gpu,
            vert,
            frag,
            stride,
            attributes,
            push_size,
            set_layouts,
            State::default(),
        )
    }

    /// Like [`Pipeline::new`], with the given fixed-function state.
    #[allow(clippy::too_many_arguments)]
    pub fn with_state(
        gpu: &Arc<Gpu>,
        vert: &[u8],
        frag: &[u8],
        stride: u32,
        attributes: &[Attribute],
        push_size: u32,
        set_layouts: &[vk::DescriptorSetLayout],
        state: State,
    ) -> Result<Self> {
        let d = &gpu.device;
        let (vs, fs) = (shader_module(gpu, vert)?, shader_module(gpu, frag)?);
        let ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            offset: 0,
            size: push_size,
        }];
        let layout = unsafe {
            d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .push_constant_ranges(if push_size > 0 { &ranges } else { &[] })
                    .set_layouts(set_layouts),
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
            .cull_mode(state.cull)
            .front_face(state.front_face)
            // r_offsetFactor -1, r_offsetUnits -2 in the original's depth direction.
            .depth_bias_enable(state.depth_bias)
            .depth_bias_slope_factor(1.0)
            .depth_bias_constant_factor(2.0)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(state.depth_compare.is_some())
            .depth_write_enable(state.depth_write)
            .depth_compare_op(state.depth_compare.unwrap_or(vk::CompareOp::ALWAYS));
        let mut attachment =
            vk::PipelineColorBlendAttachmentState::default().color_write_mask(state.write_mask);
        if let Some((src, dst)) = state.blend {
            attachment = attachment
                .blend_enable(true)
                .src_color_blend_factor(src)
                .dst_color_blend_factor(dst)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(src)
                .dst_alpha_blend_factor(dst)
                .alpha_blend_op(vk::BlendOp::ADD);
        }
        let blend_attachments = [attachment];
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
