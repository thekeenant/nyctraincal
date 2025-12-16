use nyc_train_time::proto::gtfs_realtime::FeedMessage;
use protobuf::Message;
use std::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://api-endpoint.mta.info/Dataservice/mtagtfsfeeds/camsys%2Fsubway-alerts";
    println!("Fetching MTA subway alerts from: {}", url);

    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    let feed = FeedMessage::parse_from_bytes(&bytes)?;
    println!("Successfully parsed protobuf feed!");
    println!("Number of entities: {}", feed.entity.len());

    fs::write("tests/golden-2025-12-15.pb", bytes.as_ref())?;
    println!("Saved binary protobuf to tests/golden-2025-12-15.pb");

    Ok(())
}
