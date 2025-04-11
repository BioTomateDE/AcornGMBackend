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
use axum::routing::get_service;
use log::{info, warn, error};
use once_cell::unsync::Lazy;
use tokio::sync::RwLock;

// static NOT_FOUND_HTML: Lazy<Arc<RwLock<String>>> = Lazy::new(|| {
//     // Load the `not_found.html` file when the server starts
//     // THIS USES THE frontend dir MANUALLY!
//     let content: String = std::fs::read_to_string("./frontend/not_found.html")
//         .unwrap_or_else(|_| "<h1>404 Not Found</h1>".to_string());
//
//     Arc::new(RwLock::new(content))
// });


async fn not_found(not_found_html: &str, uri: String) -> Html<String> {
    Html(not_found_html.replace("{url_path}", &uri))
}

#[tokio::main]
async fn main() {
    const BIND_IP: &'static str = "0.0.0.0";
    const BIND_PORT: u16 = 3000;

    // The "root" directory for frontend assets
    let serve_dir_path: PathBuf = PathBuf::from("./frontend/");
    // let serve_dir: ServeDir = ServeDir::new(serve_dir_path.clone());

    let not_found_path: PathBuf = serve_dir_path.join("not_found.html");
    let not_found_html: Arc<String> = Arc::new(std::fs::read_to_string(&not_found_path).unwrap_or_else(|error| {
        error!("Could not read not_found html file (ironic) ({}): {}", not_found_path.display(), error);
        "<h1>Not Found</h1><p>Could not find URL Path <strong>{url_path}</strong></p>".to_string()
    }));

    // Build the app with static file serving
    let app: Router = Router::new()
        .route("/", get_service(ServeFile::new(serve_dir_path.join("index.html"))))
        .fallback(|uri: Uri| not_found(&not_found_html, uri.to_string()))
        // .fallback(|uri| async { not_found(uri.to_string()).await })
        // .nest("/static", serve_dir)
    ;

    // Set the address and port for the server
    let listener = tokio::net::TcpListener::bind(format!("{BIND_IP}:{BIND_PORT}"))
        .await.expect(&format!("Could not bind to \"{BIND_IP}:{BIND_PORT}\"!"));

    info!("Server running at http://{BIND_IP}:{BIND_PORT}/");

    axum::serve(listener, app)
        .await.expect("Could not serve the http server!");
}


