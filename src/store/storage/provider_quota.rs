use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Months, NaiveDate, TimeZone, Timelike, Utc, Weekday,
};
use prelay_protocol::{
    QuotaUnit, QuotaWindow, QuotaWindowKind, ShareQuotaWindow, ShareQuotaWindowKind,
};
use sea_orm::{
    ColumnTrait, EntityTrait, ExprTrait, FromQueryResult, QueryFilter, QuerySelect, Select,
};

use crate::store::entity::identity::{
    activities as identity_activities, provider_shares as identity_provider_shares,
};

use super::{
    provider_usage::{integer_sum, token_total_expr},
    Storage, StorageError,
};

const LOCAL_OFFSET_SECONDS: i32 = 8 * 60 * 60;

impl Storage {
    /// 读取某个共享用户实际生效的额度规则与起算时间。
    pub(crate) async fn shared_quota_rule(
        &self,
        provider_id: &str,
        grantee_identity_id: &str,
    ) -> Result<Option<(String, String)>, StorageError> {
        use crate::store::entity::identity::provider_configs as identity_provider_configs;

        let Some(provider) = identity_provider_configs::Entity::find_by_id(provider_id)
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let Some(json) = provider.share_quota_json else {
            return Ok(None);
        };
        let since = self
            .provider_share(provider_id, grantee_identity_id)
            .await?
            .map(|share| share.created_at)
            .unwrap_or(provider.created_at);
        Ok(Some((json, since)))
    }

    pub(crate) async fn provider_share(
        &self,
        provider_id: &str,
        grantee_identity_id: &str,
    ) -> Result<Option<identity_provider_shares::Model>, StorageError> {
        Ok(identity_provider_shares::Entity::find()
            .filter(identity_provider_shares::Column::ProviderId.eq(provider_id))
            .filter(identity_provider_shares::Column::GranteeIdentityId.eq(grantee_identity_id))
            .one(&self.db)
            .await?)
    }

    pub(crate) async fn shared_quota_windows(
        &self,
        provider_id: &str,
        grantee_identity_id: &str,
        share_created_at: &str,
        rules: &[ShareQuotaWindow],
        now: DateTime<Utc>,
    ) -> Result<Vec<QuotaWindow>, StorageError> {
        let share_created_at = DateTime::parse_from_rfc3339(share_created_at)
            .ok()
            .map(|value| value.with_timezone(&Utc));
        let mut windows = Vec::with_capacity(rules.len());
        for rule in rules {
            let (start, end) = share_window_bounds(now, rule)?;
            // 共享关系建立前的活动不属于这次共享，不能计入本期已用。
            let usage_start = share_created_at
                .as_ref()
                .filter(|created_at| **created_at > start)
                .cloned()
                .unwrap_or(start);
            let used =
                used_tokens(&self.db, provider_id, grantee_identity_id, usage_start, end).await?;
            let mut carry_over = 0;
            if rule.accumulate {
                let (previous_start, previous_end) = previous_window_bounds(rule, start, end)?;
                // 新建共享在上一周期并不存在，不能凭空继承一整期额度。
                if share_created_at.is_none_or(|created_at| created_at <= previous_start) {
                    let previous_used = used_tokens(
                        &self.db,
                        provider_id,
                        grantee_identity_id,
                        previous_start,
                        previous_end,
                    )
                    .await?;
                    carry_over = (rule.limit - previous_used).clamp(0, rule.limit);
                }
            }
            let limit = rule.limit.saturating_add(carry_over);
            let remaining = std::cmp::Ord::max(limit - used, 0);
            let used_percent = if limit > 0 {
                (used as f64 / limit as f64 * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            windows.push(QuotaWindow {
                kind: share_window_kind(rule.window),
                span_secs: match rule.window {
                    ShareQuotaWindowKind::Day => Some(24 * 60 * 60),
                    ShareQuotaWindowKind::Week => Some(7 * 24 * 60 * 60),
                    ShareQuotaWindowKind::Month => None,
                },
                used_percent,
                resets_at: Some(end.to_rfc3339()),
                remaining: Some(remaining as f64),
                limit: Some(limit as f64),
                unit: Some(QuotaUnit::Tokens),
                display: None,
            });
        }
        Ok(windows)
    }
}

fn share_window_kind(kind: ShareQuotaWindowKind) -> QuotaWindowKind {
    match kind {
        ShareQuotaWindowKind::Day => QuotaWindowKind::Day,
        ShareQuotaWindowKind::Week => QuotaWindowKind::Week,
        ShareQuotaWindowKind::Month => QuotaWindowKind::Month,
    }
}

async fn used_tokens(
    db: &sea_orm::DatabaseConnection,
    provider_id: &str,
    grantee_identity_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<i64, StorageError> {
    let row = usage_query(provider_id, grantee_identity_id)
        .filter(identity_activities::Column::CreatedAt.gte(start.to_rfc3339()))
        .filter(identity_activities::Column::CreatedAt.lt(end.to_rfc3339()))
        .select_only()
        .column_as(integer_sum(token_total_expr().sum()), "total_tokens")
        .into_model::<UsageTotal>()
        .one(db)
        .await?;
    Ok(row.and_then(|row| row.total_tokens).unwrap_or_default())
}

fn usage_query(
    provider_id: &str,
    grantee_identity_id: &str,
) -> Select<identity_activities::Entity> {
    identity_activities::Entity::find()
        .filter(identity_activities::Column::ProviderId.eq(provider_id))
        .filter(identity_activities::Column::IdentityId.eq(grantee_identity_id))
}

#[derive(FromQueryResult)]
struct UsageTotal {
    total_tokens: Option<i64>,
}

fn share_window_bounds(
    now: DateTime<Utc>,
    rule: &ShareQuotaWindow,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let offset = local_offset()?;
    let (hour, minute) = parse_reset_time(&rule.reset_time)?;
    match rule.window {
        ShareQuotaWindowKind::Day => day_bounds(now, offset, hour, minute),
        ShareQuotaWindowKind::Week => week_bounds(
            now,
            offset,
            hour,
            minute,
            rule.reset_weekday.ok_or_else(|| {
                StorageError::InvalidProviderSharing(
                    "week quota requires a reset weekday".to_string(),
                )
            })?,
        ),
        ShareQuotaWindowKind::Month => month_bounds(
            now,
            offset,
            hour,
            minute,
            rule.reset_day.ok_or_else(|| {
                StorageError::InvalidProviderSharing("month quota requires a reset day".to_string())
            })?,
        ),
    }
}

fn previous_window_bounds(
    rule: &ShareQuotaWindow,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let shift = match rule.window {
        ShareQuotaWindowKind::Day => Duration::days(1),
        ShareQuotaWindowKind::Week => Duration::days(7),
        ShareQuotaWindowKind::Month => {
            let offset = local_offset()?;
            let start_date = start.with_timezone(&offset).date_naive();
            let previous_start_date =
                start_date
                    .checked_sub_months(Months::new(1))
                    .ok_or_else(|| {
                        StorageError::InvalidProviderSharing(
                            "could not resolve previous month".to_string(),
                        )
                    })?;
            let local_start = start.with_timezone(&offset);
            return Ok((
                local_datetime(
                    previous_start_date,
                    local_start.hour(),
                    local_start.minute(),
                    offset,
                ),
                local_datetime(start_date, local_start.hour(), local_start.minute(), offset),
            ));
        }
    };
    Ok((start - shift, end - shift))
}

fn day_bounds(
    now: DateTime<Utc>,
    offset: FixedOffset,
    hour: u32,
    minute: u32,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let local = now.with_timezone(&offset);
    let today = local.date_naive();
    let mut start_date = today;
    let mut start = local_datetime(start_date, hour, minute, offset);
    if local < start {
        start_date = today - Duration::days(1);
        start = local_datetime(start_date, hour, minute, offset);
    }
    let end_date = start_date
        .checked_add_signed(Duration::days(1))
        .ok_or_else(|| {
            StorageError::InvalidProviderSharing("could not resolve next day".to_string())
        })?;
    let end = local_datetime(end_date, hour, minute, offset);
    Ok((start, end))
}

fn week_bounds(
    now: DateTime<Utc>,
    offset: FixedOffset,
    hour: u32,
    minute: u32,
    reset_weekday: u8,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let local = now.with_timezone(&offset);
    let target = weekday_from_number(reset_weekday)?;
    let days_since = (local.weekday().num_days_from_monday() as i64
        - target.num_days_from_monday() as i64)
        .rem_euclid(7);
    let mut start_date = local.date_naive() - Duration::days(days_since);
    let mut start = local_datetime(start_date, hour, minute, offset);
    if start > now {
        start_date -= Duration::days(7);
        start = local_datetime(start_date, hour, minute, offset);
    }
    let end_date = start_date
        .checked_add_signed(Duration::days(7))
        .ok_or_else(|| {
            StorageError::InvalidProviderSharing("could not resolve next week".to_string())
        })?;
    let end = local_datetime(end_date, hour, minute, offset);
    Ok((start, end))
}

fn month_bounds(
    now: DateTime<Utc>,
    offset: FixedOffset,
    hour: u32,
    minute: u32,
    reset_day: u8,
) -> Result<(DateTime<Utc>, DateTime<Utc>), StorageError> {
    let local = now.with_timezone(&offset);
    let mut start_date = NaiveDate::from_ymd_opt(local.year(), local.month(), reset_day as u32)
        .ok_or_else(|| {
            StorageError::InvalidProviderSharing("invalid month reset day".to_string())
        })?;
    let mut start = local_datetime(start_date, hour, minute, offset);
    if start > now {
        let previous_month = NaiveDate::from_ymd_opt(local.year(), local.month(), 1)
            .and_then(|date| date.checked_sub_months(Months::new(1)))
            .ok_or_else(|| {
                StorageError::InvalidProviderSharing("could not resolve previous month".to_string())
            })?;
        start_date = NaiveDate::from_ymd_opt(
            previous_month.year(),
            previous_month.month(),
            reset_day as u32,
        )
        .ok_or_else(|| {
            StorageError::InvalidProviderSharing("invalid month reset day".to_string())
        })?;
        start = local_datetime(start_date, hour, minute, offset);
    }
    let end_date = start_date
        .checked_add_months(Months::new(1))
        .ok_or_else(|| {
            StorageError::InvalidProviderSharing("could not resolve next month".to_string())
        })?;
    let end = local_datetime(end_date, hour, minute, offset);
    Ok((start, end))
}

fn local_datetime(date: NaiveDate, hour: u32, minute: u32, offset: FixedOffset) -> DateTime<Utc> {
    let naive = date
        .and_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| date.and_hms_opt(0, 0, 0).expect("midnight is valid"));
    offset
        .from_local_datetime(&naive)
        .single()
        .expect("fixed offset always resolves")
        .with_timezone(&Utc)
}

fn parse_reset_time(value: &str) -> Result<(u32, u32), StorageError> {
    let (hour, minute) = value.split_once(':').ok_or_else(|| {
        StorageError::InvalidProviderSharing("reset time must use HH:MM".to_string())
    })?;
    let hour = hour.parse::<u32>().map_err(|_| {
        StorageError::InvalidProviderSharing("reset time must use HH:MM".to_string())
    })?;
    let minute = minute.parse::<u32>().map_err(|_| {
        StorageError::InvalidProviderSharing("reset time must use HH:MM".to_string())
    })?;
    if hour > 23 || minute > 59 {
        return Err(StorageError::InvalidProviderSharing(
            "reset time must use HH:MM".to_string(),
        ));
    }
    Ok((hour, minute))
}

fn local_offset() -> Result<FixedOffset, StorageError> {
    FixedOffset::east_opt(LOCAL_OFFSET_SECONDS)
        .ok_or_else(|| StorageError::InvalidProviderSharing("invalid local timezone".to_string()))
}

fn weekday_from_number(value: u8) -> Result<Weekday, StorageError> {
    match value {
        1 => Ok(Weekday::Mon),
        2 => Ok(Weekday::Tue),
        3 => Ok(Weekday::Wed),
        4 => Ok(Weekday::Thu),
        5 => Ok(Weekday::Fri),
        6 => Ok(Weekday::Sat),
        7 => Ok(Weekday::Sun),
        _ => Err(StorageError::InvalidProviderSharing(
            "weekday must be from 1 to 7".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        FixedOffset::east_opt(LOCAL_OFFSET_SECONDS)
            .expect("offset")
            .with_ymd_and_hms(2026, 11, 16, hour, minute, 0)
            .single()
            .expect("local time")
            .with_timezone(&Utc)
    }

    fn rule(window: ShareQuotaWindowKind) -> ShareQuotaWindow {
        ShareQuotaWindow {
            window,
            limit: 100,
            accumulate: false,
            reset_time: "00:00".to_string(),
            reset_weekday: Some(1),
            reset_day: Some(1),
        }
    }

    fn local(value: DateTime<Utc>) -> String {
        value
            .with_timezone(&FixedOffset::east_opt(LOCAL_OFFSET_SECONDS).expect("offset"))
            .to_rfc3339()
    }

    #[test]
    fn day_window_resets_at_local_midnight() {
        let (start, end) =
            share_window_bounds(at(10, 0), &rule(ShareQuotaWindowKind::Day)).expect("day bounds");
        assert_eq!(local(start), "2026-11-16T00:00:00+08:00");
        assert_eq!(local(end), "2026-11-17T00:00:00+08:00");
    }

    #[test]
    fn day_window_respects_a_custom_reset_time() {
        let mut rule = rule(ShareQuotaWindowKind::Day);
        rule.reset_time = "18:00".to_string();
        let (start, end) = share_window_bounds(at(10, 0), &rule).expect("day bounds");
        assert_eq!(local(start), "2026-11-15T18:00:00+08:00");
        assert_eq!(local(end), "2026-11-16T18:00:00+08:00");
    }

    #[test]
    fn week_window_resets_on_monday() {
        let mut rule = rule(ShareQuotaWindowKind::Week);
        rule.reset_weekday = Some(1);
        let (start, end) = share_window_bounds(at(10, 0), &rule).expect("week bounds");
        assert_eq!(local(start), "2026-11-16T00:00:00+08:00");
        assert_eq!(local(end), "2026-11-23T00:00:00+08:00");
    }

    #[test]
    fn month_window_resets_on_first_day() {
        let mut rule = rule(ShareQuotaWindowKind::Month);
        rule.reset_day = Some(1);
        let (start, end) = share_window_bounds(at(10, 0), &rule).expect("month bounds");
        assert_eq!(local(start), "2026-11-01T00:00:00+08:00");
        assert_eq!(local(end), "2026-12-01T00:00:00+08:00");
    }

    #[test]
    fn previous_day_window_is_yesterday() {
        let rule = rule(ShareQuotaWindowKind::Day);
        let (start, end) = share_window_bounds(at(10, 0), &rule).expect("day bounds");
        let (previous_start, previous_end) =
            previous_window_bounds(&rule, start, end).expect("previous bounds");
        assert_eq!(local(previous_start), "2026-11-15T00:00:00+08:00");
        assert_eq!(local(previous_end), "2026-11-16T00:00:00+08:00");
    }

    #[test]
    fn shared_windows_use_period_kinds() {
        assert_eq!(
            share_window_kind(ShareQuotaWindowKind::Day),
            QuotaWindowKind::Day
        );
        assert_eq!(
            share_window_kind(ShareQuotaWindowKind::Week),
            QuotaWindowKind::Week
        );
        assert_eq!(
            share_window_kind(ShareQuotaWindowKind::Month),
            QuotaWindowKind::Month
        );
    }

    #[test]
    fn previous_month_window_is_the_previous_month() {
        let rule = rule(ShareQuotaWindowKind::Month);
        let (start, end) = share_window_bounds(at(10, 0), &rule).expect("month bounds");
        let (previous_start, previous_end) =
            previous_window_bounds(&rule, start, end).expect("previous bounds");
        assert_eq!(local(previous_start), "2026-10-01T00:00:00+08:00");
        assert_eq!(local(previous_end), "2026-11-01T00:00:00+08:00");
    }
}
