use super::support::*;

#[tokio::test]
#[ignore = "requires the pinned Prime Agent executable and IPython"]
async fn prime_agent_ipython_completes_a_round_trip() {
    let workspace = tempfile::tempdir().expect("workspace should exist");
    let output_path = workspace.path().join("prime-output.txt");
    let output_path_literal = serde_json::to_string(&output_path.to_string_lossy())
        .expect("Prime output path should serialize as a JSON string literal");
    run_round_trip(
        "prime-agent",
        [
            "--mode",
            "json",
            "--print",
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-context-files",
            "--tools",
            "ipython",
            "Complete the deterministic native tool conformance sequence.",
        ],
        &[("PI_OFFLINE", "1")],
        &workspace,
        vec![call(
            "ipython",
            json!({
                "code": format!(
                    "from pathlib import Path; output_path = Path({output_path_literal}); output_path.write_text('PRIME_IPYTHON_OK', encoding='utf-8'); output_path.read_text(encoding='utf-8')"
                )
            }),
        )],
        &[],
        "NAN_HARNESS_PRIME_TOOLS_OK",
    )
    .await;
    assert_file(workspace.path(), "prime-output.txt", "PRIME_IPYTHON_OK");
}
