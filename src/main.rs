mod not_found_html;

use axum::{
    Router,
    routing::get,
    handler::Handler,
    response::Html,
    http::StatusCode
};
use tower_http::services::{ServeDir, ServeFile};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use axum::http::Uri;
use axum::response::IntoResponse;
use axum::routing::get_service;
use chrono::FixedOffset;
use log::{info, warn, error};
use once_cell::unsync::Lazy;
use tokio::sync::RwLock;
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
    flexi_logger::Logger::try_with_str("info")
        .expect("Could not set up logger!")
        .format(move |w, now, log_record| {
            let gmt_plus2: FixedOffset = FixedOffset::east_opt(2 * 3600).expect("Could not generate (static) timezone in main");
            let now = now.now_utc_owned().with_timezone(&gmt_plus2);
            write!(
                w,
                "{} [{}] - {}",
                now.format("%Y-%m-%d %H:%M:%S").to_string(),
                log_record.level(),
                log_record.args()
            )
        })
        .start()
        .expect("Could not start logger!");


    // set up http server
    let serve_dir_path: PathBuf = PathBuf::from("./frontend/");  // "root" dir for url paths

    let app: Router = Router::new()
        .route("/", get_service(ServeFile::new(serve_dir_path.join("index.html"))))
        .fallback(|uri: Uri| not_found(uri.to_string()))
    ;

    let listener = tokio::net::TcpListener::bind(format!("{BIND_IP}:{BIND_PORT}"))
        .await.expect(&format!("Could not bind to \"{BIND_IP}:{BIND_PORT}\""));

    info!("Server running at http://{BIND_IP}:{BIND_PORT}/");

    axum::serve(listener, app)
        .await.expect("Could not serve the http server!");
}

