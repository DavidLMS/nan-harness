use nan_harness_core::{HarnessKind, LaunchPlan};
use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};

pub(super) fn candidate_paths(plan: &LaunchPlan, home: &Path) -> Vec<PathBuf> {
    let working = Path::new(&plan.process.working_directory);
    let mut paths = BTreeSet::new();
    paths.insert(working.join(".mcp.json"));
    add_harness_candidates(plan.harness.kind, home, working, &mut paths);
    paths.into_iter().collect()
}

pub(super) fn add_harness_candidates(
    harness: HarnessKind,
    home: &Path,
    working: &Path,
    paths: &mut BTreeSet<PathBuf>,
) {
    match harness {
        HarnessKind::ClaudeCode => add_claude_candidates(home, working, paths),
        HarnessKind::Codex => add_codex_candidates(home, paths),
        HarnessKind::OpenCode => add_opencode_candidates(home, working, paths),
        HarnessKind::Hermes => add_hermes_candidates(home, paths),
        HarnessKind::Pi => add_pi_candidates(home, working, paths),
        HarnessKind::Omp => add_omp_candidates(home, working, paths),
        HarnessKind::PrimeAgent => add_prime_agent_candidates(home, paths),
        HarnessKind::DeepSeekHarness => add_deepseek_candidates(home, paths),
        HarnessKind::OpenClaw => add_openclaw_candidates(home, paths),
        HarnessKind::Cline => add_cline_candidates(home, working, paths),
        HarnessKind::QwenCode => add_qwen_candidates(home, working, paths),
        HarnessKind::KimiCode => add_kimi_candidates(home, paths),
        HarnessKind::Goose => add_goose_candidates(home, paths),
        HarnessKind::Fx => add_fx_candidates(home, paths),
        HarnessKind::Aider => {}
    }
}

fn add_claude_candidates(home: &Path, working: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.extend([
        home.join(".claude.json"),
        home.join(".claude/settings.json"),
        working.join(".claude/settings.json"),
    ]);
}

fn add_codex_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.insert(
        env::var_os("CODEX_HOME")
            .map_or_else(|| home.join(".codex"), PathBuf::from)
            .join("config.toml"),
    );
}

fn add_opencode_candidates(home: &Path, working: &Path, paths: &mut BTreeSet<PathBuf>) {
    if let Some(path) = env::var_os("OPENCODE_CONFIG") {
        paths.insert(PathBuf::from(path));
    }
    let config_home = config_home(home);
    paths.extend([
        config_home.join("opencode/opencode.json"),
        config_home.join("opencode/opencode.jsonc"),
        working.join("opencode.json"),
        working.join("opencode.jsonc"),
    ]);
}

fn add_hermes_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.insert(
        env::var_os("HERMES_HOME")
            .map_or_else(|| home.join(".hermes"), PathBuf::from)
            .join("config.yaml"),
    );
}

fn add_pi_candidates(home: &Path, working: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.extend([
        home.join(".pi/agent/settings.json"),
        home.join(".pi/agent/mcp.json"),
        working.join(".pi/settings.json"),
    ]);
}

fn add_omp_candidates(home: &Path, working: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.extend(omp_candidate_paths(home, working));
}

fn add_prime_agent_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    let prime_home = env::var_os("PRIME_AGENT_CODING_AGENT_DIR")
        .map_or_else(|| home.join(".prime/agent"), PathBuf::from);
    paths.extend([
        prime_home.join("settings.json"),
        prime_home.join("mcp.json"),
    ]);
}

fn add_deepseek_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.extend(deepseek_candidate_paths(home));
}

fn add_openclaw_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.insert(home.join(".openclaw/openclaw.json"));
}

fn add_cline_candidates(home: &Path, working: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.extend([
        home.join(".cline/data/settings/mcp_settings.json"),
        home.join(".cline/data/settings/mcp.json"),
        working.join(".cline/mcp.json"),
    ]);
}

fn add_qwen_candidates(home: &Path, working: &Path, paths: &mut BTreeSet<PathBuf>) {
    let qwen_home = env::var_os("QWEN_HOME").map_or_else(|| home.join(".qwen"), PathBuf::from);
    paths.extend([
        qwen_home.join("settings.json"),
        qwen_home.join("mcp.json"),
        working.join(".qwen/settings.json"),
    ]);
}

fn add_kimi_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    let kimi_home =
        env::var_os("KIMI_CODE_HOME").map_or_else(|| home.join(".kimi-code"), PathBuf::from);
    paths.extend([kimi_home.join("config.toml"), kimi_home.join("mcp.json")]);
}

fn add_goose_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    let config_home = config_home(home);
    paths.extend([
        config_home.join("goose/config.yaml"),
        config_home.join("goose/profiles.yaml"),
    ]);
    if let Some(additional) = env::var_os("GOOSE_ADDITIONAL_CONFIG_FILES") {
        paths.extend(env::split_paths(&additional));
    }
}

fn add_fx_candidates(home: &Path, paths: &mut BTreeSet<PathBuf>) {
    paths.insert(config_home(home).join("fx/config.json"));
}

fn config_home(home: &Path) -> PathBuf {
    env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from)
}

fn omp_candidate_paths(home: &Path, working: &Path) -> [PathBuf; 3] {
    let omp_home =
        env::var_os("PI_CODING_AGENT_DIR").map_or_else(|| home.join(".omp/agent"), PathBuf::from);
    [
        omp_home.join("config.yml"),
        omp_home.join("config.yaml"),
        working.join(".omp/config.yml"),
    ]
}

fn deepseek_candidate_paths(home: &Path) -> [PathBuf; 4] {
    let deepseek_home = env::var_os("DSH_HOME").map_or_else(|| home.join(".dsh"), PathBuf::from);
    [
        deepseek_home.join("config.yaml"),
        deepseek_home.join("cordis.patch.yml"),
        deepseek_home.join("profiles/default.yaml"),
        deepseek_home.join("profiles/web/cordis.patch.yml"),
    ]
}
