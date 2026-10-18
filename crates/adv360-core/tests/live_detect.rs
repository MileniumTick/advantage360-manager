//! Live probe: reports what the real machine says about the keyboard layout.
//!
//! This is deliberately a test rather than a hidden side effect: it is how the claim "detection
//! works on this machine" is evidenced.

#[test]
fn reports_the_detected_layout_without_asserting_a_specific_answer() {
    let l = adv360_core::kbd_layout::detect();
    println!(
        "LIVE_DETECT layout={} variant={:?} source={:?} device={:?}",
        l.layout, l.variant, l.source, l.device
    );
    println!("LIVE_DETECT detail={}", l.detail);
    println!(
        "LIVE_DETECT reliable={} warning={:?}",
        l.preview_is_reliable(),
        l.reliability_warning()
    );
    assert!(!l.layout.is_empty());
}
