use std::collections::HashMap;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::error::DatabaseError;
use sqlx::PgPool;
use sqlx::postgres::{PgDatabaseError, PgQueryResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(crate = "rocket::serde")]
pub struct DeviceInfo {
    pub distro: String,
    pub platform: String,
    pub desktop_environment: String,
    pub cpu_architecture: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountJson {
    name: String,
    date_created: String,      // will be converted to chrono timestamp later
    discord_id: String,
    access_tokens: HashMap<String, DeviceInfo>,
}

#[derive(Debug, Clone)]
pub struct AcornAccount {
    pub username: String,
    pub discord_user_id: String,
    pub created_at: DateTime<Utc>,
    pub access_tokens: HashMap<String, DeviceInfo>,
}


pub async fn insert_account(pool: &PgPool, account: &AcornAccount) -> Result<(), String> {
    // Insert the account row
    sqlx::query!(
        r#"
        INSERT INTO accounts (username, discord_user_id, created_at)
        VALUES ($1, $2, $3)
        "#,
        account.username,
        account.discord_user_id,
        account.created_at,
    )
        .execute(pool)
        .await
        .map_err(|e| format!("Could not insert account row for account with username {}: {e}", account.username))?;

    // Insert all access_tokens
    for (token, device_info) in &account.access_tokens {
        let device_info_json = serde_json::to_value(device_info)
            .map_err(|e| format!("Could not convert device info to json for account with username {}: {e}", account.username))?;

        sqlx::query!(
            r#"
            INSERT INTO access_tokens (username, token, device_info)
            VALUES ($1, $2, $3)
            "#,
            account.discord_user_id,
            token,
            device_info_json,
        )
            .execute(pool)
            .await
            .map_err(|e| format!("Could not insert access token row for account with username {}: {e}", account.username))?;
    }

    Ok(())
}


/// returns whether the temp login token already exists (-> respond 404)
pub async fn insert_temp_login_token(pool: &PgPool, temp_login_token: &str, username: &str) -> Result<bool, String> {
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
        .execute(pool)
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


pub async fn delete_expired_temp_login_tokens(pool: &PgPool) -> Result<(), String> {
    sqlx::query!("DELETE FROM temp_login_tokens WHERE expires_at < NOW()")
        .execute(&pool)
        .await
        .map_err(|e| format!("Could not delete expired temp login tokens: {e}"))?;
    Ok(())
}


pub async fn temp_login_token_get_username(pool: &PgPool, temp_login_token: &str) -> Result<Option<String>, String> {
    let result = sqlx::query!(
        r#"
        SELECT username FROM temp_login_tokens
        WHERE token = $1 AND expires_at > NOW()
        "#,
        temp_login_token,
    )
        .fetch_optional(&pool)
        .await
        .map_err(|e| format!("Could not get username for temp login token: {e}"))?;

    match result {
        None => Ok(None),
        Some(record) => Ok(record.username)
    }
}

pub async fn remove_temp_login_token(pool: &PgPool, temp_login_token: &str) -> Result<(), String> {
    sqlx::query!(
        r#"
        DELETE FROM temp_login_tokens
        WHERE token = $1
        "#,
        temp_login_token,
    )
        .execute(&pool)
        .await
        .map_err(|e| format!("Could not remove temp login token: {e}"))?;

    Ok(())
}

