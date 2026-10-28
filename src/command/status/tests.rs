use chrono::{DateTime, Local, NaiveTime};

use super::format::{format_bytes, format_tried_at, table_lines};
use super::print::pretool_flag;
use crate::command::ttl::{format_duration_ms, window_with_cap};

#[test]
fn format_duration_ms_uses_exact_units() {
    assert_eq!(format_duration_ms(3_600_000), "1h");
    assert_eq!(format_duration_ms(5_400_000), "1h30m");
    assert_eq!(format_duration_ms(45_000), "45s");
    assert_eq!(format_duration_ms(500), "500ms");
}

#[test]
fn table_lines_align_columns_without_trailing_spaces() {
    let header = vec!["KEY".to_string(), "TTL".to_string()];
    let rows = vec![
        vec!["OS_PASSWORD".to_string(), "4m left".to_string()],
        vec!["A".to_string(), String::new()],
    ];
    assert_eq!(
        table_lines(&header, &rows),
        vec!["KEY          TTL", "OS_PASSWORD  4m left", "A"]
    );
}

fn local_at(now: DateTime<Local>, days_ago: u64, hour: u32, minute: u32) -> DateTime<Local> {
    let date = now
        .date_naive()
        .checked_sub_days(chrono::Days::new(days_ago))
        .unwrap();
    let time = NaiveTime::from_hms_opt(hour, minute, 0).unwrap();
    date.and_time(time)
        .and_local_timezone(Local)
        .earliest()
        .unwrap()
}

#[test]
fn pretool_flag_marks_drift() {
    assert_eq!(pretool_flag(true, true), "current");
    assert_eq!(pretool_flag(true, false), "not current");
    assert_eq!(pretool_flag(false, false), "missing");
    assert_eq!(pretool_flag(false, true), "missing");
}

#[test]
fn format_bytes_uses_human_units() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(1023), "1023 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
}

#[test]
fn tried_at_today_includes_local_time() {
    let now = Local::now();
    let checked = local_at(now, 0, 14, 25);
    assert_eq!(
        format_tried_at(checked.with_timezone(&chrono::Utc), now),
        format!("tried today at {}", checked.format("%H:%M"))
    );
}

#[test]
fn tried_at_yesterday_includes_local_time() {
    let now = Local::now();
    let checked = local_at(now, 1, 9, 5);
    assert_eq!(
        format_tried_at(checked.with_timezone(&chrono::Utc), now),
        format!("tried yesterday at {}", checked.format("%H:%M"))
    );
}

#[test]
fn tried_at_older_uses_calendar_date() {
    let now = Local::now();
    let checked = local_at(now, 3, 18, 0);
    assert_eq!(
        format_tried_at(checked.with_timezone(&chrono::Utc), now),
        format!(
            "tried {} at {}",
            checked.format("%d %b %Y"),
            checked.format("%H:%M")
        )
    );
}

#[test]
fn window_with_cap_says_when_max_cache_cuts_it() {
    assert_eq!(window_with_cap("8h", None), "8h");
    assert_eq!(window_with_cap("30m", Some(3_600_000)), "30m");
    assert_eq!(
        window_with_cap("8h", Some(3_600_000)),
        "8h, capped to 1h by max_cache"
    );
    assert_eq!(window_with_cap("8h", Some(0)), "8h, cache off by max_cache");
    assert_eq!(window_with_cap("off", Some(3_600_000)), "off");
}
