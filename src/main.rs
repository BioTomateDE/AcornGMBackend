mod accounts;
mod login;
mod mods;
mod search_mods;
mod sanitize;

#[macro_use]
extern crate rocket;

use once_cell::sync::OnceCell;
use crate::login::api_get_access_token;
use crate::login::api_get_discord_auth;
use crate::login::api_post_register;
use crate::login::api_post_temp_login;
use crate::login::redirect_get_goto_discord_auth;
use log::{error, info};
use rocket::fs::FileServer;
use rocket::response::{status, Redirect};
use sqlx::{Pool, Postgres};
use sqlx::postgres::PgPoolOptions;
use std::path::PathBuf;
use std::sync::LazyLock;
use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::{json, Value};
use crate::mods::{api_delete_mod, api_update_mod, api_upload_mod};

#[get("/")]
fn html_get_index() -> Redirect {
    Redirect::to("index.html")
}
#[get("/eula")]
fn html_get_eula() -> Redirect {
    Redirect::to("eula.html")
}

type RespType = Result<Option<Json<Value>>, status::Custom<Json<Value>>>;
fn respond_err(status: Status, error_message: &str) -> status::Custom<Json<Value>> {
    status::Custom(
        status,
        Json(json!({
            "error": error_message,
        }))
    )
}
fn respond_ok_value(json_response: Value) -> RespType {
    Ok(Some(Json(json!(json_response))))
}
fn respond_ok_empty() -> RespType {
    Ok(None)
}

fn pool<'a>() -> &'a Pool<Postgres> {
    POOL.get().expect("Database pool not initialized")
}

static SERVE_DIR_PATH: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from("./frontend/"));
static POOL: OnceCell<Pool<Postgres>> = OnceCell::new();

#[launch]
async fn rocket() -> _ {
    println!("Main function started");
    dotenvy::dotenv().ok();
    biologischer_log::init(env!("CARGO_CRATE_NAME"));
    info!("Logger initialized");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(env!("DATABASE_URL"))
        .await
        .unwrap_or_else(|e| {
            error!("Could not initialize database: {e}");
            std::process::exit(1);
        });
    POOL.set(pool).expect("Could not set database pool OnceCell");

    info!("Starting rocket");
    rocket::build()
        .configure(rocket::Config::figment().merge(("port", 24187)))
        .mount("/", routes![html_get_index, html_get_eula, redirect_get_goto_discord_auth])
        .mount(
            "/api",
            routes![
                api_get_discord_auth,
                api_post_register,
                api_post_temp_login,
                api_get_access_token,
                api_upload_mod,
                api_update_mod,
                api_delete_mod,
            ],
        )
        .mount("/", FileServer::from(SERVE_DIR_PATH.clone()))
}
