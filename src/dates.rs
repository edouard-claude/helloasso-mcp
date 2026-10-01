//! Dates and periods as people say them.
//!
//! HelloAsso filters take ISO dates or datetimes, with an *exclusive* upper
//! bound. A treasurer asks for "last month". This module is the translation:
//! `today`, `yesterday`, `2026-09-15`, a full RFC 3339 datetime for a date, and
//! `this month`, `last month`, `2026-09`, `2026`, `last 30 days` for a period.

use chrono::{Datelike, Duration, Local, NaiveDate};

use crate::error::{Error, Result};

/// Today, in the timezone of the machine running the server.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Format a date the way the API wants it.
pub fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Read one date bound: a word, a date, or a datetime kept as is.
///
/// Returns the string to send, so a datetime keeps its time and offset.
pub fn bound(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.len() > 10 && chrono::DateTime::parse_from_rfc3339(raw).is_ok() {
        return Ok(raw.to_owned());
    }
    if raw.len() > 10 && chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S").is_ok() {
        return Ok(raw.to_owned());
    }
    Ok(iso(date(raw)?))
}

/// Read one calendar day.
pub fn date(raw: &str) -> Result<NaiveDate> {
    let lower = raw.trim().to_lowercase();
    let today = today();
    match lower.as_str() {
        "today" | "aujourd'hui" | "aujourdhui" => return Ok(today),
        "yesterday" | "hier" => return Ok(today - Duration::days(1)),
        "tomorrow" | "demain" => return Ok(today + Duration::days(1)),
        _ => {}
    }
    if let Some(days) = lower
        .strip_suffix(" days ago")
        .or_else(|| lower.strip_suffix(" day ago"))
        .and_then(|n| n.trim().parse::<i64>().ok())
    {
        return Ok(today - Duration::days(days));
    }
    for format in ["%Y-%m-%d", "%d/%m/%Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(&lower, format) {
            return Ok(date);
        }
    }
    Err(Error::invalid(format!(
        "{raw:?} is not a date. Use YYYY-MM-DD, DD/MM/YYYY, today, yesterday, or \"N days ago\"."
    )))
}

/// Read a period into `[from, to)`, the half-open range HelloAsso filters on.
pub fn period(raw: &str) -> Result<(NaiveDate, NaiveDate)> {
    let lower = raw.trim().to_lowercase();
    let today = today();
    let month_start = |d: NaiveDate| d.with_day(1).unwrap_or(d);
    let next_month = |d: NaiveDate| {
        let (y, m) = if d.month() == 12 {
            (d.year() + 1, 1)
        } else {
            (d.year(), d.month() + 1)
        };
        NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(d)
    };
    let year_start = |y: i32| NaiveDate::from_ymd_opt(y, 1, 1).unwrap_or(today);
    let monday = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));

    let range = match lower.as_str() {
        "today" | "aujourd'hui" => (today, today + Duration::days(1)),
        "yesterday" | "hier" => (today - Duration::days(1), today),
        "this week" | "cette semaine" => (monday, monday + Duration::weeks(1)),
        "last week" | "la semaine dernière" | "semaine dernière" => {
            (monday - Duration::weeks(1), monday)
        }
        "this month" | "ce mois" | "ce mois-ci" => {
            (month_start(today), next_month(month_start(today)))
        }
        "last month" | "le mois dernier" | "mois dernier" => {
            let start = month_start(month_start(today) - Duration::days(1));
            (start, month_start(today))
        }
        "this year" | "cette année" => (year_start(today.year()), year_start(today.year() + 1)),
        "last year" | "l'année dernière" | "année dernière" => {
            (year_start(today.year() - 1), year_start(today.year()))
        }
        other => {
            if let Some(n) = other
                .strip_prefix("last ")
                .or_else(|| other.strip_prefix("past "))
                .and_then(|r| r.strip_suffix(" days"))
                .and_then(|n| n.trim().parse::<i64>().ok())
            {
                (today - Duration::days(n - 1), today + Duration::days(1))
            } else if let Ok(year) = other.parse::<i32>().map_err(|_| ()).and_then(|y| {
                if (2000..=2100).contains(&y) {
                    Ok(y)
                } else {
                    Err(())
                }
            }) {
                (year_start(year), year_start(year + 1))
            } else if let Ok(start) = NaiveDate::parse_from_str(&format!("{other}-01"), "%Y-%m-%d")
            {
                (start, next_month(start))
            } else if let Ok(day) = date(other) {
                (day, day + Duration::days(1))
            } else {
                return Err(Error::invalid(format!(
                    "{raw:?} is not a period. Use today, yesterday, this week, last week, this month, last month, this year, last year, \"last 30 days\", 2026, 2026-09 or 2026-09-15."
                )));
            }
        }
    };
    Ok(range)
}

/// Resolve the `period` / `from` / `to` trio every sales tool takes.
///
/// A period wins over nothing and loses to an explicit bound, so "last month,
/// but only from the 10th" is expressible.
pub fn range(
    period_arg: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(Option<String>, Option<String>)> {
    let (mut start, mut end) = match period_arg.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let (a, b) = period(p)?;
            (Some(iso(a)), Some(iso(b)))
        }
        None => (None, None),
    };
    if let Some(from) = from.map(str::trim).filter(|s| !s.is_empty()) {
        start = Some(bound(from)?);
    }
    if let Some(to) = to.map(str::trim).filter(|s| !s.is_empty()) {
        end = Some(bound(to)?);
    }
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    #[test]
    fn plain_dates_and_words_are_read() {
        assert_eq!(iso(date("2026-09-15").unwrap()), "2026-09-15");
        assert_eq!(iso(date("15/09/2026").unwrap()), "2026-09-15");
        assert_eq!(date("yesterday").unwrap(), today() - Duration::days(1));
        assert_eq!(date("3 days ago").unwrap(), today() - Duration::days(3));
        assert!(date("someday").is_err());
    }

    #[test]
    fn a_datetime_is_kept_as_is() {
        assert_eq!(
            bound("2026-09-15T10:00:00+02:00").unwrap(),
            "2026-09-15T10:00:00+02:00"
        );
        assert_eq!(bound("2026-09-15").unwrap(), "2026-09-15");
    }

    #[test]
    fn periods_are_half_open() {
        let (from, to) = period("2026-02").unwrap();
        assert_eq!(
            (iso(from), iso(to)),
            ("2026-02-01".into(), "2026-03-01".into())
        );
        let (from, to) = period("2026-12").unwrap();
        assert_eq!(
            (iso(from), iso(to)),
            ("2026-12-01".into(), "2027-01-01".into())
        );
        let (from, to) = period("2025").unwrap();
        assert_eq!(
            (iso(from), iso(to)),
            ("2025-01-01".into(), "2026-01-01".into())
        );
        let (from, to) = period("2026-09-15").unwrap();
        assert_eq!(
            (iso(from), iso(to)),
            ("2026-09-15".into(), "2026-09-16".into())
        );
    }

    #[test]
    fn last_month_ends_where_this_month_starts() {
        let (_, end) = period("last month").unwrap();
        let (start, _) = period("this month").unwrap();
        assert_eq!(end, start);
        assert_eq!(start.day(), 1);
    }

    #[test]
    fn last_n_days_includes_today() {
        let (from, to) = period("last 30 days").unwrap();
        assert_eq!((to - from).num_days(), 30);
        assert_eq!(to, today() + Duration::days(1));
    }

    #[test]
    fn an_explicit_bound_overrides_the_period() {
        let (from, to) = range(Some("2026-09"), Some("2026-09-10"), None).unwrap();
        assert_eq!(from.as_deref(), Some("2026-09-10"));
        assert_eq!(to.as_deref(), Some("2026-10-01"));
        assert_eq!(range(None, None, None).unwrap(), (None, None));
    }

    #[test]
    fn a_bad_period_lists_the_accepted_ones() {
        let message = period("soon").unwrap_err().to_string();
        assert!(message.contains("last month"), "{message}");
    }
}
