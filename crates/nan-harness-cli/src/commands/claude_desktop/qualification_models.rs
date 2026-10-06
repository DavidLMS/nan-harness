//! Positive authenticated local-catalog observations, never upstream discovery.
use super::{DesktopPaths, RunningClaudeDesktopBridge};
use nan_harness_runtime::BridgeActivity;
use serde::Serialize;
use std::path::PathBuf;
use tokio::sync::{broadcast, oneshot};

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Counts {
    authenticated_models_count: u8,
    complete: bool,
}
impl Counts {
    fn record(&mut self, activity: &BridgeActivity) {
        if matches!(activity, BridgeActivity::AuthenticatedModels) {
            if self.authenticated_models_count < 32 {
                self.authenticated_models_count += 1;
            } else {
                self.complete = false;
            }
        }
    }
}

pub(super) struct Observation {
    directory: PathBuf,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Counts>,
}
impl Observation {
    pub(super) fn start(paths: &DesktopPaths, bridge: &RunningClaudeDesktopBridge) -> Option<Self> {
        let directory = super::qualification_config::observation_directory(paths)?;
        let receiver = bridge.subscribe_activities();
        let (stop, stopping) = oneshot::channel();
        Some(Self {
            directory,
            stop,
            task: tokio::spawn(collect(receiver, stopping)),
        })
    }
    pub(super) async fn finish(self) {
        let _ = self.stop.send(());
        let Ok(counts) = self.task.await else {
            return;
        };
        if !super::qualification_config::private_directory(&self.directory) {
            return;
        }
        let name = format!("claude-model-discovery-{}.json", std::process::id());
        let Ok(mut file) = nan_harness_private_fs::open_private_new(&self.directory.join(name))
        else {
            return;
        };
        let value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-model-discovery",
            "diagnosticsOnly":true,"authenticatedModelsCount":counts.authenticated_models_count,
            "complete":counts.complete,"modelDiscoverySeen":(counts.authenticated_models_count > 0).then_some(true)});
        let _ = serde_json::to_writer(&mut file, &value);
    }
}

async fn collect(
    mut receiver: broadcast::Receiver<BridgeActivity>,
    mut stop: oneshot::Receiver<()>,
) -> Counts {
    let mut counts = Counts {
        complete: true,
        ..Counts::default()
    };
    loop {
        tokio::select! {
            _ = &mut stop => break,
            result = receiver.recv() => match result {
                Ok(activity) => counts.record(&activity),
                Err(broadcast::error::RecvError::Lagged(_)) => counts.complete = false,
                Err(broadcast::error::RecvError::Closed) => return counts,
            }
        }
    }
    loop {
        match receiver.try_recv() {
            Ok(activity) => counts.record(&activity),
            Err(broadcast::error::TryRecvError::Lagged(_)) => counts.complete = false,
            Err(_) => return counts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn only_models_are_counted_and_finish_drains_pending_events() {
        let (sender, receiver) = broadcast::channel(64);
        let (stop, stopping) = oneshot::channel();
        sender.send(BridgeActivity::AuthenticatedClient).unwrap();
        sender.send(BridgeActivity::AuthenticatedModels).unwrap();
        stop.send(()).unwrap();
        let counts = collect(receiver, stopping).await;
        assert_eq!(counts.authenticated_models_count, 1);
        assert!(counts.complete);
    }
    #[tokio::test]
    async fn lag_and_saturation_never_become_complete_or_negative_proof() {
        let (sender, receiver) = broadcast::channel(1);
        let (stop, stopping) = oneshot::channel();
        for _ in 0..3 {
            sender.send(BridgeActivity::AuthenticatedModels).unwrap();
        }
        stop.send(()).unwrap();
        let mut counts = collect(receiver, stopping).await;
        assert!(!counts.complete);
        for _ in 0..40 {
            counts.record(&BridgeActivity::AuthenticatedModels);
        }
        assert_eq!(counts.authenticated_models_count, 32);
        assert!(!counts.complete);
    }
}
