use nyc_train_time::calendar::proto_feed_to_events;
use nyc_train_time::ics::generate_ics;
use nyc_train_time::proto::gtfs_realtime::FeedMessage;
use protobuf::Message;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tests_dir = Path::new("tests");

    let entries = fs::read_dir(tests_dir)?;

    let mut pb_files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("pb") {
            pb_files.push(path);
        }
    }

    if pb_files.is_empty() {
        println!("No .pb files found in tests/");
        return Ok(());
    }

    println!("Found {} .pb file(s) to convert", pb_files.len());

    for pb_path in pb_files {
        println!("\nProcessing: {}", pb_path.display());

        let pb_bytes = fs::read(&pb_path)?;
        let feed = FeedMessage::parse_from_bytes(&pb_bytes)?;
        println!("  Parsed {} entities", feed.entity.len());

        let events = proto_feed_to_events(&feed);
        println!("  Converted to {} calendar events", events.len());

        let ics = generate_ics(&events);
        let ics_path = pb_path.with_extension("ics");
        fs::write(&ics_path, ics)?;
        println!("  Saved ICS to {}", ics_path.display());
    }

    println!("\nDone!");
    Ok(())
}
