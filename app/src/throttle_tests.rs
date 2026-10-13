use super::*;

const PERIOD: Duration = Duration::from_millis(16);
const QUICK: Duration = Duration::from_millis(1);

fn quick_pacing() -> Pacing {
    Pacing {
        period: PERIOD,
        quick: Some((QUICK, Arc::new(AtomicBool::new(true)))),
        last_quick_wait_at: None,
    }
}

#[test]
fn test_plain_throttle_always_waits_a_period() {
    let mut pacing = Pacing {
        period: PERIOD,
        quick: None,
        last_quick_wait_at: None,
    };
    let start = Instant::now();
    assert_eq!(pacing.wait_after_item(start, true), PERIOD);
    assert_eq!(
        pacing.wait_after_item(start + Duration::from_secs(1), true),
        PERIOD
    );
}

#[test]
fn test_item_after_quiet_spell_is_followed_quickly() {
    let mut pacing = quick_pacing();
    let start = Instant::now();

    // An echo, then a redraw 3ms later, both after the stream went quiet: the echo gets the
    // short wait, so the redraw goes through right away; anything soon after the redraw is
    // throttled again.
    assert_eq!(pacing.wait_after_item(start, true), QUICK);
    assert_eq!(
        pacing.wait_after_item(start + Duration::from_millis(3), true),
        PERIOD
    );

    // The next keystroke's echo, well after that, gets the short wait again.
    assert_eq!(
        pacing.wait_after_item(start + Duration::from_millis(120), true),
        QUICK
    );
}

#[test]
fn test_items_that_keep_coming_stay_throttled() {
    let mut pacing = quick_pacing();
    let start = Instant::now();
    assert_eq!(pacing.wait_after_item(start, true), QUICK);
    // Items that arrive while a wait is running never get the short wait, even when the
    // consumer is slow and handles them far apart.
    for i in 1..10 {
        assert_eq!(
            pacing.wait_after_item(start + PERIOD * 4 * i, false),
            PERIOD
        );
    }
}

#[test]
fn test_quick_waits_can_be_turned_off() {
    let enabled = Arc::new(AtomicBool::new(true));
    let mut pacing = Pacing {
        period: PERIOD,
        quick: Some((QUICK, enabled.clone())),
        last_quick_wait_at: None,
    };
    let start = Instant::now();
    enabled.store(false, Ordering::Relaxed);
    assert_eq!(pacing.wait_after_item(start, true), PERIOD);
    enabled.store(true, Ordering::Relaxed);
    assert_eq!(
        pacing.wait_after_item(start + Duration::from_secs(1), true),
        QUICK
    );
}
