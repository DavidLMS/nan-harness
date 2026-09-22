use super::{
    CodingModelProfile, ConfigurationError, ConfigurationManager, HarnessKind, IntegrationChange,
    PersistentIntegration, RemovalOutcome,
};
use crate::commands::persistence::PreparedFileChange;

impl ConfigurationManager {
    pub(crate) fn prepare_catalogs(
        &self,
        harness: HarnessKind,
        models: &[CodingModelProfile],
        provider_base_url: &str,
        _api_key: &str,
        search_managed: bool,
    ) -> Result<(Vec<PreparedFileChange>, Option<IntegrationChange>), ConfigurationError> {
        let change = match harness {
            HarnessKind::OpenCode => Some(self.legacy.prepare_opencode(
                models,
                provider_base_url,
                search_managed,
            )?),
            HarnessKind::QwenCode => {
                Some(self.legacy.prepare_qwen_code(models, provider_base_url)?)
            }
            HarnessKind::DeepSeekHarness => Some(
                self.legacy
                    .prepare_deepseek_harness(models, provider_base_url)?,
            ),
            HarnessKind::Aider => Some(self.legacy.prepare_aider(models, provider_base_url)?),
            _ => None,
        };
        Ok(change.map_or_else(
            || (Vec::new(), None),
            |(files, change)| (files, Some(change)),
        ))
    }

    pub(crate) fn prepare_remove_legacy(
        &self,
        harness: HarnessKind,
    ) -> Result<(Vec<PreparedFileChange>, RemovalOutcome), ConfigurationError> {
        let outcome = match harness {
            HarnessKind::OpenCode => self.legacy.prepare_remove_opencode()?,
            HarnessKind::Pi => self.legacy.prepare_remove_pi()?,
            HarnessKind::PrimeAgent => self.legacy.prepare_remove_prime_agent()?,
            HarnessKind::QwenCode => self.legacy.prepare_remove_qwen_code()?,
            HarnessKind::DeepSeekHarness => self.legacy.prepare_remove_deepseek_harness()?,
            HarnessKind::Aider => self.legacy.prepare_remove_aider()?,
            _ => (Vec::new(), RemovalOutcome::NotConfigured),
        };
        Ok(outcome)
    }

    pub(crate) fn legacy_health(
        &self,
        harness: HarnessKind,
    ) -> Result<super::ConfigurationHealth, ConfigurationError> {
        let Some(integration) = catalog_integration(harness) else {
            return Ok(super::ConfigurationHealth::Active);
        };
        Ok(self
            .legacy
            .inspect_integration(integration)?
            .unwrap_or(super::ConfigurationHealth::Missing))
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
