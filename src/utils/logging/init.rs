use std::io::Write;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use log::LevelFilter;

pub fn init(level: Option<&str>) {
    let default_level = if cfg!(debug_assertions) {
        LevelFilter::Trace
    } else {
        LevelFilter::Warn
    };
    let filter_level = level
        .and_then(|value| LevelFilter::from_str(value.trim()).ok())
        .unwrap_or(default_level);

    let mut builder = env_logger::Builder::new();
    builder.filter_level(filter_level);
    // An explicit --log-level takes precedence over RUST_LOG.
    if level.is_none() {
        builder.parse_default_env();
    }
    builder
        .format(|buffer, record| {
            writeln!(
                buffer,
                "| {} | {:<5} | {} | {}",
                format_timestamp(),
                record.level(),
                short_source(record.target()),
                record.args()
            )
        })
        .init();
}

pub(crate) fn short_source(target: &str) -> &str {
    target.rsplit("::").next().unwrap_or(target)
}

pub(crate) fn format_timestamp() -> String {
    let elapsed_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = elapsed_since_epoch.as_secs();
    let milliseconds = elapsed_since_epoch.subsec_millis();
    let days_since_epoch = total_seconds / 86_400;
    let seconds_in_day = total_seconds % 86_400;
    let hours = seconds_in_day / 3600;
    let minutes = (seconds_in_day % 3600) / 60;
    let seconds = seconds_in_day % 60;
    let (year, month, day) = civil_date_from_days(days_since_epoch as i64);
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}.{milliseconds:03}Z")
}

fn civil_date_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let shifted_days = days_since_epoch + 719_468;
    let era = if shifted_days >= 0 {
        shifted_days
    } else {
        shifted_days - 146_096
    } / 146_097;
    let day_of_era = shifted_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = (if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    }) as u32;
    let calendar_year = if month <= 2 { year + 1 } else { year };
    (calendar_year, month, day)
}
