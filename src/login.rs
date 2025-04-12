use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use serde::Deserialize;
use reqwest::{Client, StatusCode};
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
    pub avatar: String,
    pub discriminator: String,
    pub public_flags: u64,
    pub flags: u64,
    pub banner: Option<String>,
    pub accent_color: u32,
    pub global_name: String,
    pub avatar_decoration_data: Option<String>,
    pub collectibles: Option<String>,
    pub banner_color: String,
    pub clan: Option<String>,
    pub primary_guild: Option<String>,
    pub mfa_enabled: bool,
    pub locale: String,
    pub premium_type: u32,
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
fn respond_ok(json_response: Value) -> RespType {
    status::Custom(
        Status::Ok,
        Json(json!(json_response)),
    )
}


const DISCORD_API_BASE_URL: &'static str = "https://discord.com/api/v10";
const REDIRECT_URI: &'static str = "https://acorngm.onrender.com/discord_auth_page.html";
const DISCORD_APP_CLIENT_ID: &'static str = "1360325253766578479";

async fn get_access_token(discord_app_client_secret: &str, params: HashMap<&str, &str>) -> Result<TokenResponse, (Status, String)> {
    let client: Client = Client::new();
    let res = client
        .post(format!("{}/oauth2/token", DISCORD_API_BASE_URL))
        .basic_auth(DISCORD_APP_CLIENT_ID, Some(discord_app_client_secret))
        .form(&params)
        .send()
        .await
        .map_err(|e| (Status::InternalServerError, format!("Request failed: {e}")))?;

    let status = res.status();
    if !status.is_success() {
        // check if code is invalid; because if it is, the error is the client's fault
        let text: String = res.text().await.unwrap_or_else(|_| "<invalid response text>".to_string());
        if text.contains("Invalid \\\"code\\\" in request") {
            return Err((Status::Unauthorized, "Could not refresh discord token because the provided code is invalid".to_string()));
        }
        error!("Error while getting access token from discord - {}: {}", status, text);
        return Err((Status::InternalServerError, format!("Could not refresh discord token because discord returned a failure response: {}", status)));
    }

    res.json::<TokenResponse>().await.map_err(|error| (Status::InternalServerError, format!("Failed to parse JSON while refreshing discord token: {error}")))
}

async fn exchange_code(discord_app_client_secret: &str, code: &str) -> Result<TokenResponse, (Status, String)> {
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", &code);
    params.insert("redirect_uri", REDIRECT_URI);
    get_access_token(discord_app_client_secret, params).await
}

async fn refresh_token(discord_app_client_secret: &str, refresh_token: &str) -> Result<TokenResponse, (Status, String)> {
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
        return Err(format!("Could not get discord user info because discord returned a failure response: {status}"));
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

    pub async fn api_get_discord_auth(&self, code: &str) -> status::Custom<Json<Value>> {
        // Get access/refresh tokens from OAuth2 code
        let token_response: TokenResponse = match exchange_code(&self.discord_app_client_secret, code).await {
            Ok(token_response) => token_response,
            Err((status, error)) => return status::Custom(status, Json(json!({
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
            if account.discord_id == user_info.id {
                return respond_ok(json!({
                    "register": false,
                }));
            }
        }

        // account does not exist; let client register
        respond_ok(json!({
            "register": true,
            "discordRefreshToken": token_response.refresh_token,
            "discordUserId": user_info.id,
            "discordUsername": user_info.username,
        }))
    }

    pub async fn api_post_register(&self, register_data: Json<RegisterRequest>) -> status::Custom<Json<Value>> {
        static USERNAME_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9._-]+$").expect("Could not load username verification pattern"));

        if !USERNAME_REGEX.is_match(&register_data.username) {
            return respond_err(Status::BadRequest, &("Invalid username! Username must contain only latin letters, \
            digits, dots, underscores, and hyphens; without spaces."))
        }

        // validate refresh token and discord user id
        let token_response: TokenResponse = match refresh_token(&self.discord_app_client_secret, &register_data.discord_refresh_token).await {
            Ok(token) => token,
            Err((status, e)) => return respond_err(status, &e),
        };

        let user_info: DiscordUserInfo = match get_user_info(&token_response.access_token).await {
            Ok(token) => token,
            Err(e) => return respond_err(Status::Unauthorized, &e),
        };

        if user_info.id != register_data.discord_user_id {
            return respond_err(Status::Unauthorized, "The provided discord user ID does not belong to the provided discord access token!");
        }

        // check if there is already an AcornGM account connected to this discord user
        for account in self.accounts.clone().read().await.iter() {
            if account.discord_id == register_data.discord_user_id || account.discord_refresh_token == register_data.discord_refresh_token {
                return respond_err(Status::Conflict, "There is already an AcornGM account connected to this discord account!");
            }
        }

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
#[get("/discord_auth?<discord_code>")]
pub async fn api_get_discord_auth(handler: &State<DiscordHandler>, discord_code: &str) -> status::Custom<Json<Value>> {
    handler.api_get_discord_auth(discord_code).await
}
#[post("/register", data = "<register_data>")]
pub async fn api_post_register(handler: &State<DiscordHandler>, register_data: Json<RegisterRequest>) -> status::Custom<Json<Value>> {
    handler.api_post_register(register_data).await
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct RegisterRequest {
    username: String,
    discord_user_id: String,
    discord_refresh_token: String,
}
