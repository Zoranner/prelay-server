use chrono::{DateTime, Datelike, Duration, FixedOffset, Months, NaiveDate, TimeZone, Utc};
use prelay_protocol::{
    QuotaUnit, QuotaWindow, QuotaWindowKind, ShareQuotaAccumulate, ShareQuotaRule,
};

use crate::store::storage::{Storage, StorageError};

const LOCAL_OFFSET_SECONDS: i32 = 8 * 60 * 60;

pub(crate) async fn windows(
    storage: &Storage,
    provider_id: &str,
    grantee_identity_id: &str,
    share_created_at: &str,
    rule: &ShareQuotaRule,
    now: DateTime<Utc>,
) -> Result<Vec<QuotaWindow>, StorageError> {
    let share_created_at = DateTime::parse_from_rfc3339(share_created_at)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            StorageError::InvalidProviderSharing(format!("invalid share creation time: {error}"))
        })?;
    let offset = local_offset()?;
    let (period_start, period_end) = period_bounds(now, offset, rule.accumulate)?;
    let period_days = (period_end - period_start).num_days().max(1);
    let (_, day_end) = day_bounds(now, offset)?;
    let days = accrual_days(now, offset, period_start, period_days, share_created_at);
    // 共享关系建立前的活动不属于这次共享，不能计入本期已用。
    let used_start = if share_created_at > period_start {
        share_created_at
    } else {
        period_start
    };
    let used = storage
        .shared_quota_used_tokens(provider_id, grantee_identity_id, used_start, period_end)
        .await?;
    let mut windows = vec![window_for(
        QuotaWindowKind::Day,
        day_end,
        rule.amount.saturating_mul(days),
        used,
    )];
    if let Some(kind) = period_window_kind(rule.accumulate) {
        windows.push(window_for(
            kind,
            period_end,
            rule.amount.saturating_mul(period_days),
            used,
        ));
    }
    Ok(windows)
}

fn period_window_kind(accumulate: ShareQuotaAccumulate) -> Option<QuotaWindowKind> {
    match accumulate {
        ShareQuotaAccumulate::None => None,
        ShareQuotaAccumulate::Week => Some(QuotaWindowKind::Week),
        ShareQuotaAccumulate::Month => Some(QuotaWindowKind::Month),
    }
}

fn period_bounds(
    now: DateTime<Utc>,
    offset: FixedOffset,
    accumulate: ShareQuotaAccumulate,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    match accumulate {
        ShareQuotaAccumulate::None => day_bounds(now, offset),
        ShareQuotaAccumulate::Week => week_bounds(now, offset),
        ShareQuotaAccumulate::Month => month_bounds(now, offset),
    }
}

/// 每天注入一份额度，从共享建立当天算起；共享建立在周期中间不会补前面几天的量。
fn accrual_days(
    now: DateTime<Utc>,
    offset: FixedOffset,
    period_start: DateTime<Utc>,
    period_days: i64,
    share_created_at: DateTime<Utc>,
) -> i64 {
    let period_date = period_start.with_timezone(&offset).date_naive();
    let first_date = share_created_at
        .with_timezone(&offset)
        .date_naive()
        .max(period_date);
    let elapsed = (now.with_timezone(&offset).date_naive() - first_date).num_days();
    (elapsed + 1).clamp(1, period_days)
}

fn window_for(kind: QuotaWindowKind, end: DateTime<Utc>, limit: i64, used: i64) -> QuotaWindow {
    let remaining = (limit - used).max(0);
    let used_percent = if limit > 0 {
        (used as f64 / limit as f64 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    QuotaWindow {
        kind,
        span_secs: match kind {
            QuotaWindowKind::Day => Some(24 * 60 * 60),
            QuotaWindowKind::Week => Some(7 * 24 * 60 * 60),
            _ => None,
        },
        used_percent,
        resets_at: Some(end.to_rfc3339()),
        remaining: Some(remaining as f64),
        limit: Some(limit as f64),
        unit: Some(QuotaUnit::Tokens),
        display: None,
    }
}

fn day_bounds(
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let today = now.with_timezone(&offset).date_naive();
    let end_date = today.checked_add_signed(Duration::days(1)).ok_or_else(|| {
        StorageError::InvalidProviderSharing("could not resolve next day".to_string())
    })?;
    Ok((
        local_midnight(today, offset),
        local_midnight(end_date, offset),
    ))
}

fn week_bounds(
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let local = now.with_timezone(&offset);
    let start_date =
        local.date_naive() - Duration::days(local.weekday().num_days_from_monday() as i64);
    let end_date = start_date
        .checked_add_signed(Duration::days(7))
        .ok_or_else(|| {
            StorageError::InvalidProviderSharing("could not resolve next week".to_string())
        })?;
    Ok((
        local_midnight(start_date, offset),
        local_midnight(end_date, offset),
    ))
}

fn month_bounds(
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let local = now.with_timezone(&offset);
    let start_date = NaiveDate::from_ymd_opt(local.year(), local.month(), 1)
        .ok_or_else(|| StorageError::InvalidProviderSharing("invalid month start".to_string()))?;
    let end_date = start_date
        .checked_add_months(Months::new(1))
        .ok_or_else(|| {
            StorageError::InvalidProviderSharing("could not resolve next month".to_string())
        })?;
    Ok((
        local_midnight(start_date, offset),
        local_midnight(end_date, offset),
    ))
}

fn local_midnight(date: NaiveDate, offset: FixedOffset) -> DateTime<Utc> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight is valid");
    offset
        .from_local_datetime(&naive)
        .single()
        .expect("fixed offset always resolves")
        .with_timezone(&Utc)
}

fn local_offset() -> Result<FixedOffset, StorageError> {
    FixedOffset::east_opt(LOCAL_OFFSET_SECONDS)
        .ok_or_else(|| StorageError::InvalidProviderSharing("invalid local timezone".to_string()))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn offset() -> FixedOffset {
        FixedOffset::east_opt(LOCAL_OFFSET_SECONDS).expect("offset")
    }

    fn at(month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        FixedOffset::east_opt(LOCAL_OFFSET_SECONDS)
            .expect("offset")
            .with_ymd_and_hms(2026, month, day, hour, minute, 0)
            .single()
            .expect("local time")
            .with_timezone(&Utc)
    }

    fn rule(accumulate: ShareQuotaAccumulate) -> ShareQuotaRule {
        ShareQuotaRule {
            amount: 100,
            accumulate,
        }
    }

    fn local(value: DateTime<Utc>) -> String {
        value
            .with_timezone(&FixedOffset::east_opt(LOCAL_OFFSET_SECONDS).expect("offset"))
            .to_rfc3339()
    }

    #[test]
    fn day_bounds_reset_at_local_midnight() {
        let (start, end) = day_bounds(at(11, 16, 10, 0), offset()).expect("day bounds");
        assert_eq!(local(start), "2026-11-16T00:00:00+08:00");
        assert_eq!(local(end), "2026-11-17T00:00:00+08:00");
    }

    #[test]
    fn week_bounds_reset_on_monday() {
        let (start, end) = week_bounds(at(11, 18, 10, 0), offset()).expect("week bounds");
        assert_eq!(local(start), "2026-11-16T00:00:00+08:00");
        assert_eq!(local(end), "2026-11-23T00:00:00+08:00");
    }

    #[test]
    fn month_bounds_cover_the_real_month_length() {
        let (start, end) = month_bounds(at(11, 18, 10, 0), offset()).expect("month bounds");
        assert_eq!(local(start), "2026-11-01T00:00:00+08:00");
        assert_eq!(local(end), "2026-12-01T00:00:00+08:00");
        assert_eq!((end - start).num_days(), 30);
    }

    #[test]
    fn month_bounds_follow_leap_years() {
        let leap = FixedOffset::east_opt(LOCAL_OFFSET_SECONDS)
            .expect("offset")
            .with_ymd_and_hms(2028, 2, 10, 10, 0, 0)
            .single()
            .expect("local time")
            .with_timezone(&Utc);
        let (start, end) = month_bounds(leap, offset()).expect("month bounds");
        assert_eq!(local(start), "2028-02-01T00:00:00+08:00");
        assert_eq!((end - start).num_days(), 29);
    }

    #[test]
    fn non_accumulating_rule_stays_on_the_day_period() {
        let rule = rule(ShareQuotaAccumulate::None);
        let (start, end) =
            period_bounds(at(11, 18, 10, 0), offset(), rule.accumulate).expect("day period");
        assert_eq!(local(start), "2026-11-18T00:00:00+08:00");
        assert_eq!(local(end), "2026-11-19T00:00:00+08:00");
        assert_eq!(period_window_kind(rule.accumulate), None);
    }

    #[test]
    fn week_rule_uses_the_week_period() {
        let rule = rule(ShareQuotaAccumulate::Week);
        let (start, end) =
            period_bounds(at(11, 18, 10, 0), offset(), rule.accumulate).expect("week period");
        assert_eq!(local(start), "2026-11-16T00:00:00+08:00");
        assert_eq!((end - start).num_days(), 7);
        assert_eq!(
            period_window_kind(rule.accumulate),
            Some(QuotaWindowKind::Week)
        );
    }

    #[test]
    fn month_rule_uses_the_month_period() {
        let rule = rule(ShareQuotaAccumulate::Month);
        let (start, end) =
            period_bounds(at(11, 18, 10, 0), offset(), rule.accumulate).expect("month period");
        assert_eq!((end - start).num_days(), 30);
        assert_eq!(
            period_window_kind(rule.accumulate),
            Some(QuotaWindowKind::Month)
        );
    }

    #[test]
    fn window_carries_limit_remaining_and_percent() {
        let window = window_for(QuotaWindowKind::Day, at(11, 18, 10, 0), 300, 40);

        assert_eq!(window.kind, QuotaWindowKind::Day);
        assert_eq!(window.span_secs, Some(24 * 60 * 60));
        assert_eq!(window.limit, Some(300.0));
        assert_eq!(window.remaining, Some(260.0));
        assert!((window.used_percent - 40.0 / 300.0 * 100.0).abs() < 1e-9);
        assert_eq!(window.unit, Some(QuotaUnit::Tokens));
    }

    #[test]
    fn exhausted_window_reports_zero_remaining() {
        let window = window_for(QuotaWindowKind::Week, at(11, 18, 10, 0), 300, 420);
        assert_eq!(window.remaining, Some(0.0));
        assert_eq!(window.used_percent, 100.0);
    }

    #[test]
    fn accrual_days_count_calendar_days_from_the_week_start() {
        let (start, end) = week_bounds(at(11, 18, 10, 0), offset()).expect("week bounds");
        let days = |now: DateTime<Utc>| accrual_days(now, offset(), start, 7, at(11, 1, 0, 0));

        assert_eq!(days(at(11, 16, 0, 1)), 1);
        assert_eq!(days(at(11, 18, 10, 0)), 3);
        assert_eq!(days(at(11, 22, 23, 59)), 7);
        assert_eq!(days(at(11, 23, 0, 0)), 7);
        assert!(end > at(11, 22, 23, 59));
    }

    #[test]
    fn accrual_days_start_at_the_share_creation_day() {
        let (start, _) = week_bounds(at(11, 19, 10, 0), offset()).expect("week bounds");
        let created = at(11, 18, 15, 0);

        assert_eq!(
            accrual_days(at(11, 18, 16, 0), offset(), start, 7, created),
            1
        );
        assert_eq!(
            accrual_days(at(11, 19, 10, 0), offset(), start, 7, created),
            2
        );
        assert_eq!(
            accrual_days(at(11, 22, 10, 0), offset(), start, 7, created),
            5
        );
    }

    #[test]
    fn accrual_days_stop_at_the_month_end() {
        let (start, end) = month_bounds(at(12, 31, 23, 0), offset()).expect("month bounds");
        assert_eq!((end - start).num_days(), 31);
        assert_eq!(
            accrual_days(at(12, 31, 23, 0), offset(), start, 31, at(12, 1, 0, 0)),
            31
        );
    }

    #[test]
    fn accrual_days_never_exceed_the_period_length() {
        let (start, _) = month_bounds(at(2, 10, 10, 0), offset()).expect("month bounds");
        assert_eq!(
            accrual_days(at(2, 28, 23, 0), offset(), start, 28, at(2, 1, 0, 0)),
            28
        );
    }
}
