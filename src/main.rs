#![deny(clippy::print_stdout, clippy::print_stderr)]

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use moka::future::Cache;
use std::net::SocketAddr;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Clone)]
struct AppState {
    cache: Cache<String, String>,
    gtag_snippet: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    // Cache for 30 seconds - reduces MTA API calls significantly
    let cache = Cache::builder()
        .max_capacity(100)
        .time_to_live(Duration::from_secs(30))
        .build();

    let gtag_id = required_gtag_id_from_env().map_err(std::io::Error::other)?;
    let gtag_snippet = build_gtag_snippet(&gtag_id);

    let state = AppState {
        cache,
        gtag_snippet,
    };

    // Rate limiting: 10 requests per IP per second
    let governor_conf = GovernorConfigBuilder::default()
        .per_second(10)
        .burst_size(20)
        .finish()
        .ok_or("Failed to build governor config")?;

    let api = Router::new()
        .route(
            "/api/calendars/train/:train_name",
            get(handle_train_calendar),
        )
        .layer(
            ServiceBuilder::new()
                .layer(GovernorLayer {
                    config: governor_conf.into(),
                })
                .layer(tower::limit::ConcurrencyLimitLayer::new(50)),
        );

    let app = Router::new()
        .route("/", get(handle_index))
        .route("/favicon.svg", get(handle_favicon))
        .route("/favicon.ico", get(handle_favicon))
        .route("/trains/:train_name", get(handle_index_train))
        .merge(api)
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;

    info!("Server running on http://0.0.0.0:{port}");
    info!("Rate limit: 10 req/s per IP, 30s cache, max 50 concurrent requests");
    info!("Example: http://localhost:{port}/api/calendars/train/A.ics");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn handle_train_calendar(
    State(state): State<AppState>,
    Path(train_name): Path<String>,
) -> Response {
    let train_name = train_name.strip_suffix(".ics").unwrap_or(&train_name);

    const VALID_TRAINS: &[&str] = &[
        "A", "C", "E", "B", "D", "F", "M", "G", "J", "Z", "L", "N", "Q", "R", "W", "1", "2", "3",
        "4", "5", "6", "7", "S", "SI",
    ];

    if !VALID_TRAINS.contains(&train_name) {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Invalid train line: {}. Train lines are case-sensitive.",
                train_name
            ),
        )
            .into_response();
    }

    // Check cache first
    if let Some(cached_content) = state.cache.get(train_name).await {
        info!("Cache hit for train: {}", train_name);
        return (
            StatusCode::OK,
            [("Content-Type", "text/calendar; charset=utf-8")],
            cached_content,
        )
            .into_response();
    }

    info!("Cache miss - fetching calendar for train: {}", train_name);

    match nyc_train_time::generate_train_ics(train_name).await {
        Ok(ics_content) => {
            // Cache the result
            state
                .cache
                .insert(train_name.to_string(), ics_content.clone())
                .await;

            (
                StatusCode::OK,
                [("Content-Type", "text/calendar; charset=utf-8")],
                ics_content,
            )
                .into_response()
        }
        Err(e) => {
            error!("Error generating calendar: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error generating calendar: {}", e),
            )
                .into_response()
        }
    }
}

async fn handle_index(State(state): State<AppState>) -> Response {
    render_index_page(&state.gtag_snippet).into_response()
}

async fn handle_index_train(
    State(state): State<AppState>,
    Path(train_name): Path<String>,
) -> Response {
    let train = train_name.to_uppercase();
    if !is_valid_train(&train) {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    render_index_page(&state.gtag_snippet).into_response()
}

fn render_index_page(
    gtag_snippet: &str,
) -> (StatusCode, [(&'static str, &'static str); 1], String) {
    let html_template = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>NYC Train Cal — MTA Subway Service Alert Calendars</title>
    <meta name="description" content="Subscribe to live MTA subway service alerts as calendar feeds. Get planned service changes, weekend reroutes, and suspensions directly in Google Calendar, Apple Calendar, or Outlook — filtered by train line.">
    <!-- GTAG_PLACEHOLDER -->
    <link rel="icon" href="/favicon.svg" type="image/svg+xml">
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            max-width: 800px;
            margin: 0 auto;
            padding: 20px;
            line-height: 1.6;
        }
        h1 {
            color: #333;
            margin-top: 20px;
        }
        .line-group {
            margin: 30px 0;
        }
        .train-grid {
            display: flex;
            flex-wrap: wrap;
            gap: 12px;
            margin-bottom: 20px;
        }
        .train-link {
            display: flex;
            align-items: center;
            justify-content: center;
            width: 64px;
            height: 64px;
            font-weight: bold;
            font-size: 22px;
            border-radius: 50%;
            transition: transform 0.2s;
            cursor: pointer;
            border: none;
            text-decoration: none;
            flex-shrink: 0;
        }
        .train-link:hover {
            transform: scale(1.05);
        }
        .train-link.selected {
            box-shadow: 0 0 0 3px #333;
        }
        /* NYC Subway line colors */
        .train-1, .train-2, .train-3 { background-color: #ee352e; color: white; }
        .train-4, .train-5, .train-6 { background-color: #00933c; color: white; }
        .train-7 { background-color: #b933ad; color: white; }
        .train-A, .train-C, .train-E { background-color: #0039a6; color: white; }
        .train-B, .train-D, .train-F, .train-M { background-color: #ff6319; color: white; }
        .train-G { background-color: #6cbe45; color: white; }
        .train-J, .train-Z { background-color: #996633; color: white; }
        .train-L { background-color: #a7a9ac; color: white; }
        .train-N, .train-Q, .train-R, .train-W { background-color: #fccc0a; color: black; }
        .train-S, .train-SI { background-color: #808183; color: white; }
        .train-badge {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: 26px;
            height: 26px;
            border-radius: 50%;
            font-size: 12px;
            font-weight: bold;
            flex-shrink: 0;
            vertical-align: middle;
        }
        #subscribeSection { margin-top: 20px; }
        .subscribe-buttons { display: flex; flex-direction: column; gap: 10px; }
        .subscribe-btn {
            display: flex;
            align-items: center;
            gap: 10px;
            padding: 12px 18px;
            border-radius: 8px;
            text-decoration: none;
            font-size: 15px;
            font-weight: 600;
            color: #333;
            background: #f0f0f0;
            border: 1px solid #ddd;
            transition: background 0.15s;
        }
        .subscribe-btn:hover { background: #e0e0e0; }
        .about {
            background-color: #f5f5f5;
            padding: 20px;
            border-radius: 8px;
            margin-top: 30px;
        }
        @media (max-width: 600px) {
            body { padding: 15px; }
            h1 { font-size: 24px; margin-top: 10px; }
            .train-grid {
                grid-template-columns: repeat(auto-fill, minmax(60px, 1fr));
                gap: 8px;
            }
            .train-link { width: 52px; height: 52px; font-size: 18px; }
        }
    </style>
</head>
<body>
    <h1>🚇 NYC Train Cal — MTA Subway Alert Calendars</h1>
    <p>Select your train line below to subscribe to its live MTA service alerts as a calendar feed:</p>
    
    <div class="train-grid">
        <a class="train-link train-A" data-train="A" href="/trains/A">A</a>
        <a class="train-link train-C" data-train="C" href="/trains/C">C</a>
        <a class="train-link train-E" data-train="E" href="/trains/E">E</a>
        <a class="train-link train-B" data-train="B" href="/trains/B">B</a>
        <a class="train-link train-D" data-train="D" href="/trains/D">D</a>
        <a class="train-link train-F" data-train="F" href="/trains/F">F</a>
        <a class="train-link train-M" data-train="M" href="/trains/M">M</a>
        <a class="train-link train-G" data-train="G" href="/trains/G">G</a>
        <a class="train-link train-J" data-train="J" href="/trains/J">J</a>
        <a class="train-link train-Z" data-train="Z" href="/trains/Z">Z</a>
        <a class="train-link train-L" data-train="L" href="/trains/L">L</a>
        <a class="train-link train-N" data-train="N" href="/trains/N">N</a>
        <a class="train-link train-Q" data-train="Q" href="/trains/Q">Q</a>
        <a class="train-link train-R" data-train="R" href="/trains/R">R</a>
        <a class="train-link train-W" data-train="W" href="/trains/W">W</a>
        <a class="train-link train-1" data-train="1" href="/trains/1">1</a>
        <a class="train-link train-2" data-train="2" href="/trains/2">2</a>
        <a class="train-link train-3" data-train="3" href="/trains/3">3</a>
        <a class="train-link train-4" data-train="4" href="/trains/4">4</a>
        <a class="train-link train-5" data-train="5" href="/trains/5">5</a>
        <a class="train-link train-6" data-train="6" href="/trains/6">6</a>
        <a class="train-link train-7" data-train="7" href="/trains/7">7</a>
        <a class="train-link train-S" data-train="S" href="/trains/S">S</a>
        <a class="train-link train-SI" data-train="SI" href="/trains/SI">SI</a>
    </div>

    <div id="subscribeSection" style="display:none;"></div>

    <div class="about">
        <h2>How it works</h2>
        <p>NYC Train Cal pulls live service alerts directly from the MTA's real-time data feeds and converts them into iCalendar (.ics) files you can subscribe to from any calendar application. When you subscribe, your calendar app — whether that's Google Calendar, Apple Calendar, Microsoft Outlook, or Yahoo Calendar — automatically syncs upcoming planned service changes so you always stay informed.</p>
        <p>Every MTA service alert is converted into a calendar event with a clear title, description, and accurate start and end times. That means weekend reroutes, late-night suspensions, planned shutdowns for track work, and station closures show up directly alongside the rest of your schedule. No more checking the MTA website or getting blindsided at the platform.</p>
        <p>Subscriptions are available for every line in the New York City subway system, including the A, C, E, B, D, F, M, G, J, Z, L, N, Q, R, W, 1, 2, 3, 4, 5, 6, 7, S, and SIR lines. Calendar feeds update automatically — your calendar app polls for changes in the background, so you always have the latest information without any manual refreshing.</p>
        <h2>Supported calendars</h2>
        <p>NYC Train Cal works with any calendar application that supports iCalendar subscriptions, including Google Calendar, Apple Calendar (iOS and macOS), Microsoft Outlook, and Yahoo Calendar. Once subscribed, service alert events appear alongside your existing events and update automatically as the MTA publishes new alerts.</p>
    </div>

    <script>
        const trainLinks = document.querySelectorAll('.train-link[data-train]');
        const subscribeSection = document.getElementById('subscribeSection');
        const validTrains = new Set(['A','C','E','B','D','F','M','G','J','Z','L','N','Q','R','W','1','2','3','4','5','6','7','S','SI']);

        const trainPathMatch = window.location.pathname.match(/^\/trains\/([^/]+)\/?$/);
        const maybeTrainFromPath = trainPathMatch ? trainPathMatch[1].toUpperCase() : '';
        const selectedTrain = validTrains.has(maybeTrainFromPath) ? maybeTrainFromPath : null;

        function renderSelection(train) {
            const activeLink = Array.from(trainLinks).find(link => link.dataset.train === train);
            if (!activeLink) {
                subscribeSection.style.display = 'none';
                return;
            }

            const icsUrl = window.location.origin + '/api/calendars/train/' + train + '.ics';
            const webcalUrl = icsUrl.replace(/^https?:/, 'webcal:');
            const colorClass = activeLink.className.split(' ').find(c => c.startsWith('train-') && c !== 'train-link');
            const badge = `<span class="train-badge ${colorClass}">${train}</span>`;

            trainLinks.forEach(link => link.classList.remove('selected'));
            activeLink.classList.add('selected');

            subscribeSection.innerHTML = `
                <div class="subscribe-buttons">
                    <a class="subscribe-btn" href="https://calendar.google.com/calendar/r?cid=${encodeURIComponent(webcalUrl)}" target="_blank" rel="noopener">Add ${badge} to Google Calendar</a>
                    <a class="subscribe-btn" href="${webcalUrl}">Add ${badge} to Apple Calendar</a>
                    <a class="subscribe-btn" href="https://outlook.live.com/calendar/0/addfromweb?url=${encodeURIComponent(icsUrl)}" target="_blank" rel="noopener">Add ${badge} to Outlook</a>
                    <a class="subscribe-btn" href="https://calendar.yahoo.com/?v=60&type=16&SUBCAL=${encodeURIComponent(icsUrl)}" target="_blank" rel="noopener">Add ${badge} to Yahoo Calendar</a>
                </div>`;
            subscribeSection.style.display = 'block';
        }

        if (selectedTrain) {
            renderSelection(selectedTrain);
        }
    </script>
</body>
</html>"#;

    let html = html_template.replace("<!-- GTAG_PLACEHOLDER -->", gtag_snippet);

    (
        StatusCode::OK,
        [("Content-Type", "text/html; charset=utf-8")],
        html,
    )
}

fn is_valid_train(train_name: &str) -> bool {
    const VALID_TRAINS: &[&str] = &[
        "A", "C", "E", "B", "D", "F", "M", "G", "J", "Z", "L", "N", "Q", "R", "W", "1", "2", "3",
        "4", "5", "6", "7", "S", "SI",
    ];

    VALID_TRAINS.contains(&train_name)
}

fn required_gtag_id_from_env() -> Result<String, String> {
    let ga_id = std::env::var("GTAG_ID")
        .map_err(|_| "GTAG_ID environment variable is required".to_string())?;
    let ga_id = ga_id.trim();

    if ga_id.is_empty() {
        return Err("GTAG_ID cannot be empty".to_string());
    }

    if !ga_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("GTAG_ID contains invalid characters".to_string());
    }

    Ok(ga_id.to_string())
}

fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(env_filter)
        .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339())
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .init();
}

fn build_gtag_snippet(ga_id: &str) -> String {
    format!(
        r#"<script async src="https://www.googletagmanager.com/gtag/js?id={ga_id}"></script>
    <script>
      window.dataLayer = window.dataLayer || [];
      function gtag(){{dataLayer.push(arguments);}}
      gtag('js', new Date());
      gtag('config', '{ga_id}');
    </script>"#
    )
}

async fn handle_favicon() -> Response {
    let favicon = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="NYC Train Cal favicon">
    <rect x="0" y="0" width="64" height="64" rx="12" fill="#0039a6"/>
    <rect x="16" y="10" width="32" height="36" rx="10" fill="#ffffff"/>
    <rect x="22" y="18" width="20" height="10" rx="2" fill="#0039a6"/>
    <circle cx="24" cy="38" r="3" fill="#0039a6"/>
    <circle cx="40" cy="38" r="3" fill="#0039a6"/>
    <rect x="28" y="46" width="8" height="8" rx="2" fill="#ffffff"/>
    <rect x="14" y="56" width="36" height="4" rx="2" fill="#ff6319"/>
</svg>"##;

    (
        StatusCode::OK,
        [("Content-Type", "image/svg+xml; charset=utf-8")],
        favicon,
    )
        .into_response()
}
