//! Concurrency budget for native macOS click-waits.
//!
//! Every outstanding `mac_notification_sys` click-wait registers a repeating 0.5s timer on the
//! **main** run loop, and each tick makes a synchronous XPC call to the notification daemon. With
//! enough ignored notifications the main thread stops servicing input entirely, which is the
//! unresponsive-app failure this budget exists to prevent. Over-budget notifications are still
//! delivered, they just skip the in-app click-wait.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crate::infrastructure::agents::claude::stream_timeouts;

/// How a single actionable notification should be dispatched.
pub(crate) enum SendMode {
    /// A click-wait slot was reserved. The permit is never read — it is carried into the waiting
    /// thread and held for its `Drop`, which returns the slot when the wait resolves.
    WaitForClick(#[allow(dead_code)] ClickWaitPermit),
    /// Budget is saturated — deliver without waiting for a click.
    FireAndForget,
}

/// Process-wide cap on concurrent native click-waits.
pub(crate) struct ClickWaitBudget {
    active: AtomicUsize,
    cap: usize,
}

impl ClickWaitBudget {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            cap,
        }
    }

    /// Reserves a click-wait slot when one is available, degrading instead of blocking.
    pub(crate) fn plan_send(self: &Arc<Self>) -> SendMode {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= self.cap {
                return SendMode::FireAndForget;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return SendMode::WaitForClick(ClickWaitPermit {
                        budget: Arc::clone(self),
                    })
                }
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) fn active_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

/// RAII slot reservation. Dropping it — normally, on panic, or on spawn failure — frees the slot.
pub(crate) struct ClickWaitPermit {
    budget: Arc<ClickWaitBudget>,
}

impl Drop for ClickWaitPermit {
    fn drop(&mut self) {
        self.budget.active.fetch_sub(1, Ordering::AcqRel);
    }
}

static CLICK_WAIT_BUDGET: OnceLock<Arc<ClickWaitBudget>> = OnceLock::new();

/// Process-global budget, sized once from runtime config.
pub(crate) fn click_wait_budget() -> &'static Arc<ClickWaitBudget> {
    CLICK_WAIT_BUDGET.get_or_init(|| {
        Arc::new(ClickWaitBudget::new(
            stream_timeouts().desktop_notification_max_click_waits,
        ))
    })
}
