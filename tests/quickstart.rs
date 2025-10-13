use bunker_convert::pipeline::StageRegistry;
use bunker_convert::recipe::Recipe;
use bunker_convert::stages;
use bunker_convert::validation::validate_recipe;
use std::path::Path;

#[test]
fn quickstart_recipe_is_valid() {
    let recipe = Recipe::load(Path::new("recipes/quickstart-webp.yaml"))
        .expect("quickstart recipe should load");
    let mut registry = StageRegistry::new();
    stages::register_defaults(&mut registry);
    let report = validate_recipe(&recipe, &registry);
    assert!(
        report.is_ok(),
        "quickstart recipe should pass validation: {:?}",
        report.errors
    );
}
