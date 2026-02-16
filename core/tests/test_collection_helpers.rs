use spotify_player_core::cache::images::save_cover_art;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_save_cover_art_with_provided_bytes() {
    let temp_dir = TempDir::new().unwrap();
    let m3u_path = temp_dir.path().join("playlist.m3u8");

    // Create a fake JPEG (just some bytes)
    let cover_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header

    let result = save_cover_art(&m3u_path, Some(cover_bytes.clone()), &[]);
    assert!(result.is_ok());

    let cover_path = temp_dir.path().join("cover.jpg");
    assert!(cover_path.exists());

    let saved_bytes = std::fs::read(&cover_path).unwrap();
    assert_eq!(saved_bytes, cover_bytes);
}

#[test]
fn test_save_cover_art_with_collage() {
    let temp_dir = TempDir::new().unwrap();
    let m3u_path = temp_dir.path().join("playlist.m3u8");

    // Create test images for collage
    fn create_test_image(r: u8, g: u8, b: u8) -> Vec<u8> {
        use image::{ImageFormat, RgbImage};
        use std::io::Cursor;

        let img = RgbImage::from_fn(100, 100, |_, _| image::Rgb([r, g, b]));
        let mut buffer = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buffer, ImageFormat::Jpeg)
            .unwrap();
        buffer.into_inner()
    }

    let album_covers = vec![create_test_image(255, 0, 0), create_test_image(0, 255, 0)];

    let result = save_cover_art(&m3u_path, None, &album_covers);
    assert!(result.is_ok());

    let cover_path = temp_dir.path().join("cover.jpg");
    assert!(cover_path.exists());

    // Verify it's a valid image
    let saved_bytes = std::fs::read(&cover_path).unwrap();
    let img = image::load_from_memory(&saved_bytes);
    assert!(img.is_ok());
}

#[test]
fn test_save_cover_art_with_no_data() {
    let temp_dir = TempDir::new().unwrap();
    let m3u_path = temp_dir.path().join("playlist.m3u8");

    let result = save_cover_art(&m3u_path, None, &[]);
    assert!(result.is_ok());

    // No cover.jpg should be created
    let cover_path = temp_dir.path().join("cover.jpg");
    assert!(!cover_path.exists());
}

#[test]
fn test_save_cover_art_without_parent_dir() {
    // Path with no parent should return error
    let m3u_path = PathBuf::from("/");

    let result = save_cover_art(&m3u_path, Some(vec![1, 2, 3]), &[]);
    assert!(result.is_err());
}

#[test]
fn test_save_cover_art_prefers_provided_over_collage() {
    let temp_dir = TempDir::new().unwrap();
    let m3u_path = temp_dir.path().join("playlist.m3u8");

    let provided_bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];

    fn create_test_image() -> Vec<u8> {
        use image::{ImageFormat, RgbImage};
        use std::io::Cursor;

        let img = RgbImage::from_fn(50, 50, |_, _| image::Rgb([100, 100, 100]));
        let mut buffer = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buffer, ImageFormat::Jpeg)
            .unwrap();
        buffer.into_inner()
    }

    let album_covers = vec![create_test_image()];

    let result = save_cover_art(&m3u_path, Some(provided_bytes.clone()), &album_covers);
    assert!(result.is_ok());

    let cover_path = temp_dir.path().join("cover.jpg");
    let saved_bytes = std::fs::read(&cover_path).unwrap();

    // Should use provided bytes, not collage
    assert_eq!(saved_bytes, provided_bytes);
}
