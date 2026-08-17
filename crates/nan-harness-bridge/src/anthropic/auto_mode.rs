use crate::error::ApiError;
use nan_harness_core::model::CLAUDE_GATEWAY_MODEL_PREFIX;
use nan_harness_core::{CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID};
use serde_json::{Map, Value, json};

const POLICY_MARKERS: [&str; 3] = [
    "You are a security monitor for autonomous AI coding agents.",
    "## Classification Process",
    "## Output Format",
];
const STAGE_ONE_MARKER: &str = "Stage 1 does NOT apply user intent or ALLOW exceptions";
const STAGE_TWO_MARKER: &str = "Review the classification process and follow it carefully";
const STAGE_ONE_MAX_TOKENS: u64 = 64;
const STAGE_TWO_MAX_TOKENS: u64 = 8_192;
const QWEN_STAGE_ONE_MAX_TOKENS: u64 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassifierStage {
    One,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestShape {
    ClassifierCandidate,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PolicyFingerprint {
    Qualified,
    Unknown,
}

pub(crate) struct RequestFingerprint<'a> {
    pub(crate) model: &'a str,
    pub(crate) max_tokens: Option<u64>,
    pub(crate) shape: RequestShape,
    pub(crate) policy: PolicyFingerprint,
    pub(crate) stage_marker: Option<ClassifierStage>,
}

pub(crate) fn detect(
    fingerprint: &RequestFingerprint<'_>,
) -> Result<Option<ClassifierStage>, ApiError> {
    if !uses_qwen_auto_mode(fingerprint.model) || fingerprint.shape == RequestShape::Other {
        return Ok(None);
    }

    let expected_stage = match fingerprint.max_tokens {
        Some(STAGE_ONE_MAX_TOKENS) => Some(ClassifierStage::One),
        Some(STAGE_TWO_MAX_TOKENS) => Some(ClassifierStage::Two),
        _ => None,
    };
    let Some(expected_stage) = expected_stage else {
        return Ok(None);
    };
    if fingerprint.policy == PolicyFingerprint::Qualified
        && fingerprint.stage_marker == Some(expected_stage)
    {
        Ok(Some(expected_stage))
    } else {
        Err(ApiError::InvalidRequest(
            "unrecognized Claude Code Auto mode classifier request; blocked for safety".to_owned(),
        ))
    }
}

fn uses_qwen_auto_mode(model: &str) -> bool {
    model == CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS
        || model
            .strip_prefix(CLAUDE_GATEWAY_MODEL_PREFIX)
            .is_some_and(|provider_id| provider_id == CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID)
}

pub(crate) fn tune_for_qwen(stage: ClassifierStage, body: &mut Map<String, Value>) {
    let max_tokens = match stage {
        ClassifierStage::One => QWEN_STAGE_ONE_MAX_TOKENS,
        ClassifierStage::Two => STAGE_TWO_MAX_TOKENS,
    };
    body.insert("max_tokens".to_owned(), Value::Number(max_tokens.into()));
    body.insert("temperature".to_owned(), Value::Number(0.into()));
    body.insert(
        "chat_template_kwargs".to_owned(),
        json!({"enable_thinking": false}),
    );
}

pub(crate) const fn policy_markers() -> [&'static str; POLICY_MARKERS.len()] {
    POLICY_MARKERS
}

pub(crate) const fn stage_one_marker() -> &'static str {
    STAGE_ONE_MARKER
}

pub(crate) const fn stage_two_marker() -> &'static str {
    STAGE_TWO_MARKER
}

#[cfg(test)]
mod tests {
    use super::{
        ClassifierStage, PolicyFingerprint, RequestFingerprint, RequestShape, detect,
        stage_one_marker, stage_two_marker, tune_for_qwen,
    };
    use nan_harness_core::{
        CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID,
        claude_gateway_model_id,
    };
    use serde_json::Map;

    #[test]
    fn detects_both_qualified_classifier_stages() {
        let gateway_model = claude_gateway_model_id(CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID);
        for model in [CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, &gateway_model] {
            for (max_tokens, expected) in
                [(64, ClassifierStage::One), (8_192, ClassifierStage::Two)]
            {
                let fingerprint = RequestFingerprint {
                    model,
                    max_tokens: Some(max_tokens),
                    shape: RequestShape::ClassifierCandidate,
                    policy: PolicyFingerprint::Qualified,
                    stage_marker: Some(expected),
                };

                assert_eq!(
                    detect(&fingerprint).expect("fingerprint should parse"),
                    Some(expected)
                );
            }
        }
    }

    #[test]
    fn rejects_classifier_shapes_with_unknown_prompts() {
        let fingerprint = RequestFingerprint {
            model: CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS,
            max_tokens: Some(64),
            shape: RequestShape::ClassifierCandidate,
            policy: PolicyFingerprint::Unknown,
            stage_marker: Some(ClassifierStage::One),
        };

        let error = detect(&fingerprint).expect_err("unknown policy must fail closed");
        assert_eq!(error.code(), "NH-BRIDGE-102");
    }

    #[test]
    fn tunes_classifier_generation_without_affecting_detection_markers() {
        let mut body = Map::new();
        tune_for_qwen(ClassifierStage::One, &mut body);

        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["temperature"], 0);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert!(!stage_one_marker().is_empty());
        assert!(!stage_two_marker().is_empty());
    }
}
