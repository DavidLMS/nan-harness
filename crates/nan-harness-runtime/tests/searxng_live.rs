//! Explicit operator check; downloads and executes the pinned upstream source.
use nan_harness_runtime::search_supervisor::LocalSearxngSpec;
use nan_harness_runtime::searxng::{
    ProcessSearxngCommandExecutor, SearxngInstallPaths, SearxngInstallRequest, SearxngPlatform,
    SearxngSetupPlan, SearxngSourceMetadata, cleanup_owned_searxng_install,
    execute_searxng_install_plan, plan_searxng_installation,
};
use nan_harness_runtime::{SearchSupervisor, SearxngConfig};
use std::time::Duration;

#[tokio::test]
#[ignore = "downloads SearXNG and Python dependencies; requires Python, tar, network, and free port 8888"]
async fn local_install_serves_json_and_releases_the_owned_process() {
    let port = std::net::TcpListener::bind("127.0.0.1:8888").expect("port must be free");
    drop(port);
    let directory = tempfile::tempdir().expect("isolated installation");
    let paths = SearxngInstallPaths::under(directory.path().join("searxng"));
    let source = SearxngSourceMetadata::official();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_mins(2))
        .build()
        .expect("HTTP client");
    let archive = client
        .get(&source.archive_url)
        .send()
        .await
        .expect("download")
        .error_for_status()
        .expect("archive status")
        .bytes()
        .await
        .expect("archive bytes");
    let plan = plan_searxng_installation(
        SearxngInstallRequest::with_python_bootstrap(),
        SearxngPlatform::current(),
        paths.clone(),
        source,
    )
    .expect("plan");
    execute_searxng_install_plan(&plan, &archive, &ProcessSearxngCommandExecutor).expect("install");
    let SearxngSetupPlan::Setup(plan) = plan else {
        panic!("setup plan")
    };
    let endpoint = SearxngConfig::local("http://127.0.0.1:8888").expect("endpoint");
    let supervisor = SearchSupervisor::new(Some(
        LocalSearxngSpec::new(paths.root(), endpoint, plan.runtime_command()).expect("spec"),
    ))
    .expect("supervisor");
    let first = supervisor
        .acquire()
        .await
        .expect("startup")
        .expect("first lease");
    let second = supervisor
        .acquire()
        .await
        .expect("shared startup")
        .expect("second lease");
    drop(first);
    let response = client
        .get("http://127.0.0.1:8888/search?q=SearXNG&format=json&engines=wikipedia")
        .send()
        .await
        .expect("search")
        .error_for_status()
        .expect("JSON search enabled");
    let payload: serde_json::Value = response.json().await.expect("search JSON");
    assert!(payload["results"].is_array());
    drop(second);
    drop(supervisor);
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if tokio::net::TcpStream::connect("127.0.0.1:8888")
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("owned process releases listener");
    cleanup_owned_searxng_install(&paths).expect("remove installation");
    assert!(!paths.active().exists());
}
