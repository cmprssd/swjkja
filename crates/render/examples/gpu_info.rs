//! Prints the picked device and the limits the renderer depends on.
use swjkja_render::vk;

fn main() {
    let gpu = swjkja_render::Gpu::new_headless().unwrap();
    let mut p12 = vk::PhysicalDeviceVulkan12Properties::default();
    let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut p12);
    unsafe {
        gpu.instance
            .get_physical_device_properties2(gpu.physical, &mut props2)
    };
    let l = props2.properties.limits;
    println!("{} ({:?})", gpu.name, gpu.kind);
    println!(
        "maxPerStageDescriptorSampledImages {}",
        l.max_per_stage_descriptor_sampled_images
    );
    println!("maxPerStageResources {}", l.max_per_stage_resources);
    println!("maxPushConstantsSize {}", l.max_push_constants_size);
    println!(
        "update-after-bind: maxPerStageDescriptorUpdateAfterBindSampledImages {}, maxPerStageUpdateAfterBindResources {}, maxDescriptorSetUpdateAfterBindSampledImages {}",
        p12.max_per_stage_descriptor_update_after_bind_sampled_images,
        p12.max_per_stage_update_after_bind_resources,
        p12.max_descriptor_set_update_after_bind_sampled_images
    );
    let mut f12 = vk::PhysicalDeviceVulkan12Features::default();
    let mut f2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut f12);
    unsafe {
        gpu.instance
            .get_physical_device_features2(gpu.physical, &mut f2)
    };
    println!(
        "descriptorBindingSampledImageUpdateAfterBind {}, descriptorBindingPartiallyBound {}",
        f12.descriptor_binding_sampled_image_update_after_bind,
        f12.descriptor_binding_partially_bound
    );
}
