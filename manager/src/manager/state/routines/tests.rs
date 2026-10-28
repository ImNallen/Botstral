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
    assert_eq!(
        calendar::latest(
            &daily,
            stamp("2026-03-27T01:30:00Z"),
            stamp("2026-03-30T12:00:00Z")
        )
        .unwrap(),
        (stamp("2026-03-30T00:30:00Z"), 3)
    );
    assert_eq!(
        calendar::latest(
            &daily,
            stamp("2026-10-23T00:30:00Z"),
            stamp("2026-10-26T12:00:00Z")
        )
        .unwrap(),
        (stamp("2026-10-26T01:30:00Z"), 3)
    );
    assert_eq!(
        calendar::next(
            &RoutineTrigger::Interval {
                anchor: 100,
                seconds: 60
            },
            i64::MIN
        ),
        Err(OperationError::InvalidInput)
    );
    assert_eq!(
        calendar::next(
            &RoutineTrigger::Interval {
                anchor: i64::MAX,
                seconds: 60
            },
            100
        ),
        Err(OperationError::InvalidInput)
    );
}
