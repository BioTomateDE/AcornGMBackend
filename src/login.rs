use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use serde::Deserialize;
use reqwest::Client;
use rocket::http::Status;
use crate::accounts::AcornAccount;
use rocket::response::status;
use rocket::serde::json::Json;
use serde_json::{json, Value};
use rocket::form::FromForm;
use rocket::State;
use regex::Regex;
use rocket::futures::lock::Mutex;
use tokio::sync::RwLock;

#[derive(Debug, Clone, FromForm)]
struct DiscordAuthQuery {
    discord_code: String,
}


#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    refresh_token: String,
    scope: String,
}

#[derive(Debug, Deserialize)]
struct DiscordUserInfo {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub verified: bool,
}


type RespType = status::Custom<Json<Value>>;
fn respond_err(status: Status, error_message: &str) -> RespType {
    status::Custom(
        status,
        Json(json!({
            "error": error_message,
        })),
    )
}
fn respond_ok(json: Value) -> RespType {
    status::Custom(
        Status::Ok,
        Json(json!(json)),
    )
}


const DISCORD_API_BASE_URL: &'static str = "https://discord.com/api/v10";
const REDIRECT_URI: &'static str = "https://acorngm.onrender.com/discord_auth_redirected.html";     // irrelevant i think
const DISCORD_APP_CLIENT_ID: &'static str = "1360325253766578479";

async fn get_access_token(discord_app_client_secret: &str, params: HashMap<&str, &str>) -> Result<TokenResponse, String> {
    let client: Client = Client::new();
    let res = client
        .post(format!("{}/oauth2/token", DISCORD_API_BASE_URL))
        .basic_auth(DISCORD_APP_CLIENT_ID, Some(discord_app_client_secret))
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    let status = res.status();
    if !status.is_success() {
        error!("Error while getting access token from discord - {}: {}", status, res.text().await.unwrap_or_else(|_| "<invalid response text>".to_string()));
        return Err(format!("Could not refresh discord token because discord returned a failure response {}", status));
    }

    res.json::<TokenResponse>().await.map_err(|error| format!("Failed to parse JSON while refreshing discord token: {error}"))
}

async fn exchange_code(discord_app_client_secret: &str, code: &str) -> Result<TokenResponse, String> {
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", &code);
    params.insert("redirect_uri", REDIRECT_URI);
    get_access_token(discord_app_client_secret, params).await
}

async fn refresh_token(discord_app_client_secret: &str, refresh_token: &str) -> Result<TokenResponse, String> {
    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("refresh_token", refresh_token);
    get_access_token(discord_app_client_secret, params).await
}


async fn get_user_info(access_token: &str) -> Result<DiscordUserInfo, String> {
    let client = Client::new();
    let res = client
        .get(format!("{}/users/@me", DISCORD_API_BASE_URL))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not request discord user info: {e}"))?;

    let status = res.status();

    if !status.is_success() {
        error!("Error while getting discord user info: {} - {}", status, res.text().await.unwrap_or_else(|_| "<invalid response text>".to_string()));
        return Err(format!("Could not get discord user info because discord returned a failure response {status}"));
    }

    res.json::<DiscordUserInfo>().await.map_err(|e| format!("Failed to parse JSON from discord user info response: {e}"))
}


pub struct DiscordHandler {
    discord_app_client_secret: String,
    accounts: Arc<RwLock<Vec<AcornAccount>>>,
}
impl DiscordHandler {
    pub fn new(discord_app_client_secret: &str, accounts: Arc<RwLock<Vec<AcornAccount>>>) -> Self {
        Self {
            discord_app_client_secret: discord_app_client_secret.to_string(),
            accounts,
        }
    }
    pub async fn handle_get_discord_auth(&self, code: &str) -> status::Custom<Json<Value>> {
        // Get access/refresh tokens from OAuth2 code
        let token_response: TokenResponse = match exchange_code(&self.discord_app_client_secret, code).await {
            Ok(token_response) => token_response,
            Err(error) => return status::Custom(Status::InternalServerError, Json(json!({
                "error": format!("Error while getting discord access token: {error}"),
            })))
        };

        // Get Discord User ID
        let user_info: DiscordUserInfo = match get_user_info(&token_response.access_token).await {
            Ok(user_info) => user_info,
            Err(error) => return respond_err(Status::InternalServerError, &format!("Error while getting discord user info: {error}")),
        };

        // check if account already exists; if it does, return acorn access token
        for account in self.accounts.clone().read().await.iter() {
            if account.discord_id != user_info.id { continue }
            return respond_ok(json!({
                "register": true,
            }));
        }

        // account does not exist; let client register
        respond_ok(json!({
            "register": false,
            "discordAccessToken": token_response.access_token,
            "discordUserId": user_info.id,
        }))
    }

    pub async fn handle_post_register(&self, register_data: Json<RegisterRequest>) -> status::Custom<Json<Value>> {
        static USERNAME_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9._-]+$").expect("Could not load username verification pattern"));

        if !USERNAME_REGEX.is_match(&register_data.username) {
            return respond_err(Status::BadRequest, &("Invalid username! Username must contain only latin letters, \
            digits, dots, underscores, and hyphens; without spaces."))
        }

        // validate refresh token and discord user id
        let token_response: TokenResponse = match refresh_token(&self.discord_app_client_secret, &register_data.discord_refresh_token).await {
            Ok(token) => token,
            Err(e) => return respond_err(Status::Unauthorized, &e),
        };

        let user_info: DiscordUserInfo = match get_user_info(&token_response.access_token).await {
            Ok(token) => token,
            Err(e) => return respond_err(Status::Unauthorized, &e),
        };

        if user_info.id != register_data.discord_user_id {
            return respond_err(Status::Unauthorized, "The provided discord user ID does not belong to the provided discord access token!");
        }
        if !user_info.verified {
            return respond_err(Status::Forbidden, "Please verify your discord account!");
        }

        // check if there is already an AcornGM account connected to this discord user
        for account in self.accounts.clone().read().await.iter() {
            if account.discord_id == register_data.discord_user_id || account.discord_refresh_token == register_data.discord_refresh_token {
                return respond_err(Status::Conflict, "There is already an AcornGM account connected to this discord account!");
            }
        }

        // TODO more checks maybe

        // success; add to account list
        let account = AcornAccount {
            name: register_data.username.clone(),
            date_created: chrono::Utc::now(),
            discord_id: register_data.discord_user_id.clone(),
            discord_refresh_token: register_data.discord_refresh_token.clone(),
            access_tokens: HashMap::new(),
        };

        let accounts_arc = self.accounts.clone();
        let mut accounts = accounts_arc.write().await;
        accounts.push(account);

        respond_ok(json!({}))
    }
}
#[get("/discord_auth?<code>")]
pub async fn handle_get_discord_auth(handler: &State<DiscordHandler>, code: &str) -> status::Custom<Json<Value>> {
    handler.handle_get_discord_auth(code).await
}
#[post("/register", data = "<register_data>")]
pub async fn register(handler: &State<DiscordHandler>, register_data: Json<RegisterRequest>) -> status::Custom<Json<Value>> {
    handler.handle_post_register(register_data).await
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct RegisterRequest {
    username: String,
    discord_user_id: String,
    discord_refresh_token: String,
}
