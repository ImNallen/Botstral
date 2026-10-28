use super::*;
#[test]
fn calendar_gap_fold_and_anchoring() {
    use chrono::DateTime;
    let stamp = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().timestamp();
    let daily = RoutineTrigger::Daily {
        hour: 2,
        minute: 30,
        timezone: "Europe/Stockholm".into(),
    };
    assert_eq!(
        calendar::next(&daily, stamp("2026-03-28T23:00:00Z")).unwrap(),
        Some(stamp("2026-03-29T01:00:00Z"))
    );
    assert_eq!(
        calendar::next(&daily, stamp("2026-10-24T22:00:00Z")).unwrap(),
        Some(stamp("2026-10-25T00:30:00Z"))
    );
    assert_eq!(
        calendar::next(&daily, stamp("2026-10-25T00:30:00Z")).unwrap(),
        Some(stamp("2026-10-26T01:30:00Z"))
    );
    assert_eq!(
        calendar::next(
            &RoutineTrigger::Interval {
                anchor: 100,
                seconds: 60
            },
            221
        )
        .unwrap(),
        Some(280)
    );
}
