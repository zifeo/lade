use chrono::{DateTime, Duration, Months, Utc};

pub fn parse_window(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    if let Some(number) = raw.strip_suffix("month") {
        let n: u32 = parse_amount(number, raw)?;
        if n == 0 {
            return Err("duration must be greater than 0".to_string());
        }
        let now = Utc::now();
        let then = now
            .checked_sub_months(Months::new(n))
            .ok_or_else(|| format!("invalid duration '{raw}'"))?;
        return Ok(now - then);
    }
    let (number, unit) = if let Some(number) = raw.strip_suffix('w') {
        (number, "w")
    } else if let Some(number) = raw.strip_suffix('d') {
        (number, "d")
    } else if let Some(number) = raw.strip_suffix('h') {
        (number, "h")
    } else if let Some(number) = raw.strip_suffix('m') {
        (number, "m")
    } else if let Some(number) = raw.strip_suffix('s') {
        (number, "s")
    } else {
        return Err("use a duration like 30m, 2h, 7d, 1w, or 1month".to_string());
    };
    let amount: i64 = parse_amount(number, raw)?;
    if amount <= 0 {
        return Err("duration must be greater than 0".to_string());
    }
    Ok(match unit {
        "s" => Duration::seconds(amount),
        "m" => Duration::minutes(amount),
        "h" => Duration::hours(amount),
        "d" => Duration::days(amount),
        "w" => Duration::weeks(amount),
        _ => unreachable!(),
    })
}

fn parse_amount<T: std::str::FromStr>(number: &str, raw: &str) -> Result<T, String> {
    number
        .trim()
        .parse()
        .map_err(|_| format!("invalid duration '{raw}'"))
}

pub fn cutoff(raw: &str) -> Result<DateTime<Utc>, String> {
    let dur = parse_window(raw)?;
    Utc::now()
        .checked_sub_signed(dur)
        .ok_or_else(|| format!("invalid duration '{raw}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_are_not_months() {
        assert!(parse_window("30m").unwrap() < parse_window("1h").unwrap());
        assert!(parse_window("1month").unwrap() > parse_window("7d").unwrap());
    }

    #[test]
    fn rejects_zero_and_bare() {
        assert!(parse_window("0s").is_err());
        assert!(parse_window("5").is_err());
    }
}
