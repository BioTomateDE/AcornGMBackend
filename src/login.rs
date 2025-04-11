use std::collections::HashMap;
use axum::extract::Query;
use axum::Json;
use axum::response::IntoResponse;
use serde::Deserialize;
use reqwest::Client;
use crate::accounts::AcornAccount;

#[derive(Debug, Clone)]
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

    if !res.status().is_success() {
        return Err(format!("Non-success response: {}", res.status()));
    }

    res.json::<TokenResponse>()
        .await
        .map_err(|error| format!("Failed to parse JSON: {error}"))
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
        .map_err(|e| format!("Request failed: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("Non-success response: {}", res.status()));
    }

    res.json::<DiscordUserInfo>()
        .await
        .map_err(|e| format!("Failed to parse JSON: {e}"))
}

async fn handle_get_discord_auth(discord_app_client_secret: &str, accounts: &[AcornAccount], Query(params): Query<DiscordAuthQuery>) -> impl IntoResponse {
    // Get access/refresh tokens from OAuth2 code
    let token_response: TokenResponse = match exchange_code(discord_app_client_secret, &params.discord_code).await {
        Ok(token_response) => token_response,
        Err(error) => return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Error while getting discord access token: {error}")}))
        )
    };

    // Get Discord User ID
    let user_info: DiscordUserInfo = match get_user_info(&token_response.access_token).await {
        Ok(user_info) => user_info,
        Err(error) => return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Error while getting discord user info: {error}")}))
        )
    };

    // check if account already exists; if it does, return acorn access token
    for account in accounts {
        if account.discord_id != user_info.id { continue }
        return (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "register": true,
            }))
        )
    }

    // account does not exist; let client register
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "register": false,
            "discordAccessToken": token_response.access_token,
            "discordUserId": user_info.id,
        }))
    )
}

