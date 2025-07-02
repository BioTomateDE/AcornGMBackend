use chrono::{DateTime, Duration, Utc};
use rocket::http::Status;
use sqlx::error::DatabaseError;
use sqlx::postgres::{PgDatabaseError, PgQueryResult};
use crate::{pool, respond_err, respond_ok_empty, ApiResponse};


#[derive(Debug, Clone)]
pub struct AcornAccount {
    pub username: String,
    pub discord_user_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AcornAccessToken {
    pub token: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
}


pub async fn check_if_account_exists(username: &str) -> Result<bool, String> {
    let result: Option<bool> = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM accounts
            WHERE username = $1
        )
        "#,
        username
    )
        .fetch_one(pool())
        .await
        .map_err(|e| format!("Failed to check if account with username {username} exists: {e}"))?;

    Ok(result.unwrap_or(false))
}

pub async fn check_if_account_exists_discord(username: &str, discord_user_id: &str) -> Result<bool, String> {
    let result: Option<bool> = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM accounts
            WHERE username = $1 OR discord_user_id = $2
        )
        "#,
        username,
        discord_user_id,
    )
        .fetch_one(pool())
        .await
        .map_err(|e| format!("Failed to check if account with username {username} exists: {e}"))?;

    Ok(result.unwrap_or(false))
}


pub async fn ensure_account_authentication(username: &str, access_token: &str) -> ApiResponse {
    let result: Option<bool> = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM access_tokens
            WHERE username = $1 AND token = $2
        )
        "#,
        username,
        access_token,
    )
        .fetch_one(pool())
        .await
        .map_err(|e| respond_err(Status::InternalServerError, &format!("Failed to verify authentication of {username}: {e}")))?;

    let authenticated: bool = result.unwrap_or(false);  // if account doesn't exist; authentication failed
    if !authenticated {
        return Err(respond_err(Status::Unauthorized, "Not authenticated; invalid username or access token"))
    }
    
    respond_ok_empty()   // will NOT actually be used as a response to the http request
}

pub async fn get_account(username: &str) -> Result<AcornAccount, String> {
    let account: AcornAccount = sqlx::query_as!(
        AcornAccount,
        r#"
        SELECT username, discord_user_id, created_at
        FROM accounts
        WHERE username = $1
        "#,
        username,
    )
        .fetch_one(pool())
        .await
        .map_err(|e| format!("Could not fetch account with username {username}: {e}"))?;
    Ok(account)
}


pub async fn get_account_by_discord_id(discord_user_id: &str) -> Result<Option<AcornAccount>, String> {
    let account: Option<AcornAccount> = sqlx::query_as!(
        AcornAccount,
        r#"
        SELECT username, discord_user_id, created_at
        FROM accounts
        WHERE discord_user_id = $1
        "#,
        discord_user_id,
    )
        .fetch_optional(pool())
        .await
        .map_err(|e| format!("Could not fetch account with discord user id {discord_user_id}: {e}"))?;
    Ok(account)
}


pub async fn get_access_token(username: &str, token: &str) -> Result<AcornAccessToken, String> {
    let row = sqlx::query!(
        r#"
        SELECT token, username, created_at
        FROM access_tokens
        WHERE username = $1 AND token = $2
        "#,
        username,
        token,
    )
        .fetch_one(pool())
        .await
        .map_err(|e| format!("Could not fetch access token with username {username}: {e}"))?;
    
    let access_token = AcornAccessToken {
        token: row.token,
        username: row.username,
        created_at: row.created_at,
    };
    Ok(access_token)
}


pub async fn insert_account(account: &AcornAccount) -> Result<(), String> {
    sqlx::query!(
        r#"
        INSERT INTO accounts (username, discord_user_id, created_at)
        VALUES ($1, $2, $3)
        "#,
        account.username,
        account.discord_user_id,
        account.created_at,
    )
        .execute(pool())
        .await
        .map_err(|e| format!("Could not insert account row for account with username {}: {e}", account.username))?;
    Ok(())
}


pub async fn insert_access_token(access_token: &AcornAccessToken) -> Result<(), String> {
    sqlx::query!(
        r#"
        INSERT INTO access_tokens (token, username, created_at)
        VALUES ($1, $2, $3)
        "#,
        access_token.token,
        access_token.username,
        access_token.created_at,
    )
        .execute(pool())
        .await
        .map_err(|e| format!("Could not insert access token row for username {}: {e}", access_token.username))?;
    Ok(())
}


/// returns whether the temp login token already exists (-> respond 404)
pub async fn insert_temp_login_token(temp_login_token: &str, username: &str) -> Result<bool, String> {
    let expires_at: DateTime<Utc> = Utc::now() + Duration::minutes(5);

    // Insert the account row
    let result: Result<PgQueryResult, sqlx::Error> = sqlx::query!(
        r#"
        INSERT INTO temp_login_tokens (token, username, expires_at)
        VALUES ($1, $2, $3)
        "#,
        temp_login_token,
        username,
        expires_at,
    )
        .execute(pool())
        .await;

    let result: sqlx::Error = match result {
        Ok(_) => return Ok(false),
        Err(e) => e,
    };

    let error: Box<dyn DatabaseError> = match result {
        sqlx::Error::Database(e) => e,
        e => return Err(format!("(generic) Could not insert temp login token row for username {username}: {e}")),
    };

    let error: &PgDatabaseError = error.downcast_ref::<PgDatabaseError>();
    if error.code() == "23505" {    // "unique violation"; temp login token already exists
        return Ok(true)
    }

    Err(format!("Could not insert temp login token row for username {username}: {error}"))
}


pub async fn delete_expired_temp_login_tokens() -> Result<(), String> {
    sqlx::query!("DELETE FROM temp_login_tokens WHERE expires_at < NOW()")
        .execute(pool())
        .await
        .map_err(|e| format!("Could not delete expired temp login tokens: {e}"))?;
    Ok(())
}


pub async fn temp_login_token_get_username(temp_login_token: &str) -> Result<Option<String>, String> {
    let result = sqlx::query!(
        r#"
        SELECT username FROM temp_login_tokens
        WHERE token = $1 AND expires_at > NOW()
        "#,
        temp_login_token,
    )
        .fetch_optional(pool())
        .await
        .map_err(|e| format!("Could not get username for temp login token: {e}"))?;

    let record = match result {
        None => return Ok(None),
        Some(record) => record,
    };

    remove_temp_login_token(temp_login_token).await?;

    Ok(Some(record.username))
}

pub async fn remove_temp_login_token(temp_login_token: &str) -> Result<(), String> {
    sqlx::query!(
        r#"
        DELETE FROM temp_login_tokens
        WHERE token = $1
        "#,
        temp_login_token,
    )
        .execute(pool())
        .await
        .map_err(|e| format!("Could not remove temp login token: {e}"))?;

    Ok(())
}

