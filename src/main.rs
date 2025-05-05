mod login;
mod accounts;

#[macro_use] extern crate rocket;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use log::{info, error};
use rocket::fs::FileServer;
use crate::accounts::AcornAccount;
use crate::login::{api_get_access_token, api_get_discord_auth, api_post_register, api_post_temp_login, redirect_get_goto_discord_auth, AccountHandler};
use rocket::response::Redirect;
use sqlx::Pool;
use sqlx::postgres::PgPoolOptions;
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

    let pool = match PgPoolOptions::new()
        .max_connections(5)
        .connect(env!("DATABASE_URL"))
        .await {
        Ok(p) => p,
        Err(e) => {
            error!("Could not initialize database: {e}");
            logger.shutdown();
            std::process::exit(1);
        }
    };

    let discord_handler = AccountHandler::new(pool);

    info!("Starting rocket");
    rocket::build()
        .configure(rocket::Config::figment().merge(("port", 24187)))
        .manage(discord_handler)
        .mount("/", routes![html_get_index, redirect_get_goto_discord_auth])
        .mount("/api", routes![api_get_discord_auth, api_post_register, api_post_temp_login, api_get_access_token])
        .mount("/", FileServer::from(SERVE_DIR_PATH.clone()))
}
