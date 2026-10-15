use reqwest::{Client, Url};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Deserialize)]
struct Registration {
    url: String,
    pid: u32,
    version: String,
    password: String,
}

#[derive(Deserialize)]
struct ServiceInfo {
    pid: u32,
    version: String,
}

pub(super) async fn notify(home: &Path) {
    let directory =
        std::env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
    if reload(&directory.join("opencode/service.json"))
        .await
        .is_err()
    {
        eprintln!(
            "{}",
            nan_harness_i18n::messages::opencode_config_reload_pending(nan_harness_i18n::locale())
        );
    }
}

// Use the published service registration and HTTP API. The CLI's `reload`
// command ensures a service exists and can replace it, which is inappropriate
// when applying a native configuration to an already running service.
async fn reload(path: &Path) -> Result<(), ()> {
    let contents = match std::fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(()),
    };
    let registration: Registration = serde_json::from_slice(&contents).map_err(|_| ())?;
    let url = local_endpoint(&registration)?;
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| ())?;
    let info = client
        .get(url.join("/api/info").map_err(|_| ())?)
        .basic_auth("opencode", Some(&registration.password))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?;
    let info: ServiceInfo = serde_json::from_slice(&bounded_body(info).await?).map_err(|_| ())?;
    if info.pid != registration.pid || info.version != registration.version {
        return Err(());
    }
    // A registration replaced during the probe no longer authorizes a reload.
    if std::fs::read(path).map_err(|_| ())? != contents {
        return Err(());
    }
    let response = client
        .post(url.join("/api/location/reload").map_err(|_| ())?)
        .basic_auth("opencode", Some(&registration.password))
        .send()
        .await
        .map_err(|_| ())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(())
    }
}

fn local_endpoint(registration: &Registration) -> Result<Url, ()> {
    let url = Url::parse(&registration.url).map_err(|_| ())?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    if !loopback
        || url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || registration.pid == 0
        || !registration.version.starts_with("2.")
        || registration.password.is_empty()
    {
        return Err(());
    }
    Ok(url)
}

async fn bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, ()> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if body.len() + chunk.len() > 16 * 1024 {
            return Err(());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
#[path = "tests/opencode_reload.rs"]
mod tests;
