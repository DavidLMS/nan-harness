use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DetectionSignal {
    None,
    ManagedNan,
    External,
    Collision(PathBuf),
}

impl DetectionSignal {
    pub(super) fn combine(self, other: Self) -> Self {
        match (&self, &other) {
            (Self::Collision(_), _) => self,
            (_, Self::Collision(_)) => other,
            (Self::ManagedNan, _) => self,
            (_, Self::ManagedNan) => other,
            (Self::External, _) => self,
            (_, Self::External) => other,
            (Self::None, Self::None) => Self::None,
        }
    }
}
