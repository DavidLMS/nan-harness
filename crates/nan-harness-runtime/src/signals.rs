use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Interrupt,
    Terminate,
}

impl SignalKind {
    #[must_use]
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::Interrupt => 130,
            Self::Terminate => 143,
        }
    }

    const fn encoded(self) -> u8 {
        match self {
            Self::Interrupt => 1,
            Self::Terminate => 2,
        }
    }

    const fn from_encoded(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Interrupt),
            2 => Some(Self::Terminate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CancellationToken {
    signal: Arc<AtomicU8>,
    wake: tokio_util::sync::CancellationToken,
    force: tokio_util::sync::CancellationToken,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self {
            signal: Arc::new(AtomicU8::new(0)),
            wake: tokio_util::sync::CancellationToken::new(),
            force: tokio_util::sync::CancellationToken::new(),
        }
    }
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self, signal: SignalKind) {
        if self
            .signal
            .compare_exchange(0, signal.encoded(), Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.wake.cancel();
        } else {
            self.force.cancel();
        }
    }

    #[must_use]
    pub fn signal(&self) -> Option<SignalKind> {
        SignalKind::from_encoded(self.signal.load(Ordering::Acquire))
    }

    pub async fn cancelled(&self) -> SignalKind {
        self.wake.cancelled().await;
        self.signal().unwrap_or(SignalKind::Interrupt)
    }

    pub async fn force_cancelled(&self) {
        self.force.cancelled().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{CancellationToken, SignalKind};

    #[tokio::test]
    async fn first_signal_is_preserved_and_repeat_requests_force_shutdown() {
        let cancellation = CancellationToken::new();

        assert_eq!(cancellation.signal(), None);
        assert!(!cancellation.force.is_cancelled());

        cancellation.cancel(SignalKind::Terminate);
        assert_eq!(cancellation.signal(), Some(SignalKind::Terminate));
        assert!(!cancellation.force.is_cancelled());
        assert_eq!(cancellation.cancelled().await, SignalKind::Terminate);

        cancellation.cancel(SignalKind::Interrupt);
        assert_eq!(cancellation.signal(), Some(SignalKind::Terminate));
        assert!(cancellation.force.is_cancelled());
        cancellation.force_cancelled().await;

        cancellation.cancel(SignalKind::Terminate);
        assert_eq!(cancellation.signal(), Some(SignalKind::Terminate));
        assert!(cancellation.force.is_cancelled());
    }
}
