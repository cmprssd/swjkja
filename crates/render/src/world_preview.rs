//! A preview of a map: every surface drawn with one texture times its lightmap (or
//! vertex colours). The geometry (faces, triangle soups, patch grids triangulated as
//! `RB_SurfaceGrid` does at full detail) is what the real renderer will use; the
//! shading is a stand-in until shader stages are implemented.

use std::sync::Arc;

use ash::vk;
use bytemuck::{Pod, Zeroable};
use swjkja_world::{Geometry, World};

use crate::camera::Camera;
use crate::gpu::{Gpu, Result};
use crate::pipeline::{Pipeline, spirv};
use crate::resource::Buffer;
use crate::texture::TextureSet;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    xyz: [f32; 3],
    st: [f32; 2],
    lm: [f32; 2],
    color: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Push {
    view_proj: [[f32; 4]; 4],
    texture: u32,
    lightmap: u32,
    mode: u32,
    pad: u32,
}

/// How a surface is lit in the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    /// A lightmap page (its texture slot).
    Lightmap(u32),
    Vertex,
    None,
}

/// What to draw a surface with: its texture slot and lighting, or `None` to skip it.
pub type SurfaceLook = Option<(u32, Light)>;

struct Draw {
    first_index: u32,
    count: u32,
    texture: u32,
    light: Light,
}

pub struct WorldPreview {
    vertices: Buffer,
    indexes: Buffer,
    draws: Vec<Draw>,
    pipeline: Pipeline,
}

impl WorldPreview {
    /// `looks[i]` decides how surface `i` is drawn.
    pub fn new(
        gpu: &Arc<Gpu>,
        textures: &TextureSet,
        world: &World,
        looks: &[SurfaceLook],
    ) -> Result<Self> {
        let mut verts: Vec<Vertex> = Vec::new();
        let mut indexes: Vec<u32> = Vec::new();
        let mut draws = Vec::new();
        for (surface, look) in world.surfaces.iter().zip(looks) {
            let Some((texture, light)) = *look else {
                continue;
            };
            let base = verts.len() as u32;
            let first_index = indexes.len() as u32;
            let vertex = |v: &swjkja_bsp::DrawVert| Vertex {
                xyz: v.xyz,
                st: v.st,
                lm: v.lightmap[0],
                color: v.color[0],
            };
            match &surface.geometry {
                Geometry::Face {
                    verts: v,
                    indexes: i,
                    ..
                }
                | Geometry::Triangles {
                    verts: v,
                    indexes: i,
                    ..
                } => {
                    verts.extend(v.iter().map(vertex));
                    indexes.extend(i.iter().map(|&i| base + i));
                }
                Geometry::Grid(g) => {
                    verts.extend(g.verts.iter().map(vertex));
                    let w = g.width as u32;
                    for row in 0..g.height as u32 - 1 {
                        for col in 0..w - 1 {
                            let v1 = base + row * w + col + 1;
                            let v2 = v1 - 1;
                            let v3 = v2 + w;
                            let v4 = v3 + 1;
                            indexes.extend([v2, v3, v1, v1, v3, v4]);
                        }
                    }
                }
                Geometry::Flare { .. } | Geometry::Skip => {}
            }
            let count = indexes.len() as u32 - first_index;
            if count > 0 {
                draws.push(Draw {
                    first_index,
                    count,
                    texture,
                    light,
                });
            }
        }
        let pipeline = Pipeline::new(
            gpu,
            spirv::WORLD_PREVIEW_VERT,
            spirv::WORLD_PREVIEW_FRAG,
            size_of::<Vertex>() as u32,
            &[
                (0, vk::Format::R32G32B32_SFLOAT, 0),
                (1, vk::Format::R32G32_SFLOAT, 12),
                (2, vk::Format::R32G32_SFLOAT, 20),
                (3, vk::Format::R8G8B8A8_UNORM, 28),
            ],
            size_of::<Push>() as u32,
            &[textures.layout],
        )?;
        Ok(Self {
            vertices: Buffer::with_data(
                gpu,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                bytemuck::cast_slice(&verts),
            )?,
            indexes: Buffer::with_data(
                gpu,
                vk::BufferUsageFlags::INDEX_BUFFER,
                bytemuck::cast_slice(&indexes),
            )?,
            draws,
            pipeline,
        })
    }

    pub fn triangles(&self) -> usize {
        self.draws.iter().map(|d| d.count as usize / 3).sum()
    }

    /// Records the draws (inside [`crate::Target::render`]).
    pub fn draw(&self, gpu: &Gpu, cmd: vk::CommandBuffer, textures: &TextureSet, camera: &Camera) {
        let d = &gpu.device;
        let layout = self.pipeline.layout;
        unsafe {
            d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline.pipeline);
            d.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                layout,
                0,
                &[textures.set],
                &[],
            );
            d.cmd_bind_vertex_buffers(cmd, 0, &[self.vertices.buffer], &[0]);
            d.cmd_bind_index_buffer(cmd, self.indexes.buffer, 0, vk::IndexType::UINT32);
            let view_proj = camera.view_proj();
            for draw in &self.draws {
                let (lightmap, mode) = match draw.light {
                    Light::Lightmap(slot) => (slot, 0),
                    Light::Vertex => (draw.texture, 1),
                    Light::None => (draw.texture, 2),
                };
                let push = Push {
                    view_proj,
                    texture: draw.texture,
                    lightmap,
                    mode,
                    pad: 0,
                };
                d.cmd_push_constants(
                    cmd,
                    layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    bytemuck::bytes_of(&push),
                );
                d.cmd_draw_indexed(cmd, draw.count, 1, draw.first_index, 0, 0);
            }
        }
    }
}
