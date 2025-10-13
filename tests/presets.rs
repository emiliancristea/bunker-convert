use bunker_convert::presets::generate_preset;
use std::fs;
use tempfile::tempdir;

#[test]
fn generate_web_preset_writes_file() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("web.yaml");
    let generated = generate_preset("web", &path).expect("preset generation");
    assert!(generated.exists());
    let contents = fs::read_to_string(&generated).expect("read preset");
    assert!(contents.contains("stage: encode"));
    assert!(contents.contains("format: webp"));
}
