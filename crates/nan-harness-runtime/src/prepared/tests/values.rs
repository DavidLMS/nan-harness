use crate::prepared::values::{join_goose_config_paths, render_nan_search_blocks};
use std::path::PathBuf;

#[test]
fn search_blocks_render_atomically() {
    let template = "before{runtime:nan_search:begin},search{runtime:nan_search:end}after";

    assert_eq!(
        render_nan_search_blocks(template, true).expect("enabled block"),
        "before,searchafter"
    );
    assert_eq!(
        render_nan_search_blocks(template, false).expect("disabled block"),
        "beforeafter"
    );
    assert!(render_nan_search_blocks("{runtime:nan_search:begin}open", true).is_err());
    assert!(
        render_nan_search_blocks(
            "{runtime:nan_search:begin}{runtime:nan_search:begin}nested{runtime:nan_search:end}{runtime:nan_search:end}",
            true,
        )
        .is_err()
    );
}

#[test]
fn goose_search_config_preserves_existing_additional_layers() {
    let existing =
        std::env::join_paths(["first.yaml", "second.yaml"]).expect("fixture paths should join");
    let joined = join_goose_config_paths(Some(existing.as_os_str()), "nan-search.yaml")
        .expect("Goose config paths should join");
    let paths = std::env::split_paths(&joined).collect::<Vec<_>>();

    assert_eq!(
        paths,
        ["first.yaml", "second.yaml", "nan-search.yaml"]
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
    );
}
