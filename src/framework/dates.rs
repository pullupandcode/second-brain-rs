//! ISO date inputs with JavaScript-compatible defaults and overflow behavior.
use std::sync::LazyLock;

use jiff::tz::TimeZone;
use regex::{Captures, Regex};
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, UtcOffset};

use super::invalid;
use crate::runtime::DispatchError;

static ISO_DATE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<year>[0-9]{4}|[+-][0-9]{6})(?:-(?P<month>[0-9]{2})(?:-(?P<day>[0-9]{2}))?)?(?:[Tt](?P<hour>[0-9]{2}):(?P<minute>[0-9]{2})(?::(?P<second>[0-9]{2})(?:\.(?P<fraction>[0-9]+))?)?(?P<offset>[Zz]|[+-][0-9]{2}:?[0-9]{2})?)?$",
    )
});

pub(super) fn parse_date(raw: Option<&str>) -> Result<OffsetDateTime, DispatchError> {
    raw.map_or_else(
        || Ok(OffsetDateTime::now_utc()),
        |raw| {
            parse_iso(raw, &TimeZone::system()).ok_or_else(|| invalid("date must be a valid date"))
        },
    )
}

fn parse_iso(raw: &str, zone: &TimeZone) -> Option<OffsetDateTime> {
    // ECMAScript TimeClip applies after timezone conversion, including at the endpoints.
    parse_iso_unclipped(raw, zone)
        .filter(|date| date.unix_timestamp_nanos().abs() <= 8_640_000_000_000_000_000_000)
}

fn parse_iso_unclipped(raw: &str, zone: &TimeZone) -> Option<OffsetDateTime> {
    let fields = ISO_DATE.as_ref().ok()?.captures(raw)?;
    if fields.name("year")?.as_str() == "-000000" {
        return None;
    }
    let year = number(&fields, "year", 0)?;
    let month = Month::try_from(u8::try_from(number(&fields, "month", 1)?).ok()?).ok()?;
    let day = number(&fields, "day", 1)?;
    if !(1..=31).contains(&day) {
        return None;
    }
    let date = Date::from_calendar_date(year, month, 1)
        .ok()?
        .checked_add(Duration::days(i64::from(day - 1)))?;
    let hour = number(&fields, "hour", 0)?;
    let minute = number(&fields, "minute", 0)?;
    let second = number(&fields, "second", 0)?;
    let millis = milliseconds(&fields);
    if hour > 24
        || minute > 59
        || second > 59
        || (hour == 24
            && (minute != 0
                || second != 0
                || fields
                    .name("fraction")
                    .is_some_and(|value| value.as_str().bytes().any(|digit| digit != b'0'))))
    {
        return None;
    }
    let datetime = date
        .midnight()
        .checked_add(Duration::hours(i64::from(hour)))?
        .checked_add(Duration::minutes(i64::from(minute)))?
        .checked_add(Duration::seconds(i64::from(second)))?
        .checked_add(Duration::milliseconds(i64::from(millis)))?;
    let Some(offset) = fields.name("offset") else {
        return if fields.name("hour").is_some() {
            local_to_utc(datetime, zone)
        } else {
            Some(datetime.assume_utc())
        };
    };
    datetime
        .assume_offset(parse_offset(offset.as_str())?)
        .checked_to_offset(UtcOffset::UTC)
}

fn number(fields: &Captures<'_>, name: &str, default: i32) -> Option<i32> {
    fields
        .name(name)
        .map_or(Some(default), |value| value.as_str().parse().ok())
}

fn milliseconds(fields: &Captures<'_>) -> u16 {
    let Some(fraction) = fields.name("fraction") else {
        return 0;
    };
    let mut digits = fraction.as_str().bytes();
    let mut millis = 0_u16;
    for _ in 0..3 {
        millis = millis * 10 + u16::from(digits.next().unwrap_or(b'0') - b'0');
    }
    millis
}

fn parse_offset(raw: &str) -> Option<UtcOffset> {
    if raw.eq_ignore_ascii_case("z") {
        return Some(UtcOffset::UTC);
    }
    let digits: String = raw.get(1..)?.chars().filter(|c| *c != ':').collect();
    let hour: i32 = digits.get(..2)?.parse().ok()?;
    let minute: i32 = digits.get(2..)?.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    let seconds = (hour * 60 + minute) * 60;
    UtcOffset::from_whole_seconds(if raw.starts_with('-') {
        -seconds
    } else {
        seconds
    })
    .ok()
}

fn local_to_utc(date: PrimitiveDateTime, zone: &TimeZone) -> Option<OffsetDateTime> {
    // IANA history uses its initial offset before the first transition and its final
    // POSIX recurrence for the distant future. Gregorian weekdays/leap years repeat
    // every 400 years. Include its boundary years, whose UTC conversion can overflow.
    // Resolve within jiff's range, then transfer the UTC displacement
    // (including compatible gap/overlap handling) to the original civil year.
    let year = match date.year() {
        year if year >= 9999 => 8000 + year.rem_euclid(400),
        year if year <= -9999 => -8000 + year.rem_euclid(400),
        year => year,
    };
    let surrogate = date.replace_year(year).ok()?;
    let local = jiff::civil::DateTime::new(
        i16::try_from(year).ok()?,
        i8::try_from(u8::from(date.month())).ok()?,
        i8::try_from(date.day()).ok()?,
        i8::try_from(date.hour()).ok()?,
        i8::try_from(date.minute()).ok()?,
        i8::try_from(date.second()).ok()?,
        i32::try_from(date.nanosecond()).ok()?,
    )
    .ok()?;
    let timestamp = zone.to_zoned(local).ok()?.timestamp();
    let displacement = timestamp.as_nanosecond() - surrogate.assume_utc().unix_timestamp_nanos();
    OffsetDateTime::from_unix_timestamp_nanos(
        date.assume_utc().unix_timestamp_nanos() + displacement,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use time::format_description::well_known::Rfc3339;

    use super::*;

    #[test]
    fn local_dst_gap_and_overlap_match_javascript_compatible_resolution() {
        let zone = TimeZone::get("America/New_York").unwrap();
        for (raw, expected) in [
            ("2026-03-08T02:30:00", "2026-03-08T07:30:00Z"),
            ("2026-11-01T01:30:00", "2026-11-01T05:30:00Z"),
            ("2026-02-30T00:00:00", "2026-03-02T05:00:00Z"),
            ("2026-09T14:30", "2026-09-01T18:30:00Z"),
        ] {
            assert_eq!(
                parse_iso(raw, &zone).unwrap().format(&Rfc3339).unwrap(),
                expected,
                "{raw}"
            );
        }
    }
    #[test]
    fn extended_iso_years_obey_javascript_time_clip() {
        for (raw, millis) in [
            ("+010000-01-01T12:00:00Z", 253_402_344_000_000_i128),
            ("-010000-01-01T12:00:00Z", -377_736_696_000_000),
            ("+275760-09-13T00:00:00Z", 8_640_000_000_000_000),
            ("-271821-04-20T00:00:00Z", -8_640_000_000_000_000),
            ("+275760-09-13T01:00:00+01:00", 8_640_000_000_000_000),
            ("-271821-04-19T23:00:00-01:00", -8_640_000_000_000_000),
        ] {
            assert_eq!(
                parse_iso(raw, &TimeZone::UTC)
                    .unwrap()
                    .unix_timestamp_nanos()
                    / 1_000_000,
                millis,
                "{raw}"
            );
        }
        for raw in [
            "+275760-09-13T00:00:00.001Z",
            "-271821-04-19T23:59:59.999Z",
            "+999999-01-01",
            "-999999-01-01",
            "-000000-01-01",
            "10000-01-01",
        ] {
            assert!(parse_iso(raw, &TimeZone::UTC).is_none(), "{raw}");
        }
    }

    #[test]
    fn distant_local_years_match_node_timezone_offsets_and_boundaries() {
        // Expected Unix milliseconds recorded from the pinned Node reference.
        for (zone, raw, millis) in [
            (
                "America/New_York",
                "+010000-01-01T12:00:00",
                253_402_362_000_000_i128,
            ),
            (
                "America/New_York",
                "+010000-07-01T12:00:00",
                253_418_083_200_000,
            ),
            (
                "America/New_York",
                "-010000-01-01T12:00:00",
                -377_736_678_238_000,
            ),
            (
                "America/New_York",
                "-271821-04-20T00:00:00",
                -8_639_999_982_238_000,
            ),
            (
                "Australia/Lord_Howe",
                "+010000-01-01T12:00:00",
                253_402_304_400_000,
            ),
            (
                "Australia/Lord_Howe",
                "+010000-07-01T12:00:00",
                253_418_031_000_000,
            ),
            (
                "Australia/Lord_Howe",
                "-010000-01-01T12:00:00",
                -377_736_734_180_000,
            ),
            (
                "Pacific/Apia",
                "+010000-01-01T12:00:00",
                253_402_297_200_000,
            ),
            (
                "Pacific/Apia",
                "-010000-01-01T12:00:00",
                -377_736_741_184_000,
            ),
            (
                "America/New_York",
                "+010000-03-12T02:30:00",
                253_408_462_200_000,
            ),
            (
                "America/New_York",
                "+010000-11-05T01:30:00",
                253_429_018_200_000,
            ),
            (
                "America/New_York",
                "+275760-09-12T20:00:00",
                8_640_000_000_000_000,
            ),
            (
                "America/New_York",
                "-271821-04-19T19:03:58",
                -8_640_000_000_000_000,
            ),
            (
                "America/New_York",
                "9999-12-31T23:59:59",
                253_402_318_799_000,
            ),
            (
                "Pacific/Apia",
                "-009999-01-01T00:00:00",
                -377_705_161_984_000,
            ),
        ] {
            assert_eq!(
                parse_iso(raw, &TimeZone::get(zone).unwrap())
                    .unwrap()
                    .unix_timestamp_nanos()
                    / 1_000_000,
                millis,
                "{zone}: {raw}"
            );
        }
        for (zone, raw) in [
            ("America/New_York", "+275760-09-13T00:00:00"),
            ("Australia/Lord_Howe", "-271821-04-20T00:00:00"),
        ] {
            assert!(
                parse_iso(raw, &TimeZone::get(zone).unwrap()).is_none(),
                "{zone}: {raw}"
            );
        }
    }
}
