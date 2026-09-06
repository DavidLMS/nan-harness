use super::{ManagedSearchStatus, search_confirmation_message};
use nan_harness_core::{HarnessKind, WebSearchPolicy};

fn message(harness: HarnessKind, policy: WebSearchPolicy, managed: bool) -> &'static str {
    search_confirmation_message(harness, ManagedSearchStatus { policy, managed })
}

#[test]
fn confirmation_message_preserves_or_explains_search_policy_distinctions() {
    let cases = [
        (
            "disabled preserves existing search",
            HarnessKind::Cline,
            WebSearchPolicy::Disabled,
            false,
            "will not be added; existing search configuration will be preserved",
        ),
        (
            "Aider reports unsupported fallback",
            HarnessKind::Aider,
            WebSearchPolicy::Auto,
            false,
            "does not support the NaN web search fallback",
        ),
        (
            "Pi installs runtime-aware fallback",
            HarnessKind::Pi,
            WebSearchPolicy::Auto,
            true,
            "runtime-aware NaN web search fallback will be installed",
        ),
        (
            "Prime installs runtime-aware fallback",
            HarnessKind::PrimeAgent,
            WebSearchPolicy::Auto,
            true,
            "runtime-aware NaN web search fallback will be installed",
        ),
        (
            "Pi force overrides package search",
            HarnessKind::Pi,
            WebSearchPolicy::Force,
            true,
            "will replace any package-provided web_search tool",
        ),
        (
            "Prime force overrides package search",
            HarnessKind::PrimeAgent,
            WebSearchPolicy::Force,
            true,
            "will replace any package-provided web_search tool",
        ),
        (
            "OMP prefers authenticated native search",
            HarnessKind::Omp,
            WebSearchPolicy::Auto,
            true,
            "authenticated native OMP search provider will be preferred",
        ),
        (
            "OMP force overrides native search",
            HarnessKind::Omp,
            WebSearchPolicy::Force,
            true,
            "will replace OMP's native web_search provider",
        ),
        (
            "generic auto adds absent fallback",
            HarnessKind::Cline,
            WebSearchPolicy::Auto,
            true,
            "No other web search provider was detected",
        ),
        (
            "generic auto preserves detected search",
            HarnessKind::Cline,
            WebSearchPolicy::Auto,
            false,
            "existing web search configuration was detected",
        ),
        (
            "generic force adds search despite another provider",
            HarnessKind::Cline,
            WebSearchPolicy::Force,
            true,
            "will be added even if another provider is configured",
        ),
        (
            "force leaves existing NaN entry untouched",
            HarnessKind::Cline,
            WebSearchPolicy::Force,
            false,
            "already configured, so nan-harness will leave that entry untouched",
        ),
    ];

    for (name, harness, policy, managed, expected) in cases {
        let actual = message(harness, policy, managed);
        assert!(actual.contains(expected), "{name}: {actual}");
    }
}
