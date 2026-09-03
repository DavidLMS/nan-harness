use super::reasoning::{ReasoningEffort, ReasoningPolicy};

pub const CLAUDE_GATEWAY_MODEL_PREFIX: &str = "anthropic/nan/";
pub const CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS: &str = "opus";
pub const CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID: &str = "qwen3.6";
pub const GENERIC_CODING_MODEL_DESCRIPTION: &str = "NaN text model · capabilities not yet profiled";
pub const GENERIC_CODING_MODEL_CONTEXT_WINDOW: u64 = 262_144;
pub const GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS: u64 = 32_768;
pub const KNOWN_NON_CODING_MODELS: [&str; 6] = [
    "whisper",
    "qwen3-embedding",
    "rerank",
    "kokoro",
    "flux-2-klein",
    "minimax-h3",
];
pub const KNOWN_CODING_MODELS: [CodingModelMetadata; 7] = [
    CodingModelMetadata {
        id: "qwen3.6",
        display_name: "NaN · Qwen 3.6",
        description: "General reasoning · tools + vision · 256K",
        context_window: 262_144,
        max_output_tokens: 65_536,
        image_input: true,
        reasoning: ReasoningPolicy::Toggle {
            default_enabled: true,
        },
    },
    CodingModelMetadata {
        id: "qwen3.8-flash",
        display_name: "NaN · Qwen 3.8 Flash",
        description: "General reasoning · tools + vision · 1M context",
        context_window: 1_000_000,
        max_output_tokens: GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS,
        image_input: true,
        // NaN currently accepts the thinking parameter but does not honor its
        // enabled/disabled value. Do not expose a misleading disable control.
        reasoning: ReasoningPolicy::AlwaysOn,
    },
    CodingModelMetadata {
        id: "deepseek-v4-flash",
        display_name: "NaN · DeepSeek V4 Flash",
        description: "Advanced reasoning · tools + vision · 1M context",
        context_window: 1_000_000,
        max_output_tokens: 262_144,
        image_input: true,
        reasoning: ReasoningPolicy::Effort {
            supported: [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            default: ReasoningEffort::Medium,
        },
    },
    CodingModelMetadata {
        id: "mimo-v2.5",
        display_name: "NaN · MiMo V2.5",
        description: "Omnimodal reasoning · tools + vision · 1M",
        context_window: 1_000_000,
        max_output_tokens: 65_536,
        image_input: true,
        reasoning: ReasoningPolicy::AlwaysOn,
    },
    CodingModelMetadata {
        id: "gemma4",
        display_name: "NaN · Gemma 4",
        description: "Opt-in reasoning · tools + vision · 256K",
        context_window: 262_144,
        max_output_tokens: 65_536,
        image_input: true,
        reasoning: ReasoningPolicy::Toggle {
            default_enabled: false,
        },
    },
    CodingModelMetadata {
        id: "glm5.2",
        display_name: "NaN · GLM 5.2",
        description: "Agentic coding · tools + reasoning · 500K",
        context_window: 500_000,
        max_output_tokens: 65_536,
        image_input: false,
        reasoning: ReasoningPolicy::Effort {
            supported: [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            default: ReasoningEffort::Medium,
        },
    },
    CodingModelMetadata {
        id: "glm5.3-flash",
        display_name: "NaN · GLM 5.3 Flash",
        description: "Agentic coding · tools + vision · 1M context",
        context_window: 1_000_000,
        max_output_tokens: GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS,
        image_input: true,
        reasoning: ReasoningPolicy::Effort {
            supported: [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            default: ReasoningEffort::Medium,
        },
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodingModelMetadata {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub image_input: bool,
    pub reasoning: ReasoningPolicy,
}
