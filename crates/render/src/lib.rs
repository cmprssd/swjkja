//! The Vulkan renderer (milestone 2). So far: device bring-up, an offscreen render
//! target with readback, buffers and pipelines.

pub mod gpu;
pub mod pipeline;
pub mod resource;

pub use ash::vk;
pub use gpu::{DeviceKind, Error, Gpu, Result};
pub use pipeline::{Pipeline, spirv};
pub use resource::{Buffer, Image, Target};
