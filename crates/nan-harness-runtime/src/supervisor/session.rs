use super::RuntimeError;
use crate::config::ResolvedConfig;
use nan_harness_bridge::{BridgeError, discover_coding_models};
use nan_harness_core::{CodingModelProfile, SecretRef, SecretStore, SecretValue};
use std::sync::Arc;
use tokio::sync::OnceCell;

#[derive(Debug)]
pub struct LaunchSession<'a> {
    pub(super) config: &'a ResolvedConfig,
    model_catalog: OnceCell<Vec<CodingModelProfile>>,
}

impl<'a> LaunchSession<'a> {
    #[must_use]
    pub const fn new(config: &'a ResolvedConfig) -> Self {
        Self {
            config,
            model_catalog: OnceCell::const_new(),
        }
    }

    #[must_use]
    pub fn with_model_catalog(
        config: &'a ResolvedConfig,
        model_catalog: Vec<CodingModelProfile>,
    ) -> Self {
        Self {
            config,
            model_catalog: OnceCell::new_with(Some(model_catalog)),
        }
    }

    /// Returns the credential-bound catalog snapshot for this launch session.
    ///
    /// Repeated calls reuse the same bounded discovery result.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when the provider credential cannot be resolved or model
    /// discovery fails.
    pub async fn model_catalog(&self) -> Result<&[CodingModelProfile], RuntimeError> {
        let models = self
            .model_catalog
            .get_or_try_init(|| async {
                let provider_api_key =
                    copy_secret(&self.config.secrets, &self.config.provider_credential_ref)?;
                discover_coding_models(&self.config.provider_base_url, provider_api_key)
                    .await
                    .map_err(RuntimeError::Bridge)
            })
            .await?;
        Ok(models.as_slice())
    }
}

pub(super) fn copy_secret(
    secrets: &SecretStore,
    reference: &SecretRef,
) -> Result<Arc<SecretValue>, RuntimeError> {
    secrets
        .with_secret(reference, |value| SecretValue::new(value.to_owned()))
        .map_err(RuntimeError::Secret)?
        .map(Arc::new)
        .map_err(RuntimeError::Secret)
}

pub(super) fn validate_selected_model(
    models: &[CodingModelProfile],
    selected_model: &str,
) -> Result<(), BridgeError> {
    if models.is_empty() {
        return Err(BridgeError::NoCompatibleModels);
    }
    if models.iter().any(|model| model.id == selected_model) {
        Ok(())
    } else {
        Err(BridgeError::SelectedModelUnavailable {
            model: selected_model.to_owned(),
            available: models.iter().map(|model| model.id.clone()).collect(),
        })
    }
}
