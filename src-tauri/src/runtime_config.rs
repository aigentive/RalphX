//! Layer-neutral runtime configuration accessors.
//!
//! These read process-wide configuration (`config/ralphx.yaml` and friends) and
//! are consumed by every layer, including `infrastructure`. Hosting them in
//! `application` forced repositories into an upward `infrastructure -> application`
//! import purely to read a default; this root module is the neutral seam instead.
//!
//! Seed for a future `ralphx-runtime-config` crate — keep it to thin, pure
//! configuration reads with no service logic.

use crate::infrastructure::agents::claude::verification_config;

/// Default number of adversarial verification rounds for a new ideation session.
pub(crate) fn default_verification_max_rounds() -> u32 {
    verification_config().max_rounds
}
