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
    development_mode: bool,
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
        development_mode: std::env::var("APP_ENV").is_ok_and(|value| value == "development"),
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
    render_index_page(&state.gtag_snippet, state.development_mode).into_response()
}

async fn handle_index_train(
    State(state): State<AppState>,
    Path(train_name): Path<String>,
) -> Response {
    let train = train_name.to_uppercase();
    if !is_valid_train(&train) {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    render_index_page(&state.gtag_snippet, state.development_mode).into_response()
}

fn render_index_page(
    gtag_snippet: &str,
    development_mode: bool,
) -> (StatusCode, [(&'static str, &'static str); 1], String) {
    let html_template = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <script>
        (() => {
            try {
                const saved = localStorage.getItem('theme');
                document.documentElement.dataset.theme = saved || (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
            } catch (_) {
                document.documentElement.dataset.theme = 'light';
            }
        })();
    </script>
    <title>NYC Train Cal — MTA Subway Service Alert Calendars</title>
    <meta name="description" content="Subscribe to live MTA subway service alerts as calendar feeds. Get planned service changes, weekend reroutes, and suspensions directly in Google Calendar, Apple Calendar, or Outlook — filtered by train line.">
    <!-- GTAG_PLACEHOLDER -->
    <link rel="icon" href="/favicon.svg" type="image/svg+xml">
    <link href='https://cdn.jsdelivr.net/npm/fullcalendar@6.1.17/index.global.min.css' rel='stylesheet' />
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
        .dev-banner {
            background: #fff3cd;
            border: 1px solid #ffda6a;
            border-radius: 8px;
            color: #664d03;
            font-weight: 600;
            padding: 10px 14px;
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
        #calendarContainer {
            margin-top: 24px;
            border: 1px solid #ddd;
            border-radius: 8px;
            padding: 16px;
            background: #fff;
        }
        #calendarContainer h3 {
            margin: 0 0 12px 0;
            font-size: 15px;
            color: #555;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }
        #eventDetail {
            margin-top: 16px;
            padding: 14px 16px;
            background: #fff;
            border: 1px solid #ddd;
            border-radius: 8px;
            display: none;
        }
        #eventDetail h4 {
            margin: 0 0 6px 0;
            font-size: 15px;
            color: #222;
        }
        #eventDetail .event-time {
            font-size: 13px;
            color: #666;
            margin-bottom: 8px;
        }
        #eventDetail .event-desc {
            font-size: 14px;
            color: #444;
            white-space: pre-wrap;
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

        :root {
            color-scheme: light;
            --bg: #f4f6fa;
            --surface: #ffffff;
            --surface-2: #f8fafc;
            --text: #141b2a;
            --muted: #667085;
            --border: #dfe4ec;
            --accent: #174ea6;
            --accent-hover: #0f3d85;
            --accent-soft: #e9f0ff;
            --shadow: 0 18px 50px rgba(29, 45, 74, 0.09);
            --focus: #6ea8fe;
        }
        :root[data-theme="dark"] {
            color-scheme: dark;
            --bg: #0b1018;
            --surface: #131a24;
            --surface-2: #192231;
            --text: #f4f7fb;
            --muted: #a3adbd;
            --border: #2b3647;
            --accent: #6ea8fe;
            --accent-hover: #91bdff;
            --accent-soft: #172a46;
            --shadow: 0 20px 60px rgba(0, 0, 0, 0.28);
        }
        * { box-sizing: border-box; }
        body {
            max-width: none;
            margin: 0;
            padding: 0;
            background: var(--bg);
            color: var(--text);
            transition: background .2s ease, color .2s ease;
        }
        .app-shell { width: min(1080px, calc(100% - 32px)); margin: 0 auto; padding: 24px 0 64px; }
        .site-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 22px; }
        .brand { display: flex; align-items: center; gap: 11px; color: var(--text); font-size: 18px; font-weight: 750; letter-spacing: -.02em; }
        .brand-mark { display: grid; place-items: center; width: 38px; height: 38px; border-radius: 12px; background: #0039a6; color: white; box-shadow: 0 8px 22px rgba(0,57,166,.24); }
        .header-actions { display: flex; align-items: center; gap: 10px; }
        .status-pill, .dev-banner { display: inline-flex; align-items: center; gap: 7px; border: 1px solid var(--border); border-radius: 999px; background: var(--surface); color: var(--muted); padding: 7px 11px; font-size: 12px; font-weight: 650; }
        .status-pill { height: 40px; }
        .status-dot { width: 7px; height: 7px; border-radius: 50%; background: #20a464; box-shadow: 0 0 0 4px rgba(32,164,100,.12); }
        .theme-toggle { display: grid; place-items: center; width: 40px; height: 40px; border: 1px solid var(--border); border-radius: 12px; background: var(--surface); color: var(--text); cursor: pointer; font-size: 18px; }
        .theme-toggle:hover { background: var(--surface-2); }
        .theme-icon-dark { display: none; }
        [data-theme="dark"] .theme-icon-light { display: none; }
        [data-theme="dark"] .theme-icon-dark { display: inline; }
        .hero { display: grid; grid-template-columns: 1.35fr .65fr; gap: 24px; align-items: center; overflow: hidden; position: relative; border: 1px solid var(--border); border-radius: 24px; padding: clamp(28px, 5vw, 54px); background: var(--surface); box-shadow: var(--shadow); }
        .eyebrow { margin: 0 0 12px; color: var(--accent); font-size: 13px; font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
        .hero h1 { max-width: 680px; margin: 0; color: var(--text); font-size: clamp(36px, 6vw, 64px); line-height: 1.02; letter-spacing: -.055em; }
        .hero-copy { max-width: 620px; margin: 20px 0 0; color: var(--muted); font-size: clamp(16px, 2vw, 19px); line-height: 1.65; }
        .hero-art { min-height: 190px; display: grid; place-items: center; position: relative; }
        .route-stack { position: relative; width: 190px; height: 175px; }
        .route-disc { position: absolute; display: grid; place-items: center; width: 82px; height: 82px; border-radius: 50%; color: white; font-size: 30px; font-weight: 850; border: 6px solid var(--surface); box-shadow: 0 14px 30px rgba(15,23,42,.2); }
        .route-disc:nth-child(1) { left: 5px; top: 14px; background: #0039a6; }
        .route-disc:nth-child(2) { right: 4px; top: 1px; background: #ff6319; }
        .route-disc:nth-child(3) { left: 54px; bottom: 0; background: #00933c; }
        .panel { margin-top: 24px; border: 1px solid var(--border); border-radius: 22px; background: var(--surface); box-shadow: var(--shadow); }
        .line-panel { padding: clamp(22px, 4vw, 36px); }
        .line-panel.compact { position: sticky; top: 12px; z-index: 20; grid-area: dock; margin: 0; padding: 18px; border-radius: 18px; view-transition-name: line-selector; }
        .line-panel.compact .section-heading { margin-bottom: 14px; }
        .line-panel.compact .section-heading h2 { font-size: 15px; }
        .line-panel.compact .section-heading p { display: none; }
        .line-panel.compact .train-grid { grid-template-columns: repeat(6, 1fr); gap: 9px; }
        .line-panel.compact .train-link { max-width: 38px; font-size: 12px; box-shadow: 0 4px 10px rgba(15,23,42,.12); }
        .section-heading { display: flex; justify-content: space-between; gap: 20px; align-items: end; margin-bottom: 24px; }
        .section-heading h2 { margin: 0 0 4px; color: var(--text); font-size: 25px; letter-spacing: -.03em; }
        .section-heading p { margin: 0; color: var(--muted); }
        .train-grid { display: grid; grid-template-columns: repeat(8, 1fr); gap: 16px; margin: 0; }
        .train-link { width: 100%; height: auto; max-width: 72px; aspect-ratio: 1; justify-self: center; font-size: 23px; box-shadow: 0 6px 15px rgba(15,23,42,.13); }
        .train-link:hover { transform: translateY(-3px) scale(1.03); }
        .train-link.selected { box-shadow: 0 0 0 4px var(--surface), 0 0 0 7px var(--accent), 0 8px 20px rgba(15,23,42,.18); }
        .train-link:focus-visible, .theme-toggle:focus-visible, .subscribe-btn:focus-visible, .copy-btn:focus-visible { outline: 3px solid var(--focus); outline-offset: 3px; }
        .workspace { display: none; grid-template-columns: minmax(0, 1.35fr) minmax(270px, .65fr); grid-template-areas: "calendar dock" "calendar subscribe"; align-items: start; gap: 24px; margin-top: 24px; }
        .workspace.visible { display: grid; }
        .subscribe-panel, #calendarContainer { margin: 0; padding: 26px; }
        .subscribe-panel { grid-area: subscribe; }
        .subscribe-kicker { color: var(--muted); font-size: 13px; font-weight: 750; letter-spacing: .08em; text-transform: uppercase; }
        .subscribe-title { margin: 8px 0 8px; color: var(--text); font-size: 25px; line-height: 1.25; }
        .subscribe-intro { margin: 0 0 20px; color: var(--muted); font-size: 14px; }
        .subscribe-buttons { display: grid; grid-template-columns: 1fr; gap: 10px; }
        .subscribe-btn, .copy-btn { min-height: 56px; border: 1px solid var(--border); border-radius: 13px; padding: 12px 14px; background: var(--surface-2); color: var(--text); }
        .subscribe-btn { justify-content: space-between; }
        .subscribe-btn:hover, .copy-btn:hover { border-color: var(--accent); background: var(--accent-soft); }
        .provider { display: flex; align-items: center; gap: 11px; }
        .provider-icon { display: grid; place-items: center; width: 31px; height: 31px; border-radius: 9px; background: var(--surface); border: 1px solid var(--border); font-size: 15px; }
        .provider-copy { display: flex; flex-direction: column; line-height: 1.25; }
        .provider-copy small { margin-top: 3px; color: var(--muted); font-weight: 500; }
        .copy-btn { width: 100%; margin-top: 12px; cursor: pointer; font: inherit; font-weight: 700; }
        #calendarContainer { grid-area: calendar; display: block; background: var(--surface); }
        #calendarContainer h3 { color: var(--text); font-size: 18px; text-transform: none; letter-spacing: -.02em; }
        #eventDetail { background: var(--surface-2); border-color: var(--border); }
        #eventDetail h4 { color: var(--text); }
        #eventDetail .event-time, #eventDetail .event-desc { color: var(--muted); }
        .fc { --fc-page-bg-color: var(--surface); --fc-neutral-bg-color: var(--surface-2); --fc-neutral-text-color: var(--muted); --fc-border-color: var(--border); --fc-button-bg-color: var(--accent); --fc-button-border-color: var(--accent); --fc-button-hover-bg-color: var(--accent-hover); --fc-button-hover-border-color: var(--accent-hover); --fc-button-active-bg-color: var(--accent-hover); --fc-today-bg-color: var(--accent-soft); color: var(--text); }
        .fc .fc-toolbar { flex-wrap: wrap; gap: 10px; }
        .fc .fc-toolbar-title { font-size: 18px; }
        .about { padding: clamp(24px, 4vw, 36px); background: var(--surface); border: 1px solid var(--border); border-radius: 22px; }
        .about h2 { margin-top: 0; color: var(--text); font-size: 25px; letter-spacing: -.03em; }
        .steps { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; }
        .step { padding: 20px; border: 1px solid var(--border); border-radius: 16px; background: var(--surface-2); }
        .step-number { color: var(--accent); font-weight: 850; }
        .step h3 { margin: 8px 0 5px; color: var(--text); }
        .step p { margin: 0; color: var(--muted); font-size: 14px; }
        .site-footer { display: flex; align-items: center; justify-content: center; gap: 12px; padding: 26px 0 0; color: var(--muted); text-align: center; font-size: 13px; }
        @media (max-width: 800px) { .hero { grid-template-columns: 1fr; } .hero-art { display: none; } .train-grid { grid-template-columns: repeat(6, 1fr); } .workspace { grid-template-columns: 1fr; grid-template-areas: "dock" "calendar" "subscribe"; } .line-panel.compact { top: 8px; } .line-panel.compact .train-grid { grid-template-columns: repeat(8, 1fr); } }
        @media (max-width: 600px) { .app-shell { width: min(100% - 22px, 1080px); padding-top: 12px; } .status-pill { display: none; } .hero { border-radius: 19px; padding: 28px 22px; } .hero h1 { font-size: 39px; } .panel { border-radius: 18px; } .train-grid { grid-template-columns: repeat(4, 1fr); gap: 13px; } .train-link { width: 100%; height: auto; font-size: 18px; } .line-panel.compact { padding: 12px; } .line-panel.compact .train-grid { grid-template-columns: repeat(6, 1fr); gap: 8px; } .steps { grid-template-columns: 1fr; } .fc .fc-toolbar { align-items: flex-start; } }
        ::view-transition-old(line-selector), ::view-transition-new(line-selector) { animation-duration: .38s; animation-timing-function: cubic-bezier(.22, 1, .36, 1); }
        @media (prefers-reduced-motion: reduce) { *, *::before, *::after { scroll-behavior: auto !important; transition-duration: .01ms !important; animation-duration: .01ms !important; } }
    </style>
</head>
<body>
<div class="app-shell">
    <header class="site-header">
        <div class="brand"><span class="brand-mark">N</span><span>NYC Train Cal</span></div>
        <div class="header-actions">
            <span class="status-pill"><span class="status-dot"></span>Live MTA data</span>
            <button class="theme-toggle" id="themeToggle" type="button" aria-label="Switch color theme"><span class="theme-icon-light">☾</span><span class="theme-icon-dark">☀</span></button>
        </div>
    </header>
    <main>
    <section class="hero">
        <div><p class="eyebrow">Service changes, simplified</p><h1>Subway alerts, right in your calendar.</h1><p class="hero-copy">Choose your line and subscribe once. Weekend reroutes, planned work, and service changes stay in sync automatically.</p></div>
        <div class="hero-art" aria-hidden="true"><div class="route-stack"><span class="route-disc">A</span><span class="route-disc">F</span><span class="route-disc">4</span></div></div>
    </section>
    <div id="linePanelHome"></div>
    <section class="panel line-panel" id="linePanel">
        <div class="section-heading"><div><h2>Choose your line</h2><p>Select a route to preview alerts and subscription options.</p></div></div>
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
    </section>

    <div class="workspace" id="workspace">
    <div class="panel subscribe-panel" id="subscribeSection"></div>
    <div class="panel" id="calendarContainer">
        <h3>Upcoming service alerts</h3>
        <div id="calendarEl"></div>
        <div id="eventDetail">
            <h4 id="eventDetailTitle"></h4>
            <div class="event-time" id="eventDetailTime"></div>
            <div class="event-desc" id="eventDetailDesc"></div>
        </div>
    </div>
    </div>

    <div class="about">
        <h2>How it works</h2>
        <div class="steps">
            <article class="step"><span class="step-number">01</span><h3>Choose a line</h3><p>Pick any subway route to see its planned service changes.</p></article>
            <article class="step"><span class="step-number">02</span><h3>Add it once</h3><p>Subscribe with Google, Apple, Outlook, Yahoo, or any iCalendar app.</p></article>
            <article class="step"><span class="step-number">03</span><h3>Stay in sync</h3><p>Your calendar refreshes automatically as the MTA publishes updates.</p></article>
        </div>
    </div>

    <script src='https://cdn.jsdelivr.net/npm/ical.js@1.5.0/build/ical.min.js'></script>
    <script src='https://cdn.jsdelivr.net/npm/fullcalendar@6.1.17/index.global.min.js'></script>
    <script src='https://cdn.jsdelivr.net/npm/@fullcalendar/icalendar@6.1.17/index.global.min.js'></script>
    <script>
        const trainLinks = document.querySelectorAll('.train-link[data-train]');
        const subscribeSection = document.getElementById('subscribeSection');
        const calendarContainer = document.getElementById('calendarContainer');
        const workspace = document.getElementById('workspace');
        const calendarEl = document.getElementById('calendarEl');
        const themeToggle = document.getElementById('themeToggle');
        const linePanel = document.getElementById('linePanel');
        const linePanelHome = document.getElementById('linePanelHome');
        const validTrains = new Set(['A','C','E','B','D','F','M','G','J','Z','L','N','Q','R','W','1','2','3','4','5','6','7','S','SI']);

        function selectedTrainFromPath() {
            const match = window.location.pathname.match(/^\/trains\/([^/]+)\/?$/);
            const train = match ? match[1].toUpperCase() : '';
            return validTrains.has(train) ? train : null;
        }

        let calendarInstance = null;

        function syncThemeButton() {
            const dark = document.documentElement.dataset.theme === 'dark';
            themeToggle.setAttribute('aria-pressed', String(dark));
            themeToggle.setAttribute('aria-label', dark ? 'Switch to light mode' : 'Switch to dark mode');
        }
        syncThemeButton();
        themeToggle.addEventListener('click', () => {
            const theme = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
            document.documentElement.dataset.theme = theme;
            try { localStorage.setItem('theme', theme); } catch (_) {}
            syncThemeButton();
            if (calendarInstance) calendarInstance.updateSize();
        });

        function renderSelection(train) {
            const activeLink = Array.from(trainLinks).find(link => link.dataset.train === train);
            if (!activeLink) {
                workspace.classList.remove('visible');
                trainLinks.forEach(link => { link.classList.remove('selected'); link.removeAttribute('aria-current'); });
                linePanel.classList.remove('compact');
                linePanelHome.after(linePanel);
                if (calendarInstance) {
                    calendarInstance.destroy();
                    calendarInstance = null;
                }
                return;
            }

            const icsUrl = window.location.origin + '/api/calendars/train/' + train + '.ics';
            const webcalUrl = icsUrl.replace(/^https?:/, 'webcal:');
            const colorClass = activeLink.className.split(' ').find(c => c.startsWith('train-') && c !== 'train-link');
            trainLinks.forEach(link => { link.classList.remove('selected'); link.removeAttribute('aria-current'); });
            activeLink.classList.add('selected');
            activeLink.setAttribute('aria-current', 'page');
            linePanel.classList.add('compact');
            workspace.appendChild(linePanel);

            subscribeSection.innerHTML = `
                <div class="subscribe-kicker">Selected line</div>
                <h2 class="subscribe-title">Add the ${train} train to your calendar</h2>
                <p class="subscribe-intro">Choose your calendar app. Alerts update automatically after you subscribe.</p>
                <div class="subscribe-buttons">
                    <a class="subscribe-btn" href="https://calendar.google.com/calendar/r?cid=${encodeURIComponent(webcalUrl)}" target="_blank" rel="noopener"><span class="provider"><span class="provider-icon">G</span><span class="provider-copy">Google Calendar<small>Open in a new tab</small></span></span><span>→</span></a>
                    <a class="subscribe-btn" href="${webcalUrl}"><span class="provider"><span class="provider-icon">●</span><span class="provider-copy">Apple Calendar<small>Open calendar app</small></span></span><span>→</span></a>
                    <a class="subscribe-btn" href="https://outlook.live.com/calendar/0/addfromweb?url=${encodeURIComponent(icsUrl)}" target="_blank" rel="noopener"><span class="provider"><span class="provider-icon">O</span><span class="provider-copy">Outlook<small>Open in a new tab</small></span></span><span>→</span></a>
                    <a class="subscribe-btn" href="https://calendar.yahoo.com/?v=60&type=16&SUBCAL=${encodeURIComponent(icsUrl)}" target="_blank" rel="noopener"><span class="provider"><span class="provider-icon">Y!</span><span class="provider-copy">Yahoo Calendar<small>Open in a new tab</small></span></span><span>→</span></a>
                </div>
                <button class="copy-btn" id="copyCalendarUrl" type="button">Copy calendar link</button>`;
            workspace.classList.add('visible');
            document.getElementById('copyCalendarUrl').addEventListener('click', async event => {
                await navigator.clipboard.writeText(icsUrl);
                event.currentTarget.textContent = 'Copied calendar link ✓';
            });
            if (calendarInstance) {
                calendarInstance.destroy();
            }
            const eventDetail = document.getElementById('eventDetail');
            const eventDetailTitle = document.getElementById('eventDetailTitle');
            const eventDetailTime = document.getElementById('eventDetailTime');
            const eventDetailDesc = document.getElementById('eventDetailDesc');

            calendarInstance = new FullCalendar.Calendar(calendarEl, {
                initialView: 'dayGridMonth',
                events: { url: icsUrl, format: 'ics' },
                height: 'auto',
                noEventsContent: 'No upcoming service alerts for this line.',
                headerToolbar: {
                    left: 'prev,next',
                    center: 'title',
                    right: 'today'
                },
                eventClick(info) {
                    info.jsEvent.preventDefault();
                    const start = info.event.start;
                    const end = info.event.end;
                    const fmt = d => d ? d.toLocaleString('en-US', { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }) : '';
                    const timeStr = end ? `${fmt(start)} – ${fmt(end)}` : fmt(start);
                    eventDetailTitle.textContent = info.event.title;
                    eventDetailTime.textContent = timeStr;
                    eventDetailDesc.textContent = info.event.extendedProps.description || '';
                    eventDetail.style.display = 'block';
                    eventDetail.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
                }
            });
            eventDetail.style.display = 'none';
            calendarInstance.render();
        }

        function handleTrainNavigation(event, shouldScroll) {
            if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
            event.preventDefault();
            const link = event.currentTarget;
            const scrollAfterSelection = shouldScroll && !linePanel.classList.contains('compact');
            history.pushState({}, '', link.href);
            const updateSelection = () => renderSelection(link.dataset.train);
            const transition = document.startViewTransition ? document.startViewTransition(updateSelection) : null;
            if (!transition) updateSelection();
            if (scrollAfterSelection) {
                const scrollToWorkspace = () => workspace.scrollIntoView({
                    behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
                    block: 'start'
                });
                transition ? transition.finished.then(scrollToWorkspace) : requestAnimationFrame(scrollToWorkspace);
            }
        }

        trainLinks.forEach(link => link.addEventListener('click', event => handleTrainNavigation(event, true)));

        window.addEventListener('popstate', () => renderSelection(selectedTrainFromPath()));
        renderSelection(selectedTrainFromPath());
    </script>
    </main>
    <footer class="site-footer"><span>Built for New Yorkers who would rather know before they reach the platform.</span><!-- DEV_MODE_PLACEHOLDER --></footer>
</div>
</body>
</html>"#;

    let dev_mode_badge = if development_mode {
        r#"<span class="dev-banner">Dev mode</span>"#
    } else {
        ""
    };
    let html = html_template
        .replace("<!-- GTAG_PLACEHOLDER -->", gtag_snippet)
        .replace("<!-- DEV_MODE_PLACEHOLDER -->", dev_mode_badge);

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
