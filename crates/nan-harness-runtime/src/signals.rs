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

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    signal: Arc<AtomicU8>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self, signal: SignalKind) {
        let _ =
            self.signal
                .compare_exchange(0, signal.encoded(), Ordering::AcqRel, Ordering::Acquire);
    }

    #[must_use]
    pub fn signal(&self) -> Option<SignalKind> {
        SignalKind::from_encoded(self.signal.load(Ordering::Acquire))
    }
}
