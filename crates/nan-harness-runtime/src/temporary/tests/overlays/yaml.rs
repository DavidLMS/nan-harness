use crate::temporary::TemporaryWorkspace;
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, OverlayFile, OverlayFilePolicy, TemporaryArtifactMode,
    USER_HOME_PLACEHOLDER,
};
use std::fs;

#[test]
fn yaml_overlay_merges_maps_and_unions_plugin_lists() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let source = home.path().join(".hermes");
    fs::create_dir_all(&source).expect("Hermes source should exist");
    fs::write(
        source.join("config.yaml"),
        "plugins:\n  enabled:\n    - user/plugin\n  disabled:\n    - blocked/plugin\nweb:\n  extract_backend: tavily\ncustom:\n  tagged: !native user-value\n  literal: '00123'\n  template: '${USER_SETTING}'\n  original: &user_list [one, two]\n  alias: *user_list\n  multiline: |\n    first\n    second\n",
    )
    .expect("Hermes config fixture should exist");
    let overlays = [ConfigurationOverlay {
        id: "hermes-home".to_owned(),
        path_hint: "hermes".to_owned(),
        source_path: format!("{USER_HOME_PLACEHOLDER}/.hermes"),
        files: vec![OverlayFile {
            path: "config.yaml".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "plugins:\n  enabled:\n    - model-providers/nan\n    - web/nan\nweb:\n  search_backend: nan\n"
                .to_owned(),
            policy: OverlayFilePolicy::MergeYaml,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    }];

    let workspace =
        TemporaryWorkspace::materialize_with_home(&[], &overlays, home.path(), |_, content| {
            Ok(content.to_owned())
        })
        .expect("Hermes overlay should materialize");
    let merged: yaml_serde::Value = yaml_serde::from_str(
        &fs::read_to_string(
            workspace
                .path("hermes-home")
                .expect("overlay should exist")
                .join("config.yaml"),
        )
        .expect("merged Hermes config should be readable"),
    )
    .expect("merged Hermes config should be YAML");
    let enabled = merged["plugins"]["enabled"]
        .as_sequence()
        .expect("enabled plugins should be a list");

    assert_eq!(enabled.len(), 3);
    assert!(enabled.contains(&yaml_serde::Value::String("user/plugin".to_owned())));
    assert!(enabled.contains(&yaml_serde::Value::String("model-providers/nan".to_owned())));
    assert!(enabled.contains(&yaml_serde::Value::String("web/nan".to_owned())));
    assert_eq!(merged["plugins"]["disabled"][0], "blocked/plugin");
    assert_eq!(merged["web"]["extract_backend"], "tavily");
    assert_eq!(merged["web"]["search_backend"], "nan");
    let custom = &merged["custom"];
    let yaml_serde::Value::Tagged(tagged) = &custom["tagged"] else {
        panic!("native tags must survive managed overlays");
    };
    assert_eq!(tagged.tag.to_string(), "!native");
    assert_eq!(tagged.value, "user-value");
    assert_eq!(custom["literal"], "00123");
    assert_eq!(custom["template"], "${USER_SETTING}");
    assert_eq!(custom["alias"], custom["original"]);
    assert_eq!(custom["alias"][1], "two");
    assert_eq!(custom["multiline"], "first\nsecond\n");
}
