use nan_harness_core::HarnessKind;
use nan_harness_test_support::conformance::PublishedConformanceRunner;

#[tokio::test]
#[ignore = "requires the pinned official source-built ZCode executable"]
async fn zcode_published_conformance_checks_inventory_tools_and_sentinel() {
    let report =
        PublishedConformanceRunner::new(env!("CARGO_BIN_EXE_nan-harness"), HarnessKind::ZCode)
            .run()
            .await
            .expect("published conformance should produce a safe report");
    assert!(report.is_success(), "{report:?}");
}
