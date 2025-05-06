use std::collections::HashMap;
use std::sync::LazyLock;
use base64::Engine;
use chrono::Utc;
use rand::TryRngCore;
use serde::Deserialize;
use reqwest::Client;
use rocket::http::Status;
use crate::accounts::{
    check_if_account_exists,
    check_if_account_exists_discord,
    get_account_by_discord_id,
    insert_access_token,
    insert_account,
    insert_temp_login_token,
    temp_login_token_get_username,
    AcornAccessToken,
    AcornAccount,
    DeviceInfo,
};
use rocket::response::status;
use rocket::serde::json::Json;
use serde_json::{json, Value};
use rocket::State;
use regex::Regex;
use rocket::response::content::RawHtml;
use sqlx::PgPool;

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
fn respond_ok_value(json_response: Value) -> RespType {
    status::Custom(
        Status::Ok,
        Json(json!(json_response)),
    )
}
fn respond_ok_empty() -> RespType {
    status::Custom(
        Status::NoContent,
        Json(json!({})),
    )
}


const DISCORD_API_BASE_URL: &'static str = "https://discord.com/api/v10";
const REDIRECT_URI: &'static str = "https://acorngm.biotomatede.hackclub.app/discord_auth_page.html";
const DISCORD_APP_CLIENT_ID: &'static str = "1360325253766578479";
const DISCORD_CLIENT_SECRET: LazyLock<String> = LazyLock::new(
    || std::env::var("DISCORD_CLIENT_SECRET")
        .expect("DISCORD_CLIENT_SECRET environment variable not set")
);

async fn get_access_token(params: HashMap<&str, &str>) -> Result<TokenResponse, (Status, String)> {
    let client: Client = Client::new();
    let res = client
        .post(format!("{}/oauth2/token", DISCORD_API_BASE_URL))
        .basic_auth(DISCORD_APP_CLIENT_ID, Some(&*DISCORD_CLIENT_SECRET))
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
            return Err((Status::Unauthorized, "Could not get discord token because the provided code is invalid".to_string()));
        }
        if body.contains("invalid_grant") {
            return Err((Status::Unauthorized, "Could not get discord token because the provided discord refresh token is invalid or already used".to_string()));
        }
        error!("Error while getting access token from discord - {}: {}", status, body);
        return Err((Status::InternalServerError, format!("Could not get discord token because discord returned a failure response: {}", status)));
    }

    serde_json::from_str::<TokenResponse>(&body).map_err(|e| (Status::InternalServerError, {
        error!("Failed to parse JSON while getting discord token: {e}\nRaw response text: {body}");
        format!("Failed to parse JSON while getting discord token: {e}")
    }))
}

async fn exchange_code(code: &str) -> Result<TokenResponse, (Status, String)> {
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", &code);
    params.insert("redirect_uri", REDIRECT_URI);
    get_access_token(params).await
}

async fn refresh_token(refresh_token: &str) -> Result<TokenResponse, (Status, String)> {
    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("refresh_token", refresh_token);
    get_access_token(params).await
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
    pool: PgPool,
}
impl AccountHandler {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
        }
    }

    async fn api_get_discord_auth(&self, code: &str) -> status::Custom<Json<Value>> {
        // Get access/refresh tokens from OAuth2 code
        info!("Handling `GET discord_auth` with code \"{code}\"");
        let token_response: TokenResponse = match exchange_code(code).await {
            Ok(token_response) => token_response,
            Err((status, error)) => return status::Custom(status, Json(json!({
                "error": format!("Error while getting discord access token: {error}"),
            })))
        };
        info!("Exchanged code with discord for code \"{code}\"; getting discord user info");

        // Get Discord User ID
        let user_info: DiscordUserInfo = match get_user_info(&token_response.access_token).await {
            Ok(user_info) => user_info,
            Err(error) => return respond_err(Status::InternalServerError, &format!("Error while getting discord user info: {error}")),
        };

        info!("Got user info for code \"{code}\"; username: \"{}\", displayname: \"{}\"", user_info.username, user_info.global_name);

        // check if account already exists
        let result: Result<Option<AcornAccount>, String> = get_account_by_discord_id(&self.pool, &user_info.id).await;
        let account_maybe: Option<AcornAccount> = match result {
            Err(e) => return respond_err(Status::InternalServerError, &e),
            Ok(account) => account
        };

        if let Some(account) = account_maybe {
            info!("Got discord auth for existing user {} with code \"{}\": \
            Discord ID: {}; Discord Username: {}", account.username, code, user_info.id, user_info.username);

            return respond_ok_value(json!({
                "register": false,
                "discordUserId": user_info.id,
                "username": account.username,
            }))
        }

        // account does not exist; let client register
        info!("Got discord auth for new user with code \"{}\": \
        Discord ID: {}; Discord Username: {}", code, user_info.id, user_info.username);

        respond_ok_value(json!({
            "register": true,
            "discordAccessToken": token_response.access_token,
            "discordUserId": user_info.id,
            "discordUsername": user_info.username,
        }))
    }

    async fn api_post_register(&self, register_data: Json<RegisterRequest>) -> status::Custom<Json<Value>> {
        info!("Handling `POST register` with username \"{}\" and discord user id {}", register_data.username, register_data.discord_user_id);
        static USERNAME_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_-]{3,32}$")
            .expect("Could not load username verification pattern"));

        if !USERNAME_REGEX.is_match(&register_data.username) {
            return respond_err(Status::BadRequest, &("Invalid username! Username must be 3-32 characters long \
            and contain only latin letters, digits, underscores, and hyphens; without spaces."))
        }

        // validate access token and discord user id
        info!("Getting discord user info for discord user id {}", register_data.discord_user_id);
        let user_info: DiscordUserInfo = match get_user_info(&register_data.discord_access_token).await {
            Ok(info) => info,
            Err(e) => return respond_err(Status::Unauthorized, &e),
        };

        if user_info.id != register_data.discord_user_id {
            return respond_err(Status::Unauthorized, "The provided discord user ID does not belong to the provided discord access token!");
        }

        // check if there is already an AcornGM account connected to this discord user or with this username
        info!("Got discord user info for discord user id {}: username: \"{}\", displayname: \"{}\"", register_data.discord_user_id, user_info.username, user_info.global_name);
        let result: Result<bool, String> = check_if_account_exists_discord(&self.pool, &register_data.username, &register_data.discord_user_id).await;
        match result {
            Err(e) => return respond_err(Status::InternalServerError, &e),
            Ok(account_exists) => if account_exists {
                return respond_err(Status::Conflict, "Account with this username or discord user id already exists!")
            }
        }

        // add to account list
        info!("Success, adding user with discord user id {} to account list", register_data.discord_user_id);
        let account = AcornAccount {
            username: register_data.username.clone(),
            discord_user_id: register_data.discord_user_id.clone(),
            created_at: Utc::now(),
        };

        info!("Adding account: {account:?}");
        if let Err(e) = insert_account(&self.pool, &account).await {
            return respond_err(Status::InternalServerError, &e)
        }

        info!("User {} with Discord ID {} registered successfully.", register_data.username, register_data.discord_user_id);
        respond_ok_empty()
    }

    async fn api_post_temp_login(&self, temp_login_data: Json<TempLoginRequest>) -> RespType {
        info!("Handling `POST temp_login` with username {} and temp login token \"{}\"", temp_login_data.username, temp_login_data.temp_login_token);
        let result: Result<bool, String> = insert_temp_login_token(&self.pool, &temp_login_data.temp_login_token, &temp_login_data.username).await;
        match result {
            Err(e) => respond_err(Status::InternalServerError, &e),
            Ok(already_exists) => if already_exists {
                respond_err(Status::Conflict, "Temp login token already exists")
            } else {
                info!("Inserted temp login token into database for username {}.", temp_login_data.username);
                respond_ok_empty()
            }
        }
    }

    async fn api_get_access_token(&self, temp_login_token: &String, device_info: &DeviceInfo) -> RespType {
        info!("Handling `GET access_token` with temp login token \"{}\"", temp_login_token);
        let result: Result<Option<String>, String> = temp_login_token_get_username(&self.pool, temp_login_token).await;
        let username: String = match result {
            Err(e) => return respond_err(Status::InternalServerError, &e),
            Ok(username) => match username {
                None => return respond_err(Status::NotFound, "Could not find username for temp login token. \
                It may have expired or the user has not finished logging in yet."),
                Some(username) => username,
            }
        };

        info!("Found username {} for temp login token \"{}\"", username, temp_login_token);
        let result: Result<bool, String> = check_if_account_exists(&self.pool, &username).await;
        match result {
            Err(e) => return respond_err(Status::InternalServerError, &e),
            Ok(account_exists) => if !account_exists {
                return respond_err(Status::NotFound, &format!("Account with username \"{username}\" does not exist!"))
            }
        }

        // generate access token
        let mut buf = [0u8; 187];
        if let Err(e) = rand::rngs::OsRng.try_fill_bytes(&mut buf) {
            error!("Could not generate cryptographically secure random bytes for token: {e}");
            return respond_err(Status::InternalServerError, "Could not generate access token!")
        };
        let generated_token: String = base64::prelude::BASE64_URL_SAFE.encode(buf);

        let acorn_token = AcornAccessToken {
            token: generated_token.clone(),
            username: username.clone(),
            device_info: device_info.clone(),
            created_at: Utc::now(),
        };

        if let Err(e) = insert_access_token(&self.pool, &acorn_token).await {
            return respond_err(Status::InternalServerError, &e)
        }

        // Success
        info!("User {} signed in on {}", username, device_info.distro);
        respond_ok_value(json!({
            "access_token": generated_token,
        }))
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
        &redirect_uri=https%3A%2F%2Facorngm.biotomatede.hackclub.app%2Fdiscord_auth_page.html\
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
    discord_access_token: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct TempLoginRequest {
    temp_login_token: String,
    username: String,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct GetAccessTokenRequest {
    temp_login_token: String,
    device_info: DeviceInfo,
}

