use std::sync::Arc;

use crate::application::desktop_notification_budget::{ClickWaitBudget, SendMode};

#[test]
fn under_cap_grants_a_click_wait_permit() {
    let budget = Arc::new(ClickWaitBudget::new(2));

    let first = budget.plan_send();

    assert!(matches!(first, SendMode::WaitForClick(_)));
    assert_eq!(budget.active_count(), 1);
}

#[test]
fn at_cap_degrades_to_fire_and_forget_without_consuming_a_slot() {
    let budget = Arc::new(ClickWaitBudget::new(1));
    let _held = budget.plan_send();
    assert_eq!(budget.active_count(), 1);

    let over_budget = budget.plan_send();

    assert!(matches!(over_budget, SendMode::FireAndForget));
    assert_eq!(budget.active_count(), 1);
}

#[test]
fn dropping_a_permit_releases_the_slot() {
    let budget = Arc::new(ClickWaitBudget::new(1));

    {
        let _permit = budget.plan_send();
        assert_eq!(budget.active_count(), 1);
    }

    assert_eq!(budget.active_count(), 0);
    assert!(matches!(budget.plan_send(), SendMode::WaitForClick(_)));
}

#[test]
fn a_panicking_waiter_still_releases_its_slot() {
    let budget = Arc::new(ClickWaitBudget::new(1));
    let for_panic = Arc::clone(&budget);

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _permit = for_panic.plan_send();
        panic!("waiter thread blew up while holding a permit");
    }));
    std::panic::set_hook(previous_hook);

    assert!(outcome.is_err());
    assert_eq!(budget.active_count(), 0);
}

#[test]
fn cap_zero_disables_click_waiting_entirely() {
    let budget = Arc::new(ClickWaitBudget::new(0));

    assert!(matches!(budget.plan_send(), SendMode::FireAndForget));
    assert_eq!(budget.active_count(), 0);
}

#[test]
fn concurrent_planning_never_exceeds_the_cap() {
    const CAP: usize = 3;
    const THREADS: usize = 16;

    let budget = Arc::new(ClickWaitBudget::new(CAP));
    let barrier = Arc::new(std::sync::Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);

    for _ in 0..THREADS {
        let budget = Arc::clone(&budget);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let mode = budget.plan_send();
            let observed = budget.active_count();
            // Hold the permit until every thread has planned, so the peak is observable.
            barrier.wait();
            let granted = matches!(mode, SendMode::WaitForClick(_));
            drop(mode);
            (granted, observed)
        }));
    }

    let results: Vec<(bool, usize)> = handles
        .into_iter()
        .map(|handle| handle.join().expect("planning thread panicked"))
        .collect();

    let granted = results.iter().filter(|(granted, _)| *granted).count();
    let peak = results
        .iter()
        .map(|(_, observed)| *observed)
        .max()
        .unwrap_or(0);

    assert_eq!(granted, CAP);
    assert!(
        peak <= CAP,
        "observed {peak} concurrent permits, cap is {CAP}"
    );
    assert_eq!(budget.active_count(), 0);
}
