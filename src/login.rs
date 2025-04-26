use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use base64::Engine;
use dropbox_sdk::default_async_client::UserAuthDefaultClient;
use rand::TryRngCore;
use serde::Deserialize;
use reqwest::Client;
use rocket::http::Status;
use crate::accounts::{upload_accounts, AcornAccount, DeviceInfo};
use rocket::response::status;
use rocket::serde::json::Json;
use serde_json::{json, Value};
use rocket::State;
use regex::Regex;
use rocket::response::content::RawHtml;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    refresh_token: String,
    scope: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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
    let body: String = res.text().await
        .map_err(|e| {
            error!("Could not get text from response body while getting discord access token: {e}",);
            (Status::InternalServerError, format!("Could not get text from response body: {e}"))
        })?;
    
    if !status.is_success() {
        // check if code is invalid; because if it is, the error is the client's fault
        if body.contains("Invalid \\\"code\\\" in request") {
            return Err((Status::Unauthorized, "Could not refresh discord token because the provided code is invalid".to_string()));
        }
        if body.contains("invalid_grant") {
            return Err((Status::Unauthorized, "Could not refresh discord token because the provided discord refresh token is invalid or already used".to_string()));
        }
        error!("Error while getting access token from discord - {}: {}", status, body);
        return Err((Status::InternalServerError, format!("Could not refresh discord token because discord returned a failure response: {}", status)));
    }

    serde_json::from_str::<TokenResponse>(&body).map_err(|e| (Status::InternalServerError, {
        error!("Failed to parse JSON while refreshing discord token: {e}\nRaw response text: {body}");
        format!("Failed to parse JSON while refreshing discord token: {e}")
    }))
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
        .map_err(|e| {
            error!("Could not request discord user info for discord access \
            token \"{}...\": {e}", access_token.get(0..6).unwrap_or(access_token));
            format!("Could not request discord user info: {e}")
        })?;

    let status = res.status();

    let body: String = res.text().await
        .map_err(|e| {
            error!("Could not get text from response body while getting discord user info \
            for discord access token \"{}...\": {e}", access_token.get(0..6).unwrap_or(access_token));
            format!("Could not get text from response body: {e}")
        })?;

    if !status.is_success() {
        error!("Error while getting discord user info: {} - {}", status, body);
        return Err(format!("Could not get discord user info because discord returned a failure response: {status}"));
    }

    serde_json::from_str::<DiscordUserInfo>(&body).map_err(|e| {
        error!("Failed to parse JSON from discord user info response: {e}\nRaw response text: {body}");
        format!("Failed to parse JSON from discord user info response: {e}")
    })
}


pub struct AccountHandler {
    dropbox: Arc<UserAuthDefaultClient>,
    discord_app_client_secret: String,
    accounts: Arc<RwLock<Vec<AcornAccount>>>,
    temp_login_tokens: Arc<RwLock<HashMap<String, String>>>,
}
impl AccountHandler {
    pub fn new(
        dropbox: Arc<UserAuthDefaultClient>,
        discord_app_client_secret: &str,
        accounts: Arc<RwLock<Vec<AcornAccount>>>,
        temp_login_tokens: Arc<RwLock<HashMap<String, String>>>,
    ) -> Self {
        Self {
            dropbox,
            discord_app_client_secret: discord_app_client_secret.to_string(),
            accounts,
            temp_login_tokens,
        }
    }

    async fn api_get_discord_auth(&self, code: &str) -> status::Custom<Json<Value>> {
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

        // check if account already exists
        for account in self.accounts.clone().read().await.iter() {
            if account.discord_id == user_info.id {
                info!("Got discord auth for existing user {} with code \"{}\": \
                    Discord ID: {}; Discord Username: {}", account.name, code, user_info.id, user_info.username);
                return respond_ok(json!({
                    "register": false,
                }));
            }
        }

        // account does not exist; let client register
        info!("Got discord auth for new user with code \"{}\": \
            Discord ID: {}; Discord Username: {}", code, user_info.id, user_info.username);
        respond_ok(json!({
            "register": true,
            "discordRefreshToken": token_response.refresh_token,
            "discordUserId": user_info.id,
            "discordUsername": user_info.username,
        }))
    }

    async fn api_post_register(&self, register_data: Json<RegisterRequest>) -> status::Custom<Json<Value>> {
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
            Ok(info) => info,
            Err(e) => return respond_err(Status::Unauthorized, &e),
        };

        if user_info.id != register_data.discord_user_id {
            return respond_err(Status::Unauthorized, "The provided discord user ID does not belong to the provided discord access token!");
        }

        // check if there is already an AcornGM account connected to this discord user or with this username
        for account in self.accounts.clone().read().await.iter() {
            if account.discord_id == register_data.discord_user_id {
                return respond_err(Status::Conflict, "There is already an AcornGM account connected to this discord account!");
            }
            if account.name.to_lowercase() == register_data.username.to_lowercase() {
                return respond_err(Status::Conflict, "Username is already taken!");
            }
        }

        // success; add to account list
        let account = AcornAccount {
            name: register_data.username.clone(),
            date_created: chrono::Utc::now(),
            discord_id: register_data.discord_user_id.clone(),
            access_tokens: HashMap::new(),
        };

        info!("Updating accounts: {account:?}");
        let accounts_arc =  self.accounts.clone();
        let mut accounts_guard = accounts_arc.write().await;
        accounts_guard.push(account);
        drop(accounts_guard);

        // save accounts
        info!("Uploading accounts...");
        if let Err(error) = upload_accounts(self.dropbox.clone(), self.accounts.clone()).await {
            for _ in 0..3 { error!("!!!!!Important!!!!! Failed to upload accounts to DropBox after registering new account ({}): {error}", register_data.username) }
            return respond_err(Status::InternalServerError,
    "Could not save account! If this is a reoccurring issue, contact BioTomateDE as soon as possible.")
        };

        info!("User {} with Discord ID {} registered successfully.", register_data.username, register_data.discord_user_id);
        respond_ok(json!({}))
    }

    async fn api_post_temp_login(&self, temp_login_data: Json<TempLoginRequest>) -> RespType {
        // {~~} check if account with that discord id exists

        let mut temp_login_tokens = self.temp_login_tokens.write().await;
        temp_login_tokens.insert(temp_login_data.temp_login_token.clone(), temp_login_data.discord_id.clone());

        respond_ok(json!({}))
    }

    async fn api_get_access_token(&self, temp_login_token: &String, device_info: &DeviceInfo) -> RespType {
        let temp_login_tokens = self.temp_login_tokens.read().await;
        let discord_id: &String = match temp_login_tokens.get(temp_login_token) {
            Some(id) => id,
            None => return respond_err(Status::NotFound, &format!("The provided temp login token doesn't exist: {temp_login_token}")),
        };

        let mut accounts = self.accounts.write().await;
        for account in accounts.iter_mut() {
            if account.discord_id == *discord_id {
                // generate access token
                let mut buf = [0u8; 187];
                if let Err(e) = rand::rngs::OsRng.try_fill_bytes(&mut buf) {
                    error!("Could not generate cryptographically secure random bytes for token: {e}");
                    return respond_err(Status::InternalServerError, "Could not generate access token!")
                };
                let generated_token: String = base64::prelude::BASE64_URL_SAFE.encode(buf);

                // modify `accounts` vec
                account.access_tokens.insert(generated_token.clone(), device_info.clone());

                // save accounts
                if let Err(error) = upload_accounts(self.dropbox.clone(), self.accounts.clone()).await {
                    for _ in 0..3 { error!("!!!!!Important!!!!! Failed to upload accounts to DropBox after generating new access token: {error}") }
                    return respond_err(Status::InternalServerError,
                        "Could not save account! If this is a reoccurring issue, contact BioTomateDE as soon as possible.")
                };

                info!("User {} signed in on {}", account.name, device_info.distro_pretty);
                return respond_ok(json!({
                    "access_token": generated_token,
                }))
            }
        }

        respond_err(Status::NotFound, &format!("The provided temp login token exists, but there is no account associated with its discord id: {discord_id}"))
    }
}
#[post("/register", data="<register_data>")]
pub async fn api_post_register(handler: &State<AccountHandler>, register_data: Json<RegisterRequest>) -> RespType {
    handler.api_post_register(register_data).await
}
#[get("/discord_auth?<discord_code>")]
pub async fn api_get_discord_auth(handler: &State<AccountHandler>, discord_code: &str) -> RespType {
    handler.api_get_discord_auth(discord_code).await
}

/// post request because json in body is easier to deal with than in params
#[post("/access_token", format="json", data="<get_access_token_data>")]
pub async fn api_get_access_token(handler: &State<AccountHandler>, get_access_token_data: Json<GetAccessTokenRequest>) -> RespType {
    handler.api_get_access_token(&get_access_token_data.temp_login_token, &get_access_token_data.device_info).await
}
#[post("/temp_login", data="<temp_login_data>")]
pub async fn api_post_temp_login(handler: &State<AccountHandler>, temp_login_data: Json<TempLoginRequest>) -> RespType {
    handler.api_post_temp_login(temp_login_data).await
}

#[get("/goto_discord_auth?<temp_login_token>")]
pub async fn redirect_get_goto_discord_auth(temp_login_token: String) -> RawHtml<String> {
    const DISCORD_AUTH_URL: &'static str = "https://discord.com/oauth2/authorize\
        ?client_id=1360325253766578479\
        &response_type=code\
        &redirect_uri=https%3A%2F%2Facorngm.onrender.com%2Fdiscord_auth_page.html\
        &scope=identify";

    RawHtml(format!("\
    <!DOCTYPE html>\
    <html>\
    <head>\
    <title>AcornGM</title>\
    </head>\
    <body>\
    <h1>Redirecting to Discord...</h1>\
    <script>\
    localStorage.setItem('tempLoginToken', '{temp_login_token}');\
    window.location.replace('{DISCORD_AUTH_URL}')\
    </script>\
    </body>\
    </html>\
    "))
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct RegisterRequest {
    username: String,
    discord_user_id: String,
    discord_refresh_token: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct TempLoginRequest {
    temp_login_token: String,
    discord_id: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct GetAccessTokenRequest {
    temp_login_token: String,
    device_info: DeviceInfo,
}

