use spotify_player::cache::images::create_cover_collage;
use image::{ImageFormat, RgbImage};
use std::io::Cursor;

fn create_test_image(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    let img = RgbImage::from_fn(width, height, |_, _| image::Rgb([r, g, b]));
    let mut buffer = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buffer, ImageFormat::Jpeg)
        .unwrap();
    buffer.into_inner()
}

#[test]
fn test_collage_with_four_images() {
    let images = vec![
        create_test_image(500, 500, 255, 0, 0),   // Red
        create_test_image(500, 500, 0, 255, 0),   // Green
        create_test_image(500, 500, 0, 0, 255),   // Blue
        create_test_image(500, 500, 255, 255, 0), // Yellow
    ];

    let result = create_cover_collage(&images);
    assert!(result.is_ok());

    let collage_bytes = result.unwrap();
    assert!(!collage_bytes.is_empty());

    // Verify it's a valid image
    let img = image::load_from_memory(&collage_bytes);
    assert!(img.is_ok());

    // Should be 600x600 (2x2 grid of 300x300 tiles)
    let img = img.unwrap();
    assert_eq!(img.width(), 600);
    assert_eq!(img.height(), 600);
}

#[test]
fn test_collage_with_single_image() {
    let images = vec![create_test_image(800, 800, 128, 128, 128)];

    let result = create_cover_collage(&images);
    assert!(result.is_ok());

    let collage_bytes = result.unwrap();
    let img = image::load_from_memory(&collage_bytes).unwrap();

    // Single image should be 300x300
    assert_eq!(img.width(), 300);
    assert_eq!(img.height(), 300);
}

#[test]
fn test_collage_with_three_images() {
    let images = vec![
        create_test_image(400, 400, 255, 0, 0),
        create_test_image(400, 400, 0, 255, 0),
        create_test_image(400, 400, 0, 0, 255),
    ];

    let result = create_cover_collage(&images);
    assert!(result.is_ok());

    let collage_bytes = result.unwrap();
    let img = image::load_from_memory(&collage_bytes).unwrap();

    // 3 images should still create 2x2 grid (600x600)
    assert_eq!(img.width(), 600);
    assert_eq!(img.height(), 600);
}

#[test]
fn test_collage_with_empty_list() {
    let images: Vec<Vec<u8>> = vec![];

    let result = create_cover_collage(&images);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No cover images"));
}

#[test]
fn test_collage_with_invalid_image() {
    let invalid_data = vec![0u8, 1, 2, 3, 4, 5]; // Not a valid image
    let valid_image = create_test_image(300, 300, 100, 100, 100);

    let images = vec![invalid_data, valid_image];

    // Should handle invalid image gracefully and create collage from valid ones
    let result = create_cover_collage(&images);
    assert!(result.is_ok());
}

#[test]
fn test_collage_with_different_sizes() {
    let images = vec![
        create_test_image(100, 100, 255, 0, 0),
        create_test_image(1000, 1000, 0, 255, 0),
        create_test_image(500, 300, 0, 0, 255),
        create_test_image(200, 800, 255, 255, 0),
    ];

    let result = create_cover_collage(&images);
    assert!(result.is_ok());

    // All should be resized to 300x300 and arranged in 600x600 grid
    let collage_bytes = result.unwrap();
    let img = image::load_from_memory(&collage_bytes).unwrap();
    assert_eq!(img.width(), 600);
    assert_eq!(img.height(), 600);
}
