mod accounts;
mod login;
mod mods;

#[macro_use]
extern crate rocket;

use once_cell::sync::OnceCell;
use crate::login::{
    api_get_access_token, api_get_discord_auth, api_post_register,
    api_post_temp_login, redirect_get_goto_discord_auth,
};
use log::{error, info};
use rocket::fs::FileServer;
use rocket::response::Redirect;
use sqlx::Postgres;
use sqlx::postgres::PgPoolOptions;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use biologischer_log::CustomLogger;

#[get("/")]
fn html_get_index() -> Redirect {
    Redirect::to("index.html")
}

static SERVE_DIR_PATH: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from("./frontend/"));
static LOGGER: LazyLock<Arc<CustomLogger>> = LazyLock::new(|| biologischer_log::init_logger(env!("CARGO_CRATE_NAME")));
static POOL: OnceCell<sqlx::Pool<Postgres>> = OnceCell::new();

#[launch]
async fn rocket() -> _ {
    println!("Main function started");
    dotenv::dotenv().ok();
    let _ = *LOGGER;
    info!("Logger initialized");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(env!("DATABASE_URL"))
        .await
        .unwrap_or_else(|e| {
            error!("Could not initialize database: {e}");
            LOGGER.shutdown();
            std::process::exit(1);
        });
    POOL.set(pool).expect("Could not set database pool OnceCell");

    info!("Starting rocket");
    rocket::build()
        .configure(rocket::Config::figment().merge(("port", 24187)))
        .mount("/", routes![html_get_index, redirect_get_goto_discord_auth])
        .mount(
            "/api",
            routes![
                api_get_discord_auth,
                api_post_register,
                api_post_temp_login,
                api_get_access_token
            ],
        )
        .mount("/", FileServer::from(SERVE_DIR_PATH.clone()))
}
