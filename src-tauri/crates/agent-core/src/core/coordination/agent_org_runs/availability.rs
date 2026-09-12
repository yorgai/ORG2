//! Internal availability gate for Agent Org runtime features.
//!
//! This is deliberately not persisted in Team definitions or exposed to
//! model/tool context. Missing configuration enables Agent Org; explicit
//! values still fail closed unless they are exactly `1`.

const ENABLED_VALUE: &str = "1";

// This external key predates the canonical Agent Org runtime. Keep reading it
// so existing packaged-app and operator opt-outs retain identical behavior.
const LEGACY_AGENT_ORG_AVAILABILITY_ENV: &str = "ORGII_AGENT_ORG_REDESIGN";

fn configured_enabled(value: Option<&str>, test_build: bool) -> bool {
    test_build || value.is_none_or(|value| value.trim() == ENABLED_VALUE)
}

pub fn is_enabled() -> bool {
    let configured = std::env::var(LEGACY_AGENT_ORG_AVAILABILITY_ENV).ok();
    configured_enabled(configured.as_deref(), cfg!(test))
}

/// Enables Agent Org only after the packaged WebDriver harness explicitly
/// asks for it. Ordinary binaries cannot activate this in-process override,
/// even if a production frontend somehow tries to invoke the debug command.
pub fn enable_for_webdriver_test() -> Result<(), String> {
    if !cfg!(feature = "webdriver") {
        return Err(
            "agent_org_webdriver_override_unavailable: binary lacks the webdriver feature"
                .to_string(),
        );
    }
    std::env::set_var(LEGACY_AGENT_ORG_AVAILABILITY_ENV, ENABLED_VALUE);
    require_enabled()
}

pub fn require_enabled() -> Result<(), String> {
    is_enabled()
        .then_some(())
        .ok_or_else(|| "agent_org_disabled: Agent Org is not enabled".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn unit_test_builds_use_the_internal_gate_without_environment_state() {
        assert!(super::is_enabled());
    }

    #[test]
    fn production_gate_defaults_enabled_and_explicit_values_fail_closed() {
        assert!(super::configured_enabled(None, false));
        assert!(!super::configured_enabled(Some("true"), false));
        assert!(!super::configured_enabled(Some("0"), false));
        assert!(super::configured_enabled(Some("1"), false));
    }
}
