use super::*;

#[test]
fn reset_reuses_layers_without_leaking_old_content() {
    let mut scene = Scene::new(Rgba::BLACK);
    scene.push_layer(None).quad(Quad::filled(Rect::new(0.0, 0.0, 1.0, 1.0), Rgba::WHITE));
    scene.push_layer(None);
    let capacity = scene.layers[0].quads.capacity();

    scene.reset(Rgba::WHITE);
    assert!(scene.layers().is_empty());

    let clip = Some(Rect::new(0.0, 0.0, 5.0, 5.0));
    let layer = scene.push_layer(clip);
    assert!(layer.quads.is_empty());
    assert_eq!(layer.clip, clip);
    assert_eq!(scene.layers().len(), 1);
    assert_eq!(scene.layers[0].quads.capacity(), capacity);
    assert_eq!(scene.quad_count(), 0);
}

#[test]
fn gpu_only_images_give_up_their_pixels_once_uploaded() {
    let kept = ImageData::new(1, 1, vec![1, 2, 3, 4]);
    kept.uploaded();
    assert_eq!(kept.memory(), 4);
    let image = ImageData::gpu_only(1, 1, vec![1, 2, 3, 4]);
    assert_eq!(image.with_pixels(<[u8]>::to_vec), Some(vec![1, 2, 3, 4]));
    image.uploaded();
    assert_eq!((image.memory(), image.is_lost()), (0, false));
    // The texture freed: the owner must decode it again.
    image.evicted();
    assert!(image.is_lost() && !kept.is_lost());
}
