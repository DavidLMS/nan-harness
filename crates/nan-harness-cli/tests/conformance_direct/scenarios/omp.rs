use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned OMP executable"]
async fn omp_native_write_completes_a_round_trip() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let workspace_path = workspace.path().to_string_lossy();
    run_round_trip(
        "omp",
        [
            "--mode",
            "json",
            "--print",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-rules",
            "--no-lsp",
            "--no-title",
            "--tools",
            "write",
            "Complete the deterministic native tool conformance sequence.",
        ],
        &[],
        &workspace,
        vec![call(
            "write",
            json!({
                "path": format!("{workspace_path}/omp-output.txt"),
                "content": "OMP_WRITE_OK\n"
            }),
        )],
        &[],
        "NAN_HARNESS_OMP_TOOLS_OK",
    )
    .await;

    assert_file(workspace.path(), "omp-output.txt", "OMP_WRITE_OK");
}

#[tokio::test]
#[ignore = "requires the pinned OMP executable"]
async fn omp_without_authenticated_search_falls_back_to_nan() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    run_round_trip(
        "omp",
        [
            "--mode",
            "json",
            "--print",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-rules",
            "--no-lsp",
            "--no-title",
            "--tools",
            "web_search",
            "Complete the deterministic NaN search fallback check.",
        ],
        &[],
        &workspace,
        vec![call(
            "web_search",
            json!({"query": "nan-harness OMP conformance", "limit": 1}),
        )],
        &["web_search"],
        "NAN_HARNESS_OMP_SEARCH_OK",
    )
    .await;
}
