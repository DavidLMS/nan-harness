use super::wire::{Message, MessageContent, MessagesRequest, SystemPrompt, Tool};
use serde_json::json;

pub(super) fn estimate_input_tokens(request: &MessagesRequest) -> u64 {
    let serialized_bytes = serde_json::to_vec(&json!({
        "system": request.system.as_ref().map(system_size),
        "message_count": request.messages.len(),
        "messages": request.messages.iter().map(message_size).collect::<Vec<_>>(),
        "tools": request.tools.iter().map(tool_size).collect::<Vec<_>>(),
    }))
    .map_or(0, |value| value.len());
    u64::try_from(serialized_bytes.div_ceil(3).saturating_add(16)).unwrap_or(u64::MAX)
}

fn system_size(system: &SystemPrompt) -> usize {
    match system {
        SystemPrompt::Text(text) => text.len(),
        SystemPrompt::Blocks(blocks) => blocks.iter().map(|block| block.text.len()).sum(),
    }
}

fn message_size(message: &Message) -> usize {
    match &message.content {
        MessageContent::Text(text) => text.len(),
        MessageContent::Blocks(blocks) => blocks.len().saturating_mul(32),
    }
}

fn tool_size(tool: &Tool) -> usize {
    match tool {
        Tool::Client(tool) => {
            tool.name.len()
                + tool.description.as_ref().map_or(0, String::len)
                + tool.input_schema.to_string().len()
        }
        Tool::Server(tool) => {
            tool.kind.len()
                + tool.name.len()
                + tool.allowed_domains.iter().map(String::len).sum::<usize>()
                + tool.blocked_domains.iter().map(String::len).sum::<usize>()
        }
    }
}
