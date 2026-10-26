//! The Vulkan renderer (milestone 2). So far: device bring-up, an offscreen render
//! target with readback, buffers and pipelines.

pub mod camera;
pub mod deform;
pub mod fog;
pub mod frames;
pub mod gpu;
pub mod look;
pub mod models;
pub mod parallel;
pub mod path;
pub mod pipeline;
pub mod resource;
pub mod sky;
pub mod sprites;
pub mod swapchain;
pub mod texture;
pub mod vis;
pub mod weather;
pub mod world;

pub use ash::vk;
pub use camera::Camera;
pub use frames::{FRAMES_IN_FLIGHT, FrameLoop, GpuTimes, Presented, Steps};
pub use gpu::{DeviceKind, Error, Gpu, Result};
pub use pipeline::{Pipeline, State, spirv};
pub use resource::{Buffer, Frame, Image, Target};
pub use swapchain::Swapchain;
pub use texture::{MAX_TEXTURES, Texture, TextureSet};
pub use world::WorldRenderer;
