use prelay_protocol::{ShareQuotaAccumulate, ShareQuotaRule};

use super::StorageError;

pub(super) fn validate_share_quota(quota: &ShareQuotaRule) -> Result<(), StorageError> {
    if quota.amount <= 0 {
        return Err(StorageError::InvalidProviderSharing(
            "shared quota amount must be positive".to_string(),
        ));
    }
    Ok(())
}

/// 旧版本把共享额度存成窗口数组；读取时折叠成单条规则，下次保存即写成新格式。
pub(crate) fn parse_share_quota(raw: &str) -> Result<ShareQuotaRule, StorageError> {
    if !raw.trim_start().starts_with('[') {
        let rule = serde_json::from_str::<ShareQuotaRule>(raw).map_err(|error| {
            StorageError::InvalidProviderSharing(format!(
                "stored sharing quota is invalid: {error}"
            ))
        })?;
        validate_share_quota(&rule)?;
        return Ok(rule);
    }
    let windows = serde_json::from_str::<Vec<LegacyShareQuotaWindow>>(raw).map_err(|error| {
        StorageError::InvalidProviderSharing(format!("stored sharing quota is invalid: {error}"))
    })?;
    legacy_share_quota(&windows)
}

#[derive(serde::Deserialize)]
struct LegacyShareQuotaWindow {
    window: LegacyShareQuotaWindowKind,
    limit: i64,
    #[serde(default)]
    accumulate: bool,
}

#[derive(serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum LegacyShareQuotaWindowKind {
    Day,
    Week,
    Month,
}

fn legacy_share_quota(windows: &[LegacyShareQuotaWindow]) -> Result<ShareQuotaRule, StorageError> {
    let day = windows
        .iter()
        .find(|window| window.window == LegacyShareQuotaWindowKind::Day);
    let week = windows
        .iter()
        .find(|window| window.window == LegacyShareQuotaWindowKind::Week);
    let month = windows
        .iter()
        .find(|window| window.window == LegacyShareQuotaWindowKind::Month);
    let Some(day) = day else {
        // 没有日窗口的旧规则只剩周期上限，按周期天数折算回每天的量。
        let mut amounts = Vec::new();
        if let Some(week) = week {
            amounts.push(per_day_amount(week.limit, 7));
        }
        if let Some(month) = month {
            amounts.push(per_day_amount(month.limit, 30));
        }
        let Some(amount) = amounts.into_iter().min().filter(|amount| *amount > 0) else {
            return Err(StorageError::InvalidProviderSharing(
                "stored sharing quota has no usable window".to_string(),
            ));
        };
        return Ok(ShareQuotaRule {
            amount,
            accumulate: if month.is_some() {
                ShareQuotaAccumulate::Month
            } else {
                ShareQuotaAccumulate::Week
            },
        });
    };
    if day.limit <= 0 {
        return Err(StorageError::InvalidProviderSharing(
            "stored sharing quota has no usable window".to_string(),
        ));
    }
    // 由日额度派生的周、月总额只会等于或高于日额度乘天数，低于它说明是手工写坏的数据。
    if let Some(week) = week {
        if week.limit < day.limit.saturating_mul(7) {
            return Err(StorageError::InvalidProviderSharing(
                "stored sharing quota week limit is below its daily limit".to_string(),
            ));
        }
    }
    if let Some(month) = month {
        if month.limit < day.limit.saturating_mul(30) {
            return Err(StorageError::InvalidProviderSharing(
                "stored sharing quota month limit is below its daily limit".to_string(),
            ));
        }
    }
    let accumulate = if !day.accumulate {
        ShareQuotaAccumulate::None
    } else if month.is_some() {
        ShareQuotaAccumulate::Month
    } else {
        ShareQuotaAccumulate::Week
    };
    Ok(ShareQuotaRule {
        amount: day.limit,
        accumulate,
    })
}

/// 只有周期窗口的旧规则没有日额度，按周期天数折算回每天的量。
fn per_day_amount(limit: i64, days: i64) -> i64 {
    if limit <= 0 {
        return 0;
    }
    (limit.saturating_add(days - 1)) / days
}
