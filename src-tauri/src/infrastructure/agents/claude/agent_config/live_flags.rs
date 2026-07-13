use std::sync::atomic::{AtomicI8, Ordering};

const UNSET: i8 = -1;

static AGENT_PERSONAS_OVERRIDE: AtomicI8 = AtomicI8::new(UNSET);

/// Effective agent_personas flag: DB override (if set) > env > yaml.
pub fn agent_personas_enabled() -> bool {
    match AGENT_PERSONAS_OVERRIDE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => super::ui_feature_flags_config().agent_personas,
    }
}

pub fn set_agent_personas_override(value: Option<bool>) {
    AGENT_PERSONAS_OVERRIDE.store(
        match value {
            Some(true) => 1,
            Some(false) => 0,
            None => UNSET,
        },
        Ordering::Relaxed,
    );
}

#[doc(hidden)]
pub fn reset_agent_personas_override_for_test() {
    AGENT_PERSONAS_OVERRIDE.store(UNSET, Ordering::Relaxed);
}
