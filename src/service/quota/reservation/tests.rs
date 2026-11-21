use std::{
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
};

use prelay_protocol::{QuotaUnit, QuotaWindow, QuotaWindowKind};

use super::{estimate_tokens, window_kind_name, HeldKey, QuotaReservations};

#[test]
fn estimates_at_least_one_small_request() {
    assert!(estimate_tokens(&serde_json::json!({"model":"k3"})) >= 256);
}

#[test]
fn estimates_at_least_the_declared_output_budget() {
    assert!(estimate_tokens(&serde_json::json!({"max_output_tokens": 5_000})) >= 5_000);
    assert!(estimate_tokens(&serde_json::json!({"max_completion_tokens": 7_000})) >= 7_000);
    assert!(estimate_tokens(&serde_json::json!({"max_tokens": 9_000})) >= 9_000);
}

#[test]
fn empty_reservations_can_be_cloned() {
    let reservations = QuotaReservations::default();
    let clone = reservations.clone();
    drop(clone);
}

#[test]
fn reservations_do_not_overshoot_a_window() {
    let reservations = QuotaReservations::default();
    let windows = vec![QuotaWindow {
        kind: QuotaWindowKind::Day,
        span_secs: Some(24 * 60 * 60),
        used_percent: 0.0,
        resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
        remaining: Some(1000.0),
        limit: Some(1000.0),
        unit: Some(QuotaUnit::Tokens),
        display: None,
    }];
    let guards = Arc::new(Mutex::new(Vec::new()));
    let successes = Arc::new(AtomicUsize::new(0));
    thread::scope(|scope| {
        for _ in 0..20 {
            let reservations = reservations.clone();
            let windows = windows.clone();
            let guards = Arc::clone(&guards);
            let successes = Arc::clone(&successes);
            scope.spawn(move || {
                if let Ok(guard) = reservations.reserve_windows("p", "i", &windows, 100) {
                    successes.fetch_add(1, Ordering::SeqCst);
                    guards.lock().expect("guards").push(guard);
                }
            });
        }
    });
    assert_eq!(successes.load(Ordering::SeqCst), 10);
}

#[test]
fn settled_reservations_hold_capacity_until_pending_expires() {
    let reservations = QuotaReservations::with_pending_ttl(std::time::Duration::from_millis(30));
    let windows = vec![QuotaWindow {
        kind: QuotaWindowKind::Day,
        span_secs: Some(24 * 60 * 60),
        used_percent: 0.0,
        resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
        remaining: Some(100.0),
        limit: Some(100.0),
        unit: Some(QuotaUnit::Tokens),
        display: None,
    }];

    let mut guard = reservations
        .reserve_windows("p", "i", &windows, 60)
        .expect("first reservation");
    guard.set_settle_on_drop(true);
    drop(guard);

    assert!(
        reservations
            .reserve_windows("p", "i", &windows, 50)
            .is_err(),
        "pending usage must reduce the available capacity"
    );

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        reservations.reserve_windows("p", "i", &windows, 50).is_ok(),
        "expired pending usage must release capacity"
    );
}

#[test]
fn pending_usage_is_visible_in_the_quota_window() {
    let reservations = QuotaReservations::default();
    let mut windows = vec![QuotaWindow {
        kind: QuotaWindowKind::Day,
        span_secs: Some(24 * 60 * 60),
        used_percent: 0.0,
        resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
        remaining: Some(100.0),
        limit: Some(100.0),
        unit: Some(QuotaUnit::Tokens),
        display: None,
    }];
    let mut guard = reservations
        .reserve_windows("p", "i", &windows, 60)
        .expect("reservation");
    guard.set_settle_on_drop(true);
    drop(guard);

    reservations.apply_pending("p", "i", &mut windows);
    assert_eq!(windows[0].remaining, Some(40.0));
    assert_eq!(windows[0].used_percent, 60.0);
}

#[test]
fn pending_usage_clears_when_materialization_reaches_its_sequence() {
    let durable = Arc::new(AtomicU64::new(0));
    let applied = Arc::new(AtomicU64::new(0));
    let mut reservations = QuotaReservations::default();
    reservations.track_sequences(Arc::clone(&durable), Arc::clone(&applied));
    let windows = vec![QuotaWindow {
        kind: QuotaWindowKind::Day,
        span_secs: Some(24 * 60 * 60),
        used_percent: 0.0,
        resets_at: Some("2026-11-18T00:00:00+08:00".to_string()),
        remaining: Some(100.0),
        limit: Some(100.0),
        unit: Some(QuotaUnit::Tokens),
        display: None,
    }];

    let mut guard = reservations
        .reserve_windows("p", "i", &windows, 60)
        .expect("reservation");
    guard.set_settle_on_drop(true);
    durable.store(7, Ordering::Release);
    drop(guard);

    assert!(
        reservations
            .reserve_windows("p", "i", &windows, 50)
            .is_err(),
        "unmaterialized usage must still hold capacity"
    );
    applied.store(6, Ordering::Release);
    assert!(
        reservations
            .reserve_windows("p", "i", &windows, 50)
            .is_err(),
        "usage must remain held before its sequence is materialized"
    );
    applied.store(7, Ordering::Release);
    assert!(
        reservations.reserve_windows("p", "i", &windows, 50).is_ok(),
        "materialized usage must release the pending capacity"
    );
}

#[test]
fn held_keys_distinguish_window_kinds() {
    let day = HeldKey {
        provider_id: "p".into(),
        identity_id: "i".into(),
        window: QuotaWindowKind::Day,
        reset: "r".into(),
    };
    let week = HeldKey {
        window: QuotaWindowKind::Week,
        ..day.clone()
    };
    assert_ne!(day, week);
}

#[test]
fn quota_error_uses_stable_window_names() {
    assert_eq!(window_kind_name(QuotaWindowKind::Day), "day");
    assert_eq!(window_kind_name(QuotaWindowKind::Week), "week");
    assert_eq!(window_kind_name(QuotaWindowKind::Month), "month");
}
