use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use serde::Deserialize;

pub use prelay_protocol::{
    stats::{LeaderboardMetric, UserLeaderboardEntry},
    ActivitySummary, ModelStatsSummary, ProviderStatsSummary, StatsOverview,
    TokenUsageTimelinePoint,
};

#[derive(Debug, Clone, Default)]
pub struct ActivityInsert {
    pub protocol_in: String,
    pub protocol_out: String,
    pub protocol_upstream: String,
    pub endpoint_name: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_requested: String,
    pub model_upstream: String,
    pub status: String,
    pub http_status: i64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub is_streaming: bool,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub latency_ms: i64,
    pub upstream_latency_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    pub tool_call_count: Option<i64>,
    pub upstream_request_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StreamActivityUpdate {
    pub status: String,
    pub http_status: i64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub latency_ms: i64,
    pub tool_call_count: Option<i64>,
    pub upstream_request_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatsRange {
    #[default]
    Today,
    Yesterday,
    ThisWeek,
    LastWeek,
    ThisMonth,
    LastMonth,
    ThisYear,
    LastYear,
    All,
}

#[derive(Clone, Copy)]
pub(crate) struct TimeBounds {
    pub(crate) start: DateTime<Utc>,
    pub(crate) end: DateTime<Utc>,
}

#[derive(Clone, Copy)]
pub(crate) enum TimelineGranularity {
    Hour,
    SixHours,
    Day,
    HalfMonth,
    Month,
}

pub(crate) struct TimelineBucket {
    pub(crate) label: String,
    pub(crate) bounds: TimeBounds,
}

impl StatsRange {
    pub(crate) fn bounds(self, now: DateTime<Utc>) -> Option<TimeBounds> {
        let now = now.with_timezone(&beijing_offset());
        let today = now.date_naive();
        let (start, end) = match self {
            Self::Today => (today, today + Duration::days(1)),
            Self::Yesterday => (today - Duration::days(1), today),
            Self::ThisWeek => {
                let start = today - Duration::days(today.weekday().num_days_from_monday().into());
                (start, start + Duration::days(7))
            }
            Self::LastWeek => {
                let end = today - Duration::days(today.weekday().num_days_from_monday().into());
                (end - Duration::days(7), end)
            }
            Self::ThisMonth => {
                let start = first_day_of_month(today);
                (start, first_day_of_next_month(start))
            }
            Self::LastMonth => {
                let end = first_day_of_month(today);
                (first_day_of_previous_month(end), end)
            }
            Self::ThisYear => (
                NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("valid year start"),
                NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).expect("valid next year start"),
            ),
            Self::LastYear => (
                NaiveDate::from_ymd_opt(today.year() - 1, 1, 1).expect("valid previous year start"),
                NaiveDate::from_ymd_opt(today.year(), 1, 1).expect("valid year start"),
            ),
            Self::All => return None,
        };
        Some(TimeBounds::from_beijing_dates(start, end))
    }

    pub(crate) const fn timeline_granularity(self) -> TimelineGranularity {
        match self {
            Self::Today | Self::Yesterday => TimelineGranularity::Hour,
            Self::ThisWeek | Self::LastWeek => TimelineGranularity::SixHours,
            Self::ThisMonth | Self::LastMonth => TimelineGranularity::Day,
            Self::ThisYear | Self::LastYear => TimelineGranularity::HalfMonth,
            Self::All => TimelineGranularity::Month,
        }
    }
}

impl TimeBounds {
    fn from_beijing_dates(start: NaiveDate, end: NaiveDate) -> Self {
        Self {
            start: beijing_start_of_day(start).with_timezone(&Utc),
            end: beijing_start_of_day(end).with_timezone(&Utc),
        }
    }
}

pub(crate) fn all_timeline_bounds(earliest: DateTime<Utc>, now: DateTime<Utc>) -> TimeBounds {
    let earliest = earliest.with_timezone(&beijing_offset());
    let now = now.with_timezone(&beijing_offset());
    let start = first_day_of_month(earliest.date_naive());
    let end = first_day_of_next_month(first_day_of_month(now.date_naive()));
    TimeBounds::from_beijing_dates(start, end)
}

pub(crate) fn timeline_buckets(
    bounds: TimeBounds,
    granularity: TimelineGranularity,
) -> Vec<TimelineBucket> {
    let mut buckets = Vec::new();
    let mut start = bounds.start.with_timezone(&beijing_offset());
    let end = bounds.end.with_timezone(&beijing_offset());
    while start < end {
        let next = match granularity {
            TimelineGranularity::Hour => start + Duration::hours(1),
            TimelineGranularity::SixHours => start + Duration::hours(6),
            TimelineGranularity::Day => start + Duration::days(1),
            TimelineGranularity::HalfMonth => {
                let date = start.date_naive();
                let next_date = if date.day() <= 15 {
                    date.with_day(16).expect("valid half-month boundary")
                } else {
                    first_day_of_next_month(first_day_of_month(date))
                };
                beijing_start_of_day(next_date)
            }
            TimelineGranularity::Month => beijing_start_of_day(first_day_of_next_month(
                first_day_of_month(start.date_naive()),
            )),
        };
        let label = match granularity {
            TimelineGranularity::Hour => start.format("%Y-%m-%d %H:%M:%S").to_string(),
            TimelineGranularity::SixHours
            | TimelineGranularity::Day
            | TimelineGranularity::HalfMonth
            | TimelineGranularity::Month => start.format("%Y-%m-%d").to_string(),
        };
        buckets.push(TimelineBucket {
            label,
            bounds: TimeBounds {
                start: start.with_timezone(&Utc),
                end: next.with_timezone(&Utc),
            },
        });
        start = next;
    }
    buckets
}

fn beijing_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 60 * 60).expect("valid Beijing UTC offset")
}

fn beijing_start_of_day(date: NaiveDate) -> DateTime<FixedOffset> {
    beijing_offset()
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("valid midnight"))
        .single()
        .expect("fixed offset has no ambiguous local times")
}

fn first_day_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("every month has a first day")
}

fn first_day_of_next_month(date: NaiveDate) -> NaiveDate {
    if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1).expect("valid next January")
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1).expect("valid next month")
    }
}

fn first_day_of_previous_month(date: NaiveDate) -> NaiveDate {
    if date.month() == 1 {
        NaiveDate::from_ymd_opt(date.year() - 1, 12, 1).expect("valid previous December")
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() - 1, 1).expect("valid previous month")
    }
}
