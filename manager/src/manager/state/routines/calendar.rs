use crate::manager::OperationError;
use botstral_link::RoutineTrigger;
use chrono::{DateTime, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

pub(super) fn validate(trigger: &RoutineTrigger) -> Result<(), OperationError> {
    match trigger {
        RoutineTrigger::Once { at } if *at > 0 && DateTime::from_timestamp(*at, 0).is_some() => {
            Ok(())
        }
        RoutineTrigger::Interval { anchor, seconds }
            if *anchor > 0
                && DateTime::from_timestamp(*anchor, 0).is_some()
                && (60..=31_536_000).contains(seconds) =>
        {
            Ok(())
        }
        RoutineTrigger::Daily {
            hour,
            minute,
            timezone,
        } if *hour < 24 && *minute < 60 && timezone.parse::<Tz>().is_ok() => Ok(()),
        RoutineTrigger::Event { topic, equals, .. }
            if name(topic)
                && equals.len() <= 8
                && equals.iter().all(|(k, v)| {
                    name(k) && serde_json::to_vec(v).is_ok_and(|b| b.len() <= 512)
                }) =>
        {
            Ok(())
        }
        _ => Err(OperationError::InvalidInput),
    }
}
pub(super) fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
fn daily(date: NaiveDate, hour: u8, minute: u8, zone: Tz) -> Option<i64> {
    let mut local = date.and_hms_opt(hour.into(), minute.into(), 0)?;
    for _ in 0..=1440 {
        match zone.from_local_datetime(&local) {
            LocalResult::Single(t) => return Some(t.timestamp()),
            LocalResult::Ambiguous(a, b) => return Some(a.min(b).timestamp()),
            LocalResult::None => local = local.checked_add_signed(Duration::minutes(1))?,
        }
    }
    None
}
pub(super) fn next(trigger: &RoutineTrigger, after: i64) -> Result<Option<i64>, OperationError> {
    validate(trigger)?;
    if after < 0 || DateTime::from_timestamp(after, 0).is_none() {
        return Err(OperationError::InvalidInput);
    }
    match trigger {
        RoutineTrigger::Once { at } => Ok((*at > after).then_some(*at)),
        RoutineTrigger::Interval { anchor, seconds } => {
            let step = i64::from(*seconds);
            let at = if *anchor > after {
                *anchor
            } else {
                anchor
                    .checked_add(
                        ((after - anchor) / step + 1)
                            .checked_mul(step)
                            .ok_or(OperationError::InvalidInput)?,
                    )
                    .ok_or(OperationError::InvalidInput)?
            };
            Ok(Some(at))
        }
        RoutineTrigger::Daily {
            hour,
            minute,
            timezone,
        } => {
            let zone: Tz = timezone.parse().map_err(|_| OperationError::InvalidInput)?;
            let mut date = DateTime::<Utc>::from_timestamp(after, 0)
                .ok_or(OperationError::InvalidInput)?
                .with_timezone(&zone)
                .date_naive();
            for _ in 0..=3 {
                if let Some(at) = daily(date, *hour, *minute, zone).filter(|at| *at > after) {
                    return Ok(Some(at));
                }
                date = date.succ_opt().ok_or(OperationError::InvalidInput)?;
            }
            Err(OperationError::InvalidInput)
        }
        RoutineTrigger::Event { .. } => Ok(None),
    }
}
pub(super) fn latest(
    trigger: &RoutineTrigger,
    first: i64,
    now: i64,
) -> Result<(i64, u32), OperationError> {
    if let RoutineTrigger::Interval { seconds, .. } = trigger {
        let skipped = (now - first) / i64::from(*seconds);
        return Ok((
            first + skipped * i64::from(*seconds),
            skipped.min(i64::from(u32::MAX)) as u32,
        ));
    }
    if let RoutineTrigger::Daily {
        hour,
        minute,
        timezone,
    } = trigger
    {
        let zone: Tz = timezone.parse().map_err(|_| OperationError::InvalidInput)?;
        let date_of = |at: i64| -> Result<NaiveDate, OperationError> {
            let date = DateTime::<Utc>::from_timestamp(at, 0)
                .ok_or(OperationError::InvalidInput)?
                .with_timezone(&zone)
                .date_naive();
            if let Some(previous) = date.pred_opt()
                && daily(previous, *hour, *minute, zone) == Some(at)
            {
                Ok(previous)
            } else {
                Ok(date)
            }
        };
        let after = now.saturating_sub(3 * 86400).max(first - 1);
        let mut at = next(trigger, after)?.ok_or(OperationError::InvalidInput)?;
        for _ in 0..5 {
            let Some(following) = next(trigger, at)? else {
                break;
            };
            if following > now {
                break;
            }
            at = following;
        }
        let skipped = (date_of(at)? - date_of(first)?)
            .num_days()
            .clamp(0, i64::from(u32::MAX)) as u32;
        return Ok((at, skipped));
    }
    Ok((first, 0))
}
