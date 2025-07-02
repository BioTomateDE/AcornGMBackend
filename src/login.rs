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
};
use rocket::serde::json::Json;
use serde_json::json;
use regex::Regex;
use rocket::response::content::RawHtml;
use crate::{respond_err, respond_ok_empty, respond_ok_value, ApiResponse};

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
struct DiscordUserInfo {
    pub id: String,
    pub username: String,
    pub global_name: String,
}



const DISCORD_API_BASE_URL: &'static str = "https://discord.com/api/v10";
const REDIRECT_URI: &'static str = "https://acorngm.biotomatede.hackclub.app/discord_auth_page.html";
const DISCORD_APP_CLIENT_ID: &'static str = "1360325253766578479";
const DISCORD_CLIENT_SECRET: LazyLock<String> = LazyLock::new(
    || std::env::var("DISCORD_CLIENT_SECRET")
        .expect("DISCORD_CLIENT_SECRET environment variable not set")
);

async fn exchange_code(discord_code: &str) -> Result<TokenResponse, (Status, String)> {
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", &discord_code);
    params.insert("redirect_uri", REDIRECT_URI);

    let res = Client::new()
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

async fn get_user_info(access_token: &str) -> Result<DiscordUserInfo, String> {
    let client = Client::new();
    let res = client
        .get(format!("{}/users/@me", DISCORD_API_BASE_URL))
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| {
            error!("Could not request discord user info for discord access token \"{}...\": {e}", access_token.get(0..6).unwrap_or(access_token));
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


#[allow(private_interfaces)]
#[post("/register", data="<request_data>")]
pub async fn api_post_register(request_data: Json<RegisterRequest>) -> ApiResponse {
    info!("Handling `POST register` with username \"{}\" and discord user id {}", request_data.username, request_data.discord_user_id);
    static USERNAME_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_-]{3,32}$")
        .expect("Could not load username verification pattern"));

    if !USERNAME_REGEX.is_match(&request_data.username) {
        return Err(respond_err(Status::BadRequest, &("Invalid username! Username must be 3-32 characters long \
        and contain only latin letters, digits, underscores, and hyphens; without spaces.")))
    }

    // validate access token and discord user id
    info!("Getting discord user info for discord user id {}", request_data.discord_user_id);
    let user_info: DiscordUserInfo = get_user_info(&request_data.discord_access_token).await
        .map_err(|e| respond_err(Status::Unauthorized, &e))?;

    if user_info.id != request_data.discord_user_id {
        return Err(respond_err(Status::Unauthorized, "The provided discord user ID does not belong to the provided discord access token!"));
    }

    // check if there is already an AcornGM account connected to this discord user or with this username
    info!("Got discord user info for discord user id {}: username: \"{}\", displayname: \"{}\"", request_data.discord_user_id, user_info.username, user_info.global_name);
    let account_exists: bool = check_if_account_exists_discord(&request_data.username, &request_data.discord_user_id).await
        .map_err(|e| respond_err(Status::InternalServerError, &e))?;

    if account_exists {
        return Err(respond_err(Status::Conflict, "Account with this username or discord user id already exists!"))
    }

    // add to account list
    info!("Success, adding user with discord user id {} to account list", request_data.discord_user_id);
    let account = AcornAccount {
        username: request_data.username.clone(),
        discord_user_id: request_data.discord_user_id.clone(),
        created_at: Utc::now(),
    };

    info!("Adding account: {account:?}");
    if let Err(e) = insert_account(&account).await {
        return Err(respond_err(Status::InternalServerError, &e))
    }

    info!("User {} with Discord ID {} registered successfully.", request_data.username, request_data.discord_user_id);
    respond_ok_empty()
}


#[get("/discord_auth?<discord_code>")]
pub async fn api_get_discord_auth(discord_code: &str) -> ApiResponse {
    // Get access/refresh tokens from OAuth2 code
    info!("Handling `GET discord_auth` with code \"{discord_code}\"");
    let token_response: TokenResponse = exchange_code(discord_code).await
        .map_err(|(status, e)| respond_err(status, &format!("Error while getting discord access token: {e}")))?;
    info!("Exchanged code with discord for code \"{discord_code}\"; getting discord user info");

    // Get Discord User ID
    let user_info: DiscordUserInfo = get_user_info(&token_response.access_token).await
        .map_err(|e| respond_err(Status::InternalServerError, &format!("Error while getting discord user info: {e}")))?;

    info!("Got user info for code \"{discord_code}\"; username: \"{}\", displayname: \"{}\"", user_info.username, user_info.global_name);

    // check if account already exists
    let account_maybe: Option<AcornAccount> = get_account_by_discord_id(&user_info.id).await
        .map_err(|e| respond_err(Status::InternalServerError, &e))?;

    if let Some(account) = account_maybe {
        info!("Got discord auth for existing user {} with code \"{}\": \
            Discord ID: {}; Discord Username: {}", account.username, discord_code, user_info.id, user_info.username);

        return respond_ok_value(json!({
            "register": false,
            "discordUserId": user_info.id,
            "username": account.username,
        }))
    }

    // account does not exist; let client register
    info!("Got discord auth for new user with code \"{}\": \
        Discord ID: {}; Discord Username: {}", discord_code, user_info.id, user_info.username);

    respond_ok_value(json!({
        "register": true,
        "discordAccessToken": token_response.access_token,
        "discordUserId": user_info.id,
        "discordUsername": user_info.username,
    }))
}


#[allow(private_interfaces)]
#[post("/temp_login", data="<request_data>")]
pub async fn api_post_temp_login(request_data: Json<TempLoginRequest>) -> ApiResponse {
    info!("Handling `POST temp_login` with username {} and temp login token \"{}\"", request_data.username, request_data.temp_login_token);

    let already_exists: bool = insert_temp_login_token(&request_data.temp_login_token, &request_data.username).await
        .map_err(|e| respond_err(Status::InternalServerError, &e))?;

    if already_exists {
        return Err(respond_err(Status::Conflict, "Temp login token already exists"))
    }

    info!("Inserted temp login token into database for username {}.", request_data.username);
    respond_ok_empty()
}



/// post request because json in body is easier to deal with than in params
#[allow(private_interfaces)]
#[post("/access_token", data="<temp_login_token>")]
pub async fn api_get_access_token(temp_login_token: &str) -> ApiResponse {
    info!("Handling `GET access_token` with temp login token \"{}\"", temp_login_token);
    
    let username: String = temp_login_token_get_username(&temp_login_token).await
        .map_err(|e| respond_err(Status::InternalServerError, &e))?
        .ok_or_else(|| respond_err(Status::NotFound, "Could not find username for temp login token. \
            It may have expired or the user has not finished logging in yet."))?;

    info!("Found username {} for temp login token \"{}\"", username, temp_login_token);
    let account_exists: bool = check_if_account_exists(&username).await
        .map_err(|e| respond_err(Status::InternalServerError, &e))?;
    if !account_exists {
        return Err(respond_err(Status::NotFound, &format!("Account with username \"{username}\" does not exist!")))
    }

    // generate access token
    let mut buf = [0u8; 187];
    rand::rngs::OsRng.try_fill_bytes(&mut buf).map_err(|e| {
        error!("Could not generate cryptographically secure random bytes for token: {e}");
        respond_err(Status::InternalServerError, "Could not generate access token!")
    })?;
    let generated_token: String = base64::prelude::BASE64_URL_SAFE.encode(buf);
    
    let acorn_token = AcornAccessToken {
        token: generated_token.clone(),
        username: username.clone(),
        created_at: Utc::now(),
    };
    
    insert_access_token(&acorn_token).await.map_err(|e| respond_err(Status::InternalServerError, &e))?;
    // Success
    info!("User {} signed in", username);
    respond_ok_value(json!({"access_token": generated_token}))
}


#[get("/goto_discord_auth?<temp_login_token>")]
pub async fn redirect_goto_discord_auth(temp_login_token: String) -> RawHtml<String> {
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
