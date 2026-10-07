//! The Vulkan renderer (milestone 2). So far: device bring-up, an offscreen render
//! target with readback, buffers and pipelines.

pub mod camera;
pub mod gpu;
pub mod pipeline;
pub mod resource;
pub mod texture;
pub mod world;

pub use ash::vk;
pub use camera::Camera;
pub use gpu::{DeviceKind, Error, Gpu, Result};
pub use pipeline::{Pipeline, State, spirv};
pub use resource::{Buffer, Image, Target};
pub use texture::{MAX_TEXTURES, Texture, TextureSet};
pub use world::WorldRenderer;
