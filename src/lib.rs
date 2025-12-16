pub mod calendar;
pub mod ics;
pub mod proto;

use crate::proto::gtfs_realtime::FeedMessage;
use protobuf::Message;

pub async fn fetch_mta_events()
-> Result<Vec<calendar::CalendarEvent>, Box<dyn std::error::Error + Send + Sync>> {
    let url = "https://api-endpoint.mta.info/Dataservice/mtagtfsfeeds/camsys%2Fsubway-alerts";

    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    let feed_message = FeedMessage::parse_from_bytes(&bytes)?;
    let events = calendar::proto_feed_to_events(&feed_message);

    Ok(events)
}

pub async fn generate_train_ics(
    train_name: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let events = fetch_mta_events().await?;

    let train_upper = train_name.to_uppercase();
    let filtered_events: Vec<_> = events
        .into_iter()
        .filter(|event| {
            event
                .routes
                .iter()
                .any(|route| route.to_uppercase() == train_upper)
        })
        .collect();

    Ok(ics::generate_ics_with_name(
        &filtered_events,
        Some(train_name),
    ))
}
