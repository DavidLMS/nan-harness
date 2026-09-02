#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchConfiguration {
    None,
    ManagedNan,
    External,
    Unsupported,
}
