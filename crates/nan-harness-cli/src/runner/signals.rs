#[allow(clippy::wildcard_imports)]
use super::*;

pub(crate) fn install_signal_handlers(
    cancellation: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let Ok(mut interrupt) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            else {
                loop {
                    if tokio::signal::ctrl_c().await.is_err() {
                        break;
                    }
                    cancellation.cancel(SignalKind::Interrupt);
                }
                return;
            };
            let Ok(mut terminate) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            else {
                loop {
                    if interrupt.recv().await.is_none() {
                        break;
                    }
                    cancellation.cancel(SignalKind::Interrupt);
                }
                return;
            };
            loop {
                tokio::select! {
                    value = interrupt.recv() => {
                        if value.is_some() {
                            cancellation.cancel(SignalKind::Interrupt);
                        } else {
                            break;
                        }
                    }
                    value = terminate.recv() => {
                        if value.is_some() {
                            cancellation.cancel(SignalKind::Terminate);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        #[cfg(not(unix))]
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                break;
            }
            cancellation.cancel(SignalKind::Interrupt);
        }
    })
}
