use super::SearchPolicyError;
use super::inspection::inspect_configuration;
use super::policy::{SearchResolution, resolve_from_candidates};
use super::signal::DetectionSignal;
use nan_harness_core::WebSearchPolicy;
use nan_harness_i18n::{Locale, TerminalMessage};
use std::path::Path;

fn inspect(contents: &str) -> DetectionSignal {
    inspect_configuration(Path::new("config.yaml"), contents).expect("valid YAML fixture")
}

#[test]
fn unrelated_yaml_mentions_do_not_claim_search() {
    for contents in [
        "description: nan-search uses __search-mcp\n",
        "description: |\n  nan-search:\n    command: __search-mcp\n",
        "# mcpServers: {nan-search: {command: other}}\n",
        "description: {nan-search: __search-mcp}\n",
        "mcpServers: {private-tools: {command: private-mcp}}\n",
    ] {
        assert_eq!(inspect(contents), DetectionSignal::None, "{contents}");
    }
}

#[test]
fn yaml_ownership_is_scoped_to_the_reserved_mcp_entry() {
    for contents in [
        "mcpServers: {nan-search: {command: third-party}}",
        "description: __search-mcp\nmcpServers: {nan-search: {command: other}}",
        "mcp:\n  nan-search: {command: other}\n  tools: {args: [__search-mcp]}",
        "mcp_servers:\n  nan-search: {command: other}\n  search: {disabled: true, args: [__search-mcp]}",
        "mcp: {nan-search: {command: other}} # __search-mcp",
        "components:\n  - id: nan-search\n    config: {command: __search-mcp}\nmcp: {nan-search: {command: other}}",
    ] {
        assert_eq!(
            inspect(contents),
            DetectionSignal::Collision("config.yaml".into()),
            "{contents}"
        );
    }
    for contents in [
        "mcp: {nan-search: {command: nanh, args: [__search-mcp]}}",
        "mcp_servers:\n  NAN-SEARCH:\n    command: [nanh, __search-mcp]\n",
        "mcpServer: {nan-search: {command: 'wrapper# __search-mcp'}}",
    ] {
        assert_eq!(inspect(contents), DetectionSignal::ManagedNan, "{contents}");
    }
}

#[test]
fn disabled_yaml_servers_do_not_collide_or_establish_ownership() {
    for contents in [
        "mcpServers: {nan-search: {enabled: false, command: other}}",
        "mcpServers: {nan-search: {disabled: true, args: [__search-mcp]}}",
        "mcp: {brave-search: {enabled: false}}",
        "mcp: {brave-search: {disabled: true}}",
        "mcp: {nan-search: {disabled: true, search_provider: nan}}",
        "mcp: {brave-search: {enabled: false, config: {web_search: {enabled: true}}}}",
    ] {
        assert_eq!(inspect(contents), DetectionSignal::None, "{contents}");
    }
    assert_eq!(
        inspect("mcp: {brave-search: {enabled: true}}"),
        DetectionSignal::External
    );
}

#[test]
fn yaml_components_preserve_structure_tags_and_enabled_state() {
    for contents in [
        "- id: web-search-deepseek\n  disabled: false\n",
        "- config: {provider: deepseek}\n  id: 'web-search-deepseek'\n",
        "components: [{id: web-search-deepseek}]",
        "- id: web-search-deepseek\n  config: {disabled: true}\n",
        "- id: web-search-deepseek\n- id: llm-deepseek\n  disabled: true\n",
        "- id: web-search-deepseek\n  config:\n    baseURL: !!js process.env.NAN_BASE_URL\n",
    ] {
        assert_eq!(inspect(contents), DetectionSignal::External, "{contents}");
    }
    for contents in [
        "- id: web-search-deepseek\n  disabled: true\n",
        "- id: web-search-deepseek\n  enabled: false\n",
        "components: [{id: web-search-deepseek, disabled: true}]",
        "- id: web-search-deepseek\n  disabled: true\n  config: {search_provider: brave}\n",
        "- id: agent-default-model\n  config: {provider: nan-harness}\n",
    ] {
        assert_eq!(inspect(contents), DetectionSignal::None, "{contents}");
    }
}

#[test]
fn yaml_provider_selectors_handle_flow_syntax_and_quoted_hashes() {
    for contents in [
        "web: {search_backend: brave}",
        "web_search: {provider: 'brave#search'}",
        "web_search_provider: \"#brave\" # trailing comment",
        "tools: {webSearch: {enabled: true}}",
    ] {
        assert_eq!(inspect(contents), DetectionSignal::External, "{contents}");
    }
    assert_eq!(
        inspect("web: {search_backend: nan}"),
        DetectionSignal::ManagedNan
    );
    for selector in ["none", "disabled", "false", "off", "''"] {
        assert_eq!(
            inspect(&format!("search_provider: {selector}")),
            DetectionSignal::None
        );
    }
    assert_eq!(
        inspect_configuration(Path::new("config.yml"), "search_provider: brave")
            .expect("YML extension"),
        DetectionSignal::External
    );
}

#[test]
fn malformed_yaml_returns_a_typed_error_without_disclosing_contents() {
    for contents in [
        "secret-fixture: [",
        "mcp: {}\nmcp: {}",
        "---\nmcp: {}\n---\nmcp: {}",
    ] {
        let error = inspect_configuration(Path::new("config.yaml"), contents)
            .expect_err("malformed YAML must fail");
        assert!(matches!(error, SearchPolicyError::ParseYaml { .. }));
        for locale in [Locale::En, Locale::Es] {
            let message = error.terminal_message(locale);
            assert!(!message.contains("secret-fixture"));
            assert!(!message.contains(contents));
        }
    }
}

#[test]
fn yaml_collision_and_force_policy_remain_consistent() {
    let root = tempfile::tempdir().expect("temporary fixture");
    let path = root.path().join("config.yaml");
    let candidates = std::slice::from_ref(&path);
    std::fs::write(&path, "mcp: {nan-search: {command: other}}").expect("write collision");
    for policy in [WebSearchPolicy::Auto, WebSearchPolicy::Force] {
        assert!(matches!(resolve_from_candidates(policy, candidates),
            Err(SearchPolicyError::McpNameCollision(found)) if found == path));
    }
    std::fs::write(&path, "description: nan-search __search-mcp").expect("write unrelated text");
    assert_eq!(
        resolve_from_candidates(WebSearchPolicy::Auto, candidates)
            .expect("unrelated text permits launch"),
        SearchResolution::Nan
    );
}
