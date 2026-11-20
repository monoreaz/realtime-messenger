//! Date/time and size formatting, matching the web client's helpers in `App.tsx`.

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};

/// `formatTime`: two-digit hour and minute in local time.
pub fn time(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%H:%M").to_string()
}

fn local_date(at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&Local).date_naive()
}

pub fn same_day(a: DateTime<Utc>, b: DateTime<Utc>) -> bool {
    local_date(a) == local_date(b)
}

/// `formatChatListTime`: time today, "Yesterday", weekday within 6 days, else a date.
pub fn chat_list_time(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let date = local_date(at);
    let today = local_date(now);
    if date == today {
        return time(at);
    }
    if today.pred_opt() == Some(date) {
        return "Yesterday".to_owned();
    }
    if now.signed_duration_since(at).num_seconds() < 6 * 24 * 60 * 60 && date < today {
        return date.format("%a").to_string();
    }
    if date.year() == today.year() { date.format("%-d %b").to_string() } else { date.format("%-d %b %y").to_string() }
}

/// `formatMessageDate`: "Today", "Yesterday" or "17 November 2026".
pub fn message_date(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let date = local_date(at);
    let today = local_date(now);
    if date == today {
        "Today".to_owned()
    } else if today.pred_opt() == Some(date) {
        "Yesterday".to_owned()
    } else {
        date.format("%-d %B %Y").to_string()
    }
}

/// `formatLastSeen` (web `lastSeen.ts`): "Last seen just now", "… 5 minutes ago",
/// "… today at 14:05", "… yesterday at 09:30", "… 3 Nov" (with the year if not this year).
pub fn last_seen(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let elapsed = now.signed_duration_since(at).num_seconds();
    if elapsed < 60 {
        return "Last seen just now".to_owned();
    }
    if elapsed < 60 * 60 {
        let minutes = elapsed / 60;
        return format!("Last seen {minutes} {} ago", if minutes == 1 { "minute" } else { "minutes" });
    }
    let (date, today) = (local_date(at), local_date(now));
    if date == today {
        return format!("Last seen today at {}", time(at));
    }
    if today.pred_opt() == Some(date) {
        return format!("Last seen yesterday at {}", time(at));
    }
    if date.year() == today.year() { format!("Last seen {}", date.format("%-d %b")) } else { format!("Last seen {}", date.format("%-d %b %Y")) }
}

/// `formatSessionDate`.
pub fn session_date(at: Option<DateTime<Utc>>) -> String {
    match at {
        Some(at) => at.with_timezone(&Local).format("%-d %b %Y, %H:%M").to_string(),
        None => "Never".to_owned(),
    }
}

pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
}

/// `getSessionDeviceName`, extended with this desktop client.
pub fn device_name(user_agent: Option<&str>) -> String {
    let Some(agent) = user_agent else { return "Unknown device".to_owned() };
    let app = if agent.starts_with("MessengerDesktop/") {
        "Messenger Desktop"
    } else if agent.contains("Edg/") {
        "Edge"
    } else if agent.contains("Firefox/") {
        "Firefox"
    } else if agent.contains("Chrome/") {
        "Chrome"
    } else if agent.contains("Safari/") {
        "Safari"
    } else {
        "Browser"
    };
    let os = if agent.contains("Windows") || agent.contains("windows") {
        "Windows"
    } else if agent.contains("Macintosh") || agent.contains("Mac OS") {
        "macOS"
    } else if agent.contains("Android") {
        "Android"
    } else if agent.contains("iPhone") || agent.contains("iPad") {
        "iOS"
    } else if agent.contains("Linux") || agent.contains("linux") {
        "Linux"
    } else {
        "Unknown OS"
    };
    format!("{app} on {os}")
}

/// Up to two initials from a display name (`avatarInitials`).
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.trim().split(|c: char| c.is_whitespace() || matches!(c, '.' | '_' | '-')).filter(|w| !w.is_empty()).collect();
    let first = |word: &str| word.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    match words.as_slice() {
        [] => "?".to_owned(),
        [only] => first(only),
        [a, b, ..] => first(a) + &first(b),
    }
}

/// Parsed call history text (`CallMessage.tsx`).
pub struct CallSummary {
    pub completed: bool,
    pub detail: String,
}

pub fn call_summary(content: &str) -> CallSummary {
    let completed = content.starts_with("Audio call · ") && content.ends_with(" sec") && content.contains(" min ");
    let detail = content
        .strip_prefix("Audio call · ")
        .or_else(|| content.strip_prefix("Audio call "))
        .or_else(|| content.strip_prefix("Audio call"))
        .unwrap_or(content)
        .to_owned();
    let detail = if completed {
        detail
    } else if content == "Missed audio call" {
        "Missed call".to_owned()
    } else if detail.is_empty() {
        "Call ended".to_owned()
    } else {
        capitalize(&detail)
    };
    CallSummary { completed, detail }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Truncates on a char boundary and adds an ellipsis (for one-line previews).
pub fn ellipsize(text: &str, max_chars: usize) -> String {
    let single_line: String = text.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    if single_line.chars().count() <= max_chars {
        single_line
    } else {
        let mut cut: String = single_line.chars().take(max_chars.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    #[test]
    fn initials_match_web() {
        assert_eq!(initials("Emily Carter"), "EC");
        assert_eq!(initials("alex"), "A");
        assert_eq!(initials("john_doe"), "JD");
        assert_eq!(initials("   "), "?");
        assert_eq!(initials("élan vital"), "ÉV");
    }

    #[test]
    fn relative_dates() {
        let now = Local.with_ymd_and_hms(2026, 11, 18, 15, 0, 0).unwrap().with_timezone(&Utc);
        assert_eq!(message_date(now - Duration::hours(1), now), "Today");
        assert_eq!(message_date(now - Duration::days(1), now), "Yesterday");
        assert_eq!(message_date(now - Duration::days(30), now), "19 October 2026");
        assert_eq!(chat_list_time(now - Duration::days(1), now), "Yesterday");
        assert_eq!(chat_list_time(now - Duration::days(3), now), "Sun");
        assert_eq!(chat_list_time(now - Duration::days(20), now), "29 Oct");
        assert_eq!(chat_list_time(now - Duration::days(400), now), "14 Oct 25");
    }

    #[test]
    fn last_seen_wording() {
        let now = Local.with_ymd_and_hms(2026, 11, 18, 15, 0, 0).unwrap().with_timezone(&Utc);
        assert_eq!(last_seen(now - Duration::seconds(20), now), "Last seen just now");
        assert_eq!(last_seen(now - Duration::minutes(1), now), "Last seen 1 minute ago");
        assert_eq!(last_seen(now - Duration::minutes(42), now), "Last seen 42 minutes ago");
        assert_eq!(last_seen(now - Duration::hours(3), now), "Last seen today at 12:00");
        assert_eq!(last_seen(now - Duration::hours(20), now), "Last seen yesterday at 19:00");
        assert_eq!(last_seen(now - Duration::days(15), now), "Last seen 3 Nov");
        assert_eq!(last_seen(now - Duration::days(400), now), "Last seen 14 Oct 2025");
    }

    #[test]
    fn call_summaries() {
        let done = call_summary("Audio call · 2 min 05 sec");
        assert!(done.completed);
        assert_eq!(done.detail, "2 min 05 sec");
        assert_eq!(call_summary("Missed audio call").detail, "Missed call");
        assert_eq!(call_summary("Audio call declined").detail, "Declined");
        assert_eq!(call_summary("Audio call · Busy").detail, "Busy");
    }

    #[test]
    fn devices_and_sizes() {
        assert_eq!(device_name(Some("MessengerDesktop/0.1.0 (Windows)")), "Messenger Desktop on Windows");
        assert_eq!(device_name(Some("Mozilla/5.0 (Windows NT 10.0) Chrome/120 Safari/537")), "Chrome on Windows");
        assert_eq!(device_name(None), "Unknown device");
        assert_eq!(megabytes(1_572_864), "1.5 MB");
        assert_eq!(ellipsize("hello\nworld", 20), "hello world");
        assert_eq!(ellipsize("abcdef", 4), "abc…");
    }
}
