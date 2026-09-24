//! Time tools: epoch <-> RFC3339 in any IANA zone, "now", and diffs.
//! Pure except `time_now`, which reads the system clock (no network).

use crate::server::ReclaimServer;
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, JsonSchema)]
pub struct TimeOut {
    /// RFC3339 timestamp rendered in the requested timezone.
    pub rfc3339: String,
    pub epoch_ms: i64,
    /// `format`-rendered string when a pattern was given, else equals rfc3339.
    pub formatted: String,
    pub tz: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DiffOut {
    pub seconds: i64,
    pub human: String,
}

fn parse_tz(name: &str) -> Result<Tz, String> {
    name.parse::<Tz>().map_err(|_| format!("unknown timezone: {name}"))
}

/// Parse a `value` of kind `from` into a UTC instant.
fn parse_instant(value: &str, from: &str) -> Result<DateTime<Utc>, String> {
    match from {
        "epoch_s" => {
            let s: i64 = value.parse().map_err(|_| format!("not an integer: {value}"))?;
            Utc.timestamp_opt(s, 0)
                .single()
                .ok_or_else(|| format!("epoch seconds out of range: {value}"))
        }
        "epoch_ms" => {
            let ms: i64 = value.parse().map_err(|_| format!("not an integer: {value}"))?;
            Utc.timestamp_millis_opt(ms)
                .single()
                .ok_or_else(|| format!("epoch millis out of range: {value}"))
        }
        "epoch_us" => {
            let us: i64 = value.parse().map_err(|_| format!("not an integer: {value}"))?;
            Utc.timestamp_micros(us)
                .single()
                .ok_or_else(|| format!("epoch micros out of range: {value}"))
        }
        "rfc3339" => DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| format!("invalid rfc3339: {e}")),
        other => Err(format!("unknown `from`: {other}")),
    }
}

fn render(dt_utc: DateTime<Utc>, tz: Tz, tz_name: &str, format: Option<&str>) -> TimeOut {
    let local = dt_utc.with_timezone(&tz);
    let rfc3339 = local.to_rfc3339();
    let formatted = match format {
        Some(f) => local.format(f).to_string(),
        None => rfc3339.clone(),
    };
    TimeOut {
        rfc3339,
        epoch_ms: dt_utc.timestamp_millis(),
        formatted,
        tz: tz_name.to_string(),
    }
}

pub fn time_convert(
    value: &str,
    from: &str,
    to_tz: &str,
    format: Option<&str>,
) -> Result<TimeOut, String> {
    let dt = parse_instant(value, from)?;
    let tz = parse_tz(to_tz)?;
    Ok(render(dt, tz, to_tz, format))
}

pub fn time_now(tz_name: &str, format: Option<&str>) -> Result<TimeOut, String> {
    let tz = parse_tz(tz_name)?;
    Ok(render(Utc::now(), tz, tz_name, format))
}

/// Auto-detect each side: a bare integer is epoch seconds; otherwise RFC3339.
fn parse_either(s: &str) -> Result<DateTime<Utc>, String> {
    if s.trim().parse::<i64>().is_ok() {
        parse_instant(s.trim(), "epoch_s")
    } else {
        parse_instant(s, "rfc3339")
    }
}

fn humanize(total: i64) -> String {
    let sign = if total < 0 { "-" } else { "" };
    let mut s = total.abs();
    let d = s / 86_400;
    s %= 86_400;
    let h = s / 3_600;
    s %= 3_600;
    let m = s / 60;
    let sec = s % 60;
    format!("{sign}{d}d {h}h {m}m {sec}s")
}

pub fn time_diff(a: &str, b: &str) -> Result<DiffOut, String> {
    let ta = parse_either(a)?;
    let tb = parse_either(b)?;
    let seconds = (tb - ta).num_seconds();
    Ok(DiffOut { seconds, human: humanize(seconds) })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertReq {
    pub value: String,
    /// epoch_s | epoch_ms | epoch_us | rfc3339
    pub from: String,
    /// IANA timezone name, e.g. "America/New_York".
    pub to_tz: String,
    /// Optional strftime pattern.
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NowReq {
    pub tz: String,
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiffReq {
    /// epoch seconds or RFC3339.
    pub a: String,
    pub b: String,
}

#[tool_router(router = time_router, vis = "pub(crate)")]
impl ReclaimServer {
    #[tool(name = "time_convert", description = "Convert a timestamp (epoch_s/epoch_ms/epoch_us/rfc3339) into RFC3339 in any IANA timezone, with optional strftime formatting.")]
    pub async fn time_convert(&self, p: Parameters<ConvertReq>) -> Result<Json<TimeOut>, String> {
        Ok(Json(time_convert(&p.0.value, &p.0.from, &p.0.to_tz, p.0.format.as_deref())?))
    }

    #[tool(name = "time_now", description = "Current time in the given IANA timezone (reads system clock; no network).")]
    pub async fn time_now(&self, p: Parameters<NowReq>) -> Result<Json<TimeOut>, String> {
        Ok(Json(time_now(&p.0.tz, p.0.format.as_deref())?))
    }

    #[tool(name = "time_diff", description = "Difference b - a between two timestamps (epoch seconds or RFC3339). Returns total seconds and a human string.")]
    pub async fn time_diff(&self, p: Parameters<DiffReq>) -> Result<Json<DiffOut>, String> {
        Ok(Json(time_diff(&p.0.a, &p.0.b)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_seconds_to_utc_rfc3339() {
        let out = time_convert("0", "epoch_s", "UTC", None).unwrap();
        assert_eq!(out.rfc3339, "1970-01-01T00:00:00+00:00");
        assert_eq!(out.epoch_ms, 0);
        assert_eq!(out.tz, "UTC");
    }

    #[test]
    fn epoch_ms_to_named_zone() {
        // 1_000_000_000_000 ms = 2001-09-09T01:46:40Z
        let out = time_convert("1000000000000", "epoch_ms", "America/New_York", None).unwrap();
        assert!(out.rfc3339.starts_with("2001-09-08T21:46:40"));
        assert_eq!(out.epoch_ms, 1_000_000_000_000);
    }

    #[test]
    fn rfc3339_to_epoch_and_custom_format() {
        let out = time_convert(
            "2001-09-09T01:46:40+00:00",
            "rfc3339",
            "UTC",
            Some("%Y/%m/%d %H:%M"),
        )
        .unwrap();
        assert_eq!(out.epoch_ms, 1_000_000_000_000);
        assert_eq!(out.formatted, "2001/09/09 01:46");
    }

    #[test]
    fn unknown_timezone_is_an_error() {
        assert!(time_convert("0", "epoch_s", "Mars/Olympus", None).is_err());
    }

    #[test]
    fn bad_value_is_an_error() {
        assert!(time_convert("not-a-number", "epoch_s", "UTC", None).is_err());
        assert!(time_convert("nonsense", "rfc3339", "UTC", None).is_err());
    }

    #[test]
    fn now_returns_a_valid_structure() {
        let out = time_now("UTC", None).unwrap();
        assert_eq!(out.tz, "UTC");
        assert!(out.epoch_ms > 1_700_000_000_000); // after 2023-11
    }

    #[test]
    fn diff_is_b_minus_a_and_human_readable() {
        let d = time_diff("0", "90061").unwrap(); // 1d 1h 1m 1s
        assert_eq!(d.seconds, 90061);
        assert_eq!(d.human, "1d 1h 1m 1s");
        let neg = time_diff("90061", "0").unwrap();
        assert_eq!(neg.seconds, -90061);
        assert_eq!(neg.human, "-1d 1h 1m 1s");
    }

    #[test]
    fn time_router_lists_three_tools() {
        let names: Vec<String> = ReclaimServer::time_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in ["time_convert", "time_now", "time_diff"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }
}
