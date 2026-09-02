use super::{
    CodingModelProfile, ConfigurationError, ConfigurationManager, HarnessKind, IntegrationChange,
    PersistentIntegration, RemovalOutcome,
};

impl ConfigurationManager {
    pub(crate) fn configure_catalogs(
        &self,
        harness: HarnessKind,
        models: &[CodingModelProfile],
        provider_base_url: &str,
        api_key: &str,
        search_managed: bool,
    ) -> Result<Option<IntegrationChange>, ConfigurationError> {
        let change = match harness {
            HarnessKind::OpenCode => Some(self.legacy.configure_opencode(
                models,
                provider_base_url,
                search_managed.then_some((api_key, provider_base_url)),
            )?),
            HarnessKind::QwenCode => {
                Some(self.legacy.configure_qwen_code(models, provider_base_url)?)
            }
            HarnessKind::DeepSeekHarness => Some(
                self.legacy
                    .configure_deepseek_harness(models, provider_base_url)?,
            ),
            HarnessKind::Aider => Some(self.legacy.configure_aider(models, provider_base_url)?),
            _ => None,
        };
        Ok(change)
    }

    pub(crate) fn remove_legacy(
        &self,
        harness: HarnessKind,
    ) -> Result<RemovalOutcome, ConfigurationError> {
        let outcome = match harness {
            HarnessKind::OpenCode => self.legacy.unpersist_opencode()?,
            HarnessKind::Pi => self.legacy.unpersist_pi()?,
            HarnessKind::PrimeAgent => self.legacy.unpersist_prime_agent()?,
            HarnessKind::QwenCode => self.legacy.unpersist_qwen_code()?,
            HarnessKind::DeepSeekHarness => self.legacy.unpersist_deepseek_harness()?,
            HarnessKind::Aider => self.legacy.unpersist_aider()?,
            _ => RemovalOutcome::NotConfigured,
        };
        Ok(outcome)
    }

    pub(crate) fn legacy_is_active(&self, harness: HarnessKind) -> bool {
        match harness {
            HarnessKind::OpenCode => self.legacy.opencode_is_active(),
            HarnessKind::QwenCode => self.legacy.qwen_code_is_active(),
            HarnessKind::DeepSeekHarness => self.legacy.deepseek_harness_is_active(),
            HarnessKind::Aider => self.legacy.aider_is_active(),
            _ => true,
        }
    }
}

pub(crate) const fn legacy_harness(integration: PersistentIntegration) -> HarnessKind {
    match integration {
        PersistentIntegration::OpenCode => HarnessKind::OpenCode,
        PersistentIntegration::Pi => HarnessKind::Pi,
        PersistentIntegration::PrimeAgent => HarnessKind::PrimeAgent,
        PersistentIntegration::QwenCode => HarnessKind::QwenCode,
        PersistentIntegration::DeepSeekHarness => HarnessKind::DeepSeekHarness,
        PersistentIntegration::Aider => HarnessKind::Aider,
    }
}

pub(crate) const fn catalog_integration(harness: HarnessKind) -> Option<PersistentIntegration> {
    match harness {
        HarnessKind::OpenCode => Some(PersistentIntegration::OpenCode),
        HarnessKind::QwenCode => Some(PersistentIntegration::QwenCode),
        HarnessKind::DeepSeekHarness => Some(PersistentIntegration::DeepSeekHarness),
        HarnessKind::Aider => Some(PersistentIntegration::Aider),
        _ => None,
    }
}
