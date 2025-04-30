mod login;
mod dropbox;
mod accounts;

#[macro_use] extern crate rocket;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use dropbox_sdk::default_async_client::UserAuthDefaultClient;
use log::{info, error};
use rocket::fs::FileServer;
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
    println!("Main function started");
    dotenv::dotenv().ok();
    let logger = biologischer_log::init_logger(env!("CARGO_CRATE_NAME"));
    info!("Logger initialized");

    // get important files from dropbox
    let dropbox_client: Arc<UserAuthDefaultClient> = Arc::new(initialize_dropbox().await);
    let accounts: Vec<AcornAccount> = match download_accounts(dropbox_client.clone()).await {
        Ok(accounts) => accounts,
        Err(err) => {
            error!("Failed to load accounts from DropBox: {err}");
            logger.shutdown();
            std::process::exit(1);
        },
    };

    info!("Dropbox accounts: {accounts:?}");    // debug feature
    let accounts: Arc<RwLock<Vec<AcornAccount>>> = Arc::new(RwLock::new(accounts));

    // This maps `temp_login_token`s to AcornGM account `discord_id`s
    let temp_login_tokens: Arc<RwLock<HashMap<String, String>>> = Arc::new(RwLock::new(HashMap::new()));

    // get other environment variables
    let discord_app_client_secret: String = match std::env::var("DISCORD_CLIENT_SECRET") {
        Ok(var) => var,
        Err(error) => {
            error!("Could not get environment variable for discord client secret: {error}");
            logger.shutdown();
            std::process::exit(1);
        },
    };

    let discord_handler = AccountHandler::new(dropbox_client.clone(), &discord_app_client_secret, accounts, temp_login_tokens);

    info!("Starting rocket");
    rocket::build()
        .configure(rocket::Config::figment().merge(("port", 24187)))
        .manage(discord_handler)
        .mount("/", routes![html_get_index, redirect_get_goto_discord_auth])
        .mount("/api", routes![api_get_discord_auth, api_post_register, api_post_temp_login, api_get_access_token])
        .mount("/", FileServer::from(SERVE_DIR_PATH.clone()))
}
