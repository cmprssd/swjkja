//! The Vulkan renderer (milestone 2). So far: device bring-up, an offscreen render
//! target with readback, buffers and pipelines.

pub mod camera;
pub mod deform;
pub mod fog;
pub mod gpu;
pub mod parallel;
pub mod pipeline;
pub mod resource;
pub mod sky;
pub mod texture;
pub mod vis;
pub mod world;

pub use ash::vk;
pub use camera::Camera;
pub use gpu::{DeviceKind, Error, Gpu, Result};
pub use pipeline::{Pipeline, State, spirv};
pub use resource::{Buffer, Frame, Image, Target};
pub use texture::{MAX_TEXTURES, Texture, TextureSet};
pub use world::WorldRenderer;
