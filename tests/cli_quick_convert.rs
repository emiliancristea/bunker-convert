use assert_cmd::Command;
use image::{ImageBuffer, Rgba};
use std::io::Write;
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

const ANNEX_B_SAMPLE: &[u8] = &[
    0x00, 0x00, 0x01, 0x67, 0x42, 0xE0, 0x1E, 0x8D, 0x68, 0x50, 0x1E, 0xD8, 0x08, 0x80, 0x00, 0x00,
    0x01, 0x68, 0xCE, 0x06, 0xE2, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x21, 0xA0,
];

#[test]
fn quick_convert_handles_h264_inputs() {
    let temp = tempdir().unwrap();
    let input_path = temp.path().join("clip.h264");
    let mut file = std::fs::File::create(&input_path).unwrap();
    file.write_all(ANNEX_B_SAMPLE).unwrap();

    let input_arg = input_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("input name");

    Command::cargo_bin("bunker-convert")
        .expect("binary present")
        .current_dir(temp.path())
        .args([input_arg, "to", "mp4"])
        .assert()
        .success();

    assert!(temp.path().join("clip.mp4").is_file());
}
