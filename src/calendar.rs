use crate::proto::gtfs_realtime::translated_string::Translation as ProtoTranslation;
use crate::proto::gtfs_realtime::{FeedEntity, FeedMessage};
use crate::proto::gtfs_realtime_service_status;
use chrono::{DateTime, TimeZone, Utc};

pub fn proto_feed_to_events(feed: &FeedMessage) -> Vec<CalendarEvent> {
    let default_time = feed
        .header
        .as_ref()
        .and_then(|h| Utc.timestamp_opt(h.timestamp() as i64, 0).single())
        .unwrap_or_else(Utc::now);

    feed.entity
        .iter()
        .flat_map(|e| proto_entity_to_events(e, default_time))
        .collect()
}

fn proto_entity_to_events(entity: &FeedEntity, default_time: DateTime<Utc>) -> Vec<CalendarEvent> {
    let alert = match entity.alert.as_ref() {
        Some(a) => a,
        None => return vec![],
    };

    let routes: Vec<String> = alert
        .informed_entity
        .iter()
        .filter_map(|e| e.route_id.as_ref().map(|s| s.to_string()))
        .collect();

    let route_str = if routes.is_empty() {
        String::from("MTA")
    } else {
        routes.join(", ")
    };

    let alert_type_str = gtfs_realtime_service_status::exts::mercury_alert
        .get(alert)
        .and_then(|mercury| mercury.alert_type.as_ref().map(|s| s.to_string()))
        .unwrap_or_else(|| "Alert".to_string());

    let summary = format!("{}: {}", route_str, alert_type_str);

    let mut description = String::new();

    if let Some(header_text) = find_proto_plain_text(&alert.header_text.translation) {
        description.push_str(&process_text(header_text));
    }

    if let Some(desc) = alert.description_text.as_ref()
        && let Some(desc_text) = find_proto_plain_text(&desc.translation)
    {
        if !description.is_empty() {
            description.push_str("\n\n");
        }
        description.push_str(&process_text(desc_text));
    }

    let (created_at, updated_at) = gtfs_realtime_service_status::exts::mercury_alert
        .get(alert)
        .and_then(|mercury| {
            let created = Utc.timestamp_opt(mercury.created_at() as i64, 0).single()?;
            let updated = Utc.timestamp_opt(mercury.updated_at() as i64, 0).single()?;
            Some((created, updated))
        })
        .unwrap_or((default_time, default_time));

    let active_periods = &alert.active_period;
    if active_periods.is_empty() {
        return vec![CalendarEvent {
            uid: format!("mta-alert-{}", entity.id()),
            summary: summary.clone(),
            description: description.clone(),
            start: default_time,
            end: None,
            created_at,
            updated_at,
            mta_alert_id: entity.id().to_string(),
            routes: routes.clone(),
            alert_type: alert_type_str.clone(),
        }];
    }

    active_periods
        .iter()
        .enumerate()
        .filter_map(|(idx, period)| {
            let start = Utc.timestamp_opt(period.start() as i64, 0).single()?;
            let end = if period.has_end() {
                Utc.timestamp_opt(period.end() as i64, 0).single()
            } else {
                None
            };

            let uid = if active_periods.len() > 1 {
                format!("mta-alert-{}-{}", entity.id(), idx)
            } else {
                format!("mta-alert-{}", entity.id())
            };

            Some(CalendarEvent {
                uid,
                summary: summary.clone(),
                description: description.clone(),
                start,
                end,
                created_at,
                updated_at,
                mta_alert_id: entity.id().to_string(),
                routes: routes.clone(),
                alert_type: alert_type_str.clone(),
            })
        })
        .collect()
}

fn find_proto_plain_text(translations: &[ProtoTranslation]) -> Option<&str> {
    translations
        .iter()
        .find(|t| t.language.as_deref() == Some("en"))
        .and_then(|t| t.text.as_deref())
        .or_else(|| {
            translations
                .iter()
                .find(|t| {
                    t.language
                        .as_ref()
                        .map(|s| !s.contains("html"))
                        .unwrap_or(false)
                })
                .and_then(|t| t.text.as_deref())
        })
        .or_else(|| translations.first().and_then(|t| t.text.as_deref()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalendarEvent {
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub mta_alert_id: String,
    pub routes: Vec<String>,
    pub alert_type: String,
}

fn process_text(text: &str) -> String {
    text.replace("[shuttle bus icon]", "🚌")
        .replace("[accessibility icon]", "♿")
        .replace("[elevator icon]", "🛗")
        .replace("[escalator icon]", "🚶")
        .replace("[stairs icon]", "🪜")
        .replace("[train icon]", "🚇")
        .replace("[bus icon]", "🚌")
        .replace("[ferry icon]", "⛴️")
        .replace("[bicycle icon]", "🚲")
        .replace("[parking icon]", "🅿️")
        .replace("[warning icon]", "⚠️")
        .replace("[alert icon]", "🚨")
        .replace("[construction icon]", "🚧")
        .replace("[detour icon]", "↪️")
        .replace(['\u{200C}', '\u{200B}', '\u{200D}', '\u{FEFF}'], "")
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<p>", "")
        .replace("</p>", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<ul>", "\n")
        .replace("</ul>", "\n")
        .replace("<li>", "• ")
        .replace("</li>", "\n")
        .replace("<strong>", "")
        .replace("</strong>", "")
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_text_icons() {
        assert_eq!(process_text("[shuttle bus icon]"), "🚌");
        assert_eq!(process_text("[accessibility icon]"), "♿");
        assert_eq!(
            process_text("Take [shuttle bus icon] to station [accessibility icon]"),
            "Take 🚌 to station ♿"
        );
    }
}
