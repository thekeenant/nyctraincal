use crate::calendar::CalendarEvent;
use chrono::{DateTime, Utc};

pub fn generate_ics(events: &[CalendarEvent]) -> String {
    generate_ics_with_name(events, None)
}

pub fn generate_ics_with_name(events: &[CalendarEvent], train_name: Option<&str>) -> String {
    let mut ics = String::new();

    let (cal_name, cal_desc) = if let Some(train) = train_name {
        (
            format!("MTA {} Train Alerts", train.to_uppercase()),
            format!(
                "Real-time alerts and planned service changes for MTA {} train",
                train.to_uppercase()
            ),
        )
    } else {
        (
            "MTA Subway Alerts".to_string(),
            "Real-time alerts and planned service changes for MTA Subway".to_string(),
        )
    };

    // ICS header
    ics.push_str("BEGIN:VCALENDAR\r\n");
    ics.push_str("VERSION:2.0\r\n");
    ics.push_str("PRODID:-//NYC TRAIN CAL//MTA Subway Alerts//EN\r\n");
    ics.push_str("CALSCALE:GREGORIAN\r\n");
    ics.push_str("METHOD:PUBLISH\r\n");
    ics.push_str(&format!("X-WR-CALNAME:{}\r\n", cal_name));
    ics.push_str("X-WR-TIMEZONE:America/New_York\r\n");
    ics.push_str(&format!("X-WR-CALDESC:{}\r\n", cal_desc));

    for event in events {
        ics.push_str(&generate_event(event));
    }

    ics.push_str("END:VCALENDAR\r\n");

    ics
}

fn generate_event(event: &CalendarEvent) -> String {
    let mut vevent = String::new();

    vevent.push_str("BEGIN:VEVENT\r\n");

    vevent.push_str(&fold_line(&format!("UID:{}@nyctraincal", event.uid)));
    vevent.push_str("\r\n");

    let created = format_datetime(&event.created_at);
    let updated = format_datetime(&event.updated_at);
    vevent.push_str(&fold_line(&format!("CREATED:{}", created)));
    vevent.push_str("\r\n");
    vevent.push_str(&fold_line(&format!("LAST-MODIFIED:{}", updated)));
    vevent.push_str("\r\n");
    vevent.push_str(&fold_line(&format!("DTSTAMP:{}", updated)));
    vevent.push_str("\r\n");

    let start = format_datetime(&event.start);
    vevent.push_str(&fold_line(&format!("DTSTART:{}", start)));
    vevent.push_str("\r\n");

    if let Some(end) = &event.end {
        let end_str = format_datetime(end);
        vevent.push_str(&fold_line(&format!("DTEND:{}", end_str)));
        vevent.push_str("\r\n");
    } else {
        let end = event.start + chrono::Duration::hours(1);
        let end_str = format_datetime(&end);
        vevent.push_str(&fold_line(&format!("DTEND:{}", end_str)));
        vevent.push_str("\r\n");
    }

    vevent.push_str(&fold_line(&format!(
        "SUMMARY:{}",
        escape_text(&event.summary)
    )));
    vevent.push_str("\r\n");

    if !event.description.is_empty() {
        vevent.push_str(&fold_line(&format!(
            "DESCRIPTION:{}",
            escape_text(&event.description)
        )));
        vevent.push_str("\r\n");
    }

    vevent.push_str(&fold_line(&format!(
        "CATEGORIES:{}",
        escape_text(&event.alert_type)
    )));
    vevent.push_str("\r\n");

    vevent.push_str(&fold_line(&format!(
        "X-MTA-ALERT-ID:{}",
        event.mta_alert_id
    )));
    vevent.push_str("\r\n");

    vevent.push_str("END:VEVENT\r\n");

    vevent
}

fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

fn escape_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

fn fold_line(line: &str) -> String {
    const MAX_LEN: usize = 75;

    if line.len() <= MAX_LEN {
        return line.to_string();
    }

    let mut result = String::new();
    let mut pos = 0;

    while pos < line.len() {
        let chunk_len = if pos == 0 { MAX_LEN } else { MAX_LEN - 1 };
        let end = (pos + chunk_len).min(line.len());

        let mut end = end;
        while end > pos && !line.is_char_boundary(end) {
            end -= 1;
        }

        if pos > 0 {
            result.push_str("\r\n ");
        }
        result.push_str(&line[pos..end]);
        pos = end;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use std::path::PathBuf;

    #[test]
    fn test_format_datetime() {
        let dt = Utc.with_ymd_and_hms(2025, 12, 15, 10, 30, 0).unwrap();
        assert_eq!(format_datetime(&dt), "20251215T103000Z");
    }

    #[test]
    fn test_escape_text() {
        assert_eq!(escape_text("Hello, World!"), "Hello\\, World!");
        assert_eq!(escape_text("Line 1\nLine 2"), "Line 1\\nLine 2");
        assert_eq!(escape_text("Semi;colon"), "Semi\\;colon");
    }

    #[test]
    fn test_fold_line() {
        let short = "SUMMARY:Short";
        assert_eq!(fold_line(short), short);

        let long = "DESCRIPTION:This is a very long description that exceeds seventy-five characters and should be folded";
        let folded = fold_line(long);

        for line in folded.split("\r\n") {
            assert!(line.len() <= 75, "Line too long: {} chars", line.len());
        }

        let unfolded = folded.replace("\r\n ", "");
        assert_eq!(unfolded, long);
    }

    #[test]
    fn test_generate_ics_basic() {
        use crate::calendar::CalendarEvent;

        let events = vec![CalendarEvent {
            uid: "test-event-1".to_string(),
            summary: "Test Event".to_string(),
            description: "Test Description".to_string(),
            start: Utc.with_ymd_and_hms(2025, 12, 15, 10, 0, 0).unwrap(),
            end: Some(Utc.with_ymd_and_hms(2025, 12, 15, 11, 0, 0).unwrap()),
            created_at: Utc.with_ymd_and_hms(2025, 12, 14, 9, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2025, 12, 15, 8, 0, 0).unwrap(),
            mta_alert_id: "123".to_string(),
            routes: vec!["L".to_string()],
            alert_type: "Delays".to_string(),
        }];

        let ics = generate_ics(&events);

        assert!(ics.contains("BEGIN:VCALENDAR"));
        assert!(ics.contains("END:VCALENDAR"));
        assert!(ics.contains("VERSION:2.0"));
        assert!(ics.contains("PRODID:-//NYC TRAIN CAL//MTA Subway Alerts//EN"));
        assert!(ics.contains("SUMMARY:Test Event"));
    }

    #[rstest]
    fn test_golden_ics_from_protobuf(#[files("tests/**/*.pb")] path: PathBuf) {
        use crate::calendar::proto_feed_to_events;
        use crate::proto::gtfs_realtime::FeedMessage;
        use protobuf::Message;
        use std::fs;

        // Read golden binary protobuf file
        let pb_bytes =
            fs::read(&path).unwrap_or_else(|e| panic!("Failed to read {:?}: {}", path, e));

        // Parse binary protobuf
        let feed: FeedMessage = FeedMessage::parse_from_bytes(&pb_bytes)
            .unwrap_or_else(|e| panic!("Failed to parse protobuf from {:?}: {}", path, e));

        // Convert to calendar events
        let events = proto_feed_to_events(&feed);

        // Generate ICS
        let generated_ics = generate_ics(&events);

        // Derive the corresponding .ics file path
        let ics_path = path.with_extension("ics");

        // Read golden ICS
        let golden_ics = fs::read_to_string(&ics_path)
            .unwrap_or_else(|e| panic!("Failed to read golden ICS {:?}: {}", ics_path, e));

        // Compare - they should be identical
        assert_eq!(
            generated_ics, golden_ics,
            "Generated ICS does not match golden file for {:?}",
            path
        );
    }
}
