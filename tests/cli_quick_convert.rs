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
        .args([input_arg, "to", "webp"])
        .assert()
        .success();

    assert!(temp.path().join("input.webp").is_file());
}

#[test]
fn quick_convert_handles_multiple_inputs() {
    let temp = tempdir().unwrap();
    let first = temp.path().join("first.png");
    let second = temp.path().join("second.png");
    write_sample_image(&first);
    write_sample_image(&second);

    let first_arg = first.file_name().and_then(|n| n.to_str()).unwrap();
    let second_arg = second.file_name().and_then(|n| n.to_str()).unwrap();

    Command::cargo_bin("bunker-convert")
        .expect("binary present")
        .current_dir(temp.path())
        .args([first_arg, second_arg, "to", "webp"])
        .assert()
        .success();

    assert!(temp.path().join("first.webp").is_file());
    assert!(temp.path().join("second.webp").is_file());
}

#[test]
fn quick_convert_supports_custom_output_directory() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("image.png");
    write_sample_image(&input_path);

    let input_arg = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("input name");

    let output_dir = temp.path().join("converted");
    let output_arg = output_dir
        .file_name()
        .and_then(|name| name.to_str())
        .expect("output dir name");

    Command::cargo_bin("bunker-convert")
        .expect("binary present")
        .current_dir(temp.path())
        .args([input_arg, "to", "webp", "to", output_arg])
        .assert()
        .success();

    assert!(output_dir.join("image.webp").is_file());
}
