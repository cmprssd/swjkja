//! Renders offscreen and reads the pixels back. Needs a Vulkan 1.3 device: the GPU on
//! Windows, Mesa's lavapipe (CPU) under WSL.

use swjkja_render::{Buffer, Gpu, Pipeline, Target, spirv, vk};

/// Position (3 floats) and colour (4 bytes) per vertex.
fn vertices(tris: &[([f32; 3], [u8; 4])]) -> Vec<u8> {
    tris.iter()
        .flat_map(|(p, c)| p.iter().flat_map(|f| f.to_le_bytes()).chain(*c))
        .collect()
}

#[test]
fn renders_offscreen_with_depth_test() {
    let gpu = Gpu::new_headless().expect("a Vulkan 1.3 device");
    eprintln!("device: {} ({:?})", gpu.name, gpu.kind);
    let target = Target::new(&gpu, 64, 64).unwrap();
    let pipeline = Pipeline::new(
        &gpu,
        spirv::FLAT_VERT,
        spirv::FLAT_FRAG,
        16,
        &[
            (0, vk::Format::R32G32B32_SFLOAT, 0),
            (1, vk::Format::R8G8B8A8_UNORM, 12),
        ],
        0,
    )
    .unwrap();
    let (red, green) = ([255, 0, 0, 255], [0, 255, 0, 255]);
    // Near red triangle first, then a far green one covering the whole view: the depth
    // test must keep red where they overlap. (Vulkan's y axis points down.)
    let data = vertices(&[
        ([-0.5, -0.5, 0.2], red),
        ([0.5, -0.5, 0.2], red),
        ([0.0, 0.5, 0.2], red),
        ([-1.0, -1.0, 0.8], green),
        ([3.0, -1.0, 0.8], green),
        ([-1.0, 3.0, 0.8], green),
    ]);
    let vb = Buffer::with_data(&gpu, vk::BufferUsageFlags::VERTEX_BUFFER, &data).unwrap();
    target
        .render(&gpu, [0.0, 0.0, 1.0, 1.0], |cmd| unsafe {
            gpu.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline.pipeline);
            gpu.device
                .cmd_bind_vertex_buffers(cmd, 0, &[vb.buffer], &[0]);
            gpu.device.cmd_draw(cmd, 6, 1, 0, 0);
        })
        .unwrap();
    let px = target.read_rgba(&gpu).unwrap();
    let at = |x: usize, y: usize| -> [u8; 4] { px[(y * 64 + x) * 4..][..4].try_into().unwrap() };
    assert_eq!(px.len(), 64 * 64 * 4);
    assert_eq!(at(32, 32), red, "centre: the near triangle wins");
    assert_eq!(at(2, 2), green, "top-left: only the far triangle");
    assert_eq!(
        at(32, 4),
        green,
        "top centre: above the red triangle's apex region"
    );

    // A second render with nothing drawn shows the clear colour everywhere.
    target.render(&gpu, [0.0, 0.0, 1.0, 1.0], |_| {}).unwrap();
    let px = target.read_rgba(&gpu).unwrap();
    assert!(px.chunks(4).all(|p| p == [0, 0, 255, 255]));
}
