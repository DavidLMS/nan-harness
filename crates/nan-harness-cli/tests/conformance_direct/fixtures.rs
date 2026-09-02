use std::path::Path;

pub(super) const INVENTORY_MARKER: &str = "NAN_HARNESS_DIRECT_INVENTORY_OK";
pub(super) const HERMES_OPTIONAL_CREDENTIALS_CLEARED: &[(&str, &str)] = &[
    ("BFL_API_KEY", ""),
    ("ELEVENLABS_API_KEY", ""),
    ("FAL_KEY", ""),
    ("OPENAI_API_KEY", ""),
    ("XAI_API_KEY", ""),
];
pub(super) const OPENCLAW_MEDIA_CREDENTIALS_CLEARED: &[(&str, &str)] = &[
    ("AZURE_OPENAI_API_KEY", ""),
    ("BFL_API_KEY", ""),
    ("DEEPINFRA_API_KEY", ""),
    ("FAL_KEY", ""),
    ("GEMINI_API_KEY", ""),
    ("GOOGLE_API_KEY", ""),
    ("MINIMAX_API_KEY", ""),
    ("OPENAI_API_KEY", ""),
    ("OPENROUTER_API_KEY", ""),
    ("VYDRA_API_KEY", ""),
    ("XAI_API_KEY", ""),
];

pub(super) fn write_png(workspace: &Path, relative_path: &str) {
    const ONE_PIXEL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let path = workspace.join(relative_path);
    std::fs::write(path, ONE_PIXEL_PNG).expect("PNG fixture should be written");
}
