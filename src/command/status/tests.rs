use chrono::{DateTime, Local, NaiveTime};

use super::print::{format_bytes, format_tried_at, pretool_flag};

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
fn pretool_flag_marks_stale() {
    assert_eq!(pretool_flag(true, true), "yes");
    assert_eq!(pretool_flag(true, false), "yes (stale)");
    assert_eq!(pretool_flag(false, false), "no");
    assert_eq!(pretool_flag(false, true), "no");
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
