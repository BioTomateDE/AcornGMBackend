mod not_found_html;
mod login;
mod dropbox;
mod accounts;

use axum::{Router, routing::get, response::Html, Extension};
use tower_http::services::ServeFile;
use std::path::PathBuf;
use std::sync::Arc;
use axum::http::Uri;
use axum::response::IntoResponse;
use axum::routing::get_service;
use chrono::FixedOffset;
use dropbox_sdk::default_async_client::UserAuthDefaultClient;
use log::{info, warn, error, debug};
use colored::{Color, Colorize};
use crate::accounts::{download_accounts, AcornAccount};
use crate::dropbox::initialize_dropbox;
use crate::login::handle_get_discord_auth;
use crate::not_found_html::NOT_FOUND_HTML;



async fn not_found(uri: String) -> impl IntoResponse {
    Html(NOT_FOUND_HTML.replace("{url_path}", &uri))
}

const BIND_IP: &'static str = "0.0.0.0";
const BIND_PORT: u16 = 8080;


#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    // set up logging
    flexi_logger::Logger::try_with_str("info, main=trace")
        .expect("Could not set up logger!")
        .format(move |w, now, log_record| {
            let gmt_plus2: FixedOffset = FixedOffset::east_opt(2 * 3600).expect("Could not generate (static) timezone in main");
            let now = now.now_utc_owned().with_timezone(&gmt_plus2);
            let level_color = match log_record.level() {
                log::Level::Error => Color::Red,
                log::Level::Warn => Color::Yellow,
                log::Level::Info => Color::Green,
                log::Level::Debug => Color::Cyan,
                log::Level::Trace => Color::Magenta,
            };
            write!(
                w,
                "{} [{:<5}] {}",
                now.format("%Y-%m-%d %H:%M:%S.%3f").to_string(),
                log_record.level().to_string().color(level_color),
                log_record.args().to_string().color(level_color),
            )
        })
        .start()
        .expect("Could not start logger!");


    // get important files from dropbox
    let dropbox_client: UserAuthDefaultClient = initialize_dropbox().await;
    let accounts: Vec<AcornAccount> = match download_accounts(dropbox_client).await {
        Ok(accounts) => accounts,
        Err(err) => {
            error!("Failed to load accounts from DropBox: {err}");
            std::process::exit(1);
        },
    };

    info!("Accounts: {accounts:?}");

    let accounts: Arc<Vec<AcornAccount>> = Arc::new(accounts);

    // get other environment variables
    let discord_app_client_secret: String = match std::env::var("DISCORD_CLIENT_SECRET") {
        Ok(var) => var,
        Err(error) => {
            error!("Could not get environment variable for discord client secret: {error}");
            std::process::exit(1);
        },
    };


    // set up http server
    let serve_dir_path: PathBuf = PathBuf::from("./frontend/");  // "root" dir for url paths

    let app: Router = Router::new()
        .route("/", get_service(ServeFile::new(serve_dir_path.join("index.html"))))
        .route("/styles.css", get_service(ServeFile::new(serve_dir_path.join("styles.css"))))
        .route("/discord_auth_redirected", get_service(ServeFile::new(serve_dir_path.join("discord_auth_redirected.html"))))
        .route("/api/discord_auth", get(|query| async { handle_get_discord_auth(&discord_app_client_secret, &accounts, query).await }))
        .layer(Extension(discord_app_client_secret))
        .layer(Extension(accounts))
        .fallback(|uri: Uri| not_found(uri.to_string()))
    ;

    let listener = tokio::net::TcpListener::bind(format!("{BIND_IP}:{BIND_PORT}"))
        .await.expect(&format!("Could not bind to \"{BIND_IP}:{BIND_PORT}\""));

    info!("Server running at http://{BIND_IP}:{BIND_PORT}/");

    axum::serve(listener, app)
        .await.expect("Could not serve the http server!");
}

