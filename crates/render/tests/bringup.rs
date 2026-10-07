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
        &[],
    )
    .unwrap();
    let (red, green) = ([255, 0, 0, 255], [0, 255, 0, 255]);
    // Near red triangle first, then a far green one covering the whole view: the depth
    // test must keep red where they overlap. Depth is reversed (larger is nearer), and
    // Vulkan's y axis points down.
    let data = vertices(&[
        ([-0.5, -0.5, 0.8], red),
        ([0.5, -0.5, 0.8], red),
        ([0.0, 0.5, 0.8], red),
        ([-1.0, -1.0, 0.2], green),
        ([3.0, -1.0, 0.2], green),
        ([-1.0, 3.0, 0.2], green),
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

#[test]
fn the_camera_puts_up_at_the_top_and_left_at_the_left() {
    use swjkja_render::Camera;
    let gpu = Gpu::new_headless().expect("a Vulkan 1.3 device");
    let (w, h) = (64u32, 64u32);
    let target = Target::new(&gpu, w, h).unwrap();
    // Looking along +x from the origin: +z is up, +y is left.
    let camera = Camera::new([0.0; 3], [0.0, 0.0, 0.0], w, h);
    let m = camera.view_proj();
    // Transform on the CPU with the same matrix the shaders use, then draw in clip space.
    let clip = |p: [f32; 3]| {
        let c = [0, 1, 2, 3].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r]);
        [c[0] / c[3], c[1] / c[3], c[2] / c[3]]
    };
    let (red, green, white) = ([255, 0, 0, 255], [0, 255, 0, 255], [255, 255, 255, 255]);
    let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3], col| {
        [(clip(a), col), (clip(b), col), (clip(c), col)]
    };
    let mut v = Vec::new();
    v.extend(tri(
        [100.0, -10.0, 30.0],
        [100.0, 10.0, 30.0],
        [100.0, 0.0, 60.0],
        red,
    )); // above
    v.extend(tri(
        [100.0, -10.0, -30.0],
        [100.0, 10.0, -30.0],
        [100.0, 0.0, -60.0],
        green,
    )); // below
    v.extend(tri(
        [100.0, 40.0, -10.0],
        [100.0, 40.0, 10.0],
        [100.0, 70.0, 0.0],
        white,
    )); // left
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
        &[],
    )
    .unwrap();
    let vb = Buffer::with_data(&gpu, vk::BufferUsageFlags::VERTEX_BUFFER, &vertices(&v)).unwrap();
    target
        .render(&gpu, [0.0, 0.0, 0.0, 1.0], |cmd| unsafe {
            gpu.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline.pipeline);
            gpu.device
                .cmd_bind_vertex_buffers(cmd, 0, &[vb.buffer], &[0]);
            gpu.device.cmd_draw(cmd, 9, 1, 0, 0);
        })
        .unwrap();
    let px = target.read_rgba(&gpu).unwrap();
    let find = |c: [u8; 4]| {
        let hits: Vec<(usize, usize)> = (0..(w * h) as usize)
            .filter(|i| px[i * 4..][..4] == c)
            .map(|i| (i % w as usize, i / w as usize))
            .collect();
        assert!(!hits.is_empty(), "{c:?} not drawn");
        let n = hits.len();
        (
            hits.iter().map(|p| p.0).sum::<usize>() / n,
            hits.iter().map(|p| p.1).sum::<usize>() / n,
        )
    };
    let (red_at, green_at, white_at) = (find(red), find(green), find(white));
    assert!(
        red_at.1 < 32 && green_at.1 > 32,
        "up must be at the top: red {red_at:?}, green {green_at:?}"
    );
    assert!(
        white_at.0 < 32,
        "+y (left) must be on the left: {white_at:?}"
    );
}
