use std::sync::atomic::{AtomicI8, Ordering};

const UNSET: i8 = -1;

static AGENT_PERSONAS_OVERRIDE: AtomicI8 = AtomicI8::new(UNSET);
static STANDALONE_CONVERSATIONS_OVERRIDE: AtomicI8 = AtomicI8::new(UNSET);

/// These overrides are process-global, so tests that set them cannot run concurrently with each
/// other: one test asserting "flag off" will observe another test's "flag on" and fail. Tests take
/// this lock for their whole body through a guard that also resets the override on drop.
#[cfg(any(test, feature = "test-utils"))]
pub static LIVE_FLAG_OVERRIDE_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serializes live-flag override tests and restores the ambient value when the test ends.
///
/// The lock is intentionally poison-tolerant: one failing test must fail alone, not convert every
/// other test sharing this flag into a misleading "poisoned lock" failure.
#[cfg(any(test, feature = "test-utils"))]
pub struct LiveFlagOverrideTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for LiveFlagOverrideTestGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl LiveFlagOverrideTestGuard {
    pub fn new() -> Self {
        Self {
            _lock: LIVE_FLAG_OVERRIDE_TEST_MUTEX
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Drop for LiveFlagOverrideTestGuard {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
        reset_standalone_conversations_override_for_test();
    }
}

/// Effective agent_personas flag: DB override (if set) > env > yaml.
pub fn agent_personas_enabled() -> bool {
    match AGENT_PERSONAS_OVERRIDE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => super::ui_feature_flags_config().agent_personas,
    }
}

/// Effective standalone_conversations flag: DB override (if set) > env > yaml.
pub fn standalone_conversations_enabled() -> bool {
    match STANDALONE_CONVERSATIONS_OVERRIDE.load(Ordering::Relaxed) {
        0 => false,
        1 => true,
        _ => super::ui_feature_flags_config().standalone_conversations,
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

pub fn set_standalone_conversations_override(value: Option<bool>) {
    STANDALONE_CONVERSATIONS_OVERRIDE.store(
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

#[doc(hidden)]
pub fn reset_standalone_conversations_override_for_test() {
    STANDALONE_CONVERSATIONS_OVERRIDE.store(UNSET, Ordering::Relaxed);
}
