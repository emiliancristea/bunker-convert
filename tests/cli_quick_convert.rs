use assert_cmd::Command;
use image::{ImageBuffer, Rgba};
use tempfile::tempdir;

fn write_sample_image(path: &std::path::Path) {
    let img = ImageBuffer::from_fn(16, 16, |x, y| {
        let r = (x * 16) as u8;
        let g = (y * 16) as u8;
        let b = ((x + y) * 8) as u8;
        Rgba([r, g, b, 255])
    });
    img.save(path).expect("failed to write sample image");
}

#[test]
fn quick_convert_produces_output_in_current_directory() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("input.png");
    write_sample_image(&input_path);

    let input_arg = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("input name");

    Command::cargo_bin("bunker-convert")
        .expect("binary present")
        .current_dir(temp.path())
        .args([input_arg, "webp"])
        .assert()
        .success();

    assert!(temp.path().join("input.webp").is_file());
}
