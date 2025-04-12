mod login;
mod dropbox;
mod accounts;

#[macro_use] extern crate rocket;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use chrono::FixedOffset;
use dropbox_sdk::default_async_client::UserAuthDefaultClient;
use log::{info, warn, error, debug};
use colored::{Color, Colorize};
use rocket::fs::FileServer;
use rocket::futures::lock::Mutex;
use crate::accounts::{download_accounts, AcornAccount};
use crate::dropbox::initialize_dropbox;
use crate::login::{api_get_access_token, api_get_discord_auth, api_post_register, api_post_temp_login, redirect_get_goto_discord_auth, AccountHandler};
use rocket::response::Redirect;
use tokio::sync::RwLock;

#[get("/")]
fn html_get_index() -> Redirect {
    Redirect::to("index.html")
}

static SERVE_DIR_PATH: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| PathBuf::from("./frontend/"));

#[launch]
async fn rocket() -> _ {
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
                log::Level::Info => Color::Cyan,
                log::Level::Debug => Color::Cyan,
                log::Level::Trace => Color::Magenta,
            };
            write!(
                w,
                "{} [{:<5}] {}",
                now.format("%Y-%m-%d %H:%M:%S.%3f").to_string().color(Color::White),
                log_record.level().to_string().color(level_color),
                log_record.args().to_string().color(level_color),
            )
        })
        .start()
        .expect("Could not start logger!");


    // get important files from dropbox
    let dropbox_client: Arc<UserAuthDefaultClient> = Arc::new(initialize_dropbox().await);
    let accounts: Vec<AcornAccount> = match download_accounts(dropbox_client.clone()).await {
        Ok(accounts) => accounts,
        Err(err) => {
            error!("Failed to load accounts from DropBox: {err}");
            std::process::exit(1);
        },
    };

    info!("Accounts: {accounts:?}");
    let accounts: Arc<RwLock<Vec<AcornAccount>>> = Arc::new(RwLock::new(accounts));

    /// This maps `temp_login_token`s to AcornGM account `discord_id`s
    let temp_login_tokens: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));

    // get other environment variables
    let discord_app_client_secret: String = match std::env::var("DISCORD_CLIENT_SECRET") {
        Ok(var) => var,
        Err(error) => {
            error!("Could not get environment variable for discord client secret: {error}");
            std::process::exit(1);
        },
    };

    let discord_handler = AccountHandler::new(dropbox_client.clone(), &discord_app_client_secret, accounts, temp_login_tokens);

    rocket::build()
        .manage(discord_handler)
        .mount("/", routes![html_get_index, redirect_get_goto_discord_auth])
        .mount("/api", routes![api_get_discord_auth, api_post_register, api_post_temp_login, api_get_access_token])
        .mount("/", FileServer::from(SERVE_DIR_PATH.clone()))
}

