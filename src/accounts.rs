use std::collections::HashMap;
use std::sync::Arc;
use dropbox_sdk::default_async_client::UserAuthDefaultClient;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use crate::dropbox::{download_file_string, upload_file_string};

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
    pub name: String,
    pub date_created: chrono::DateTime<chrono::Utc>,
    pub discord_id: String,
    pub access_tokens: HashMap<String, DeviceInfo>,
}


const DBX_ACCOUNTS_PATH: &'static str = "/accounts.json";

pub async fn download_accounts(client: Arc<UserAuthDefaultClient>) -> Result<Vec<AcornAccount>, String> {
    let string = download_file_string(client.as_ref(), DBX_ACCOUNTS_PATH.to_string()).await?;
    let accounts_json: Vec<AccountJson> = match serde_json::from_str(&string) {
        Ok(accounts) => accounts,
        Err(error) => return Err(format!("Could not parse accounts json: {error}")),
    };

    let mut accounts: Vec<AcornAccount> = Vec::with_capacity(accounts_json.len());
    for account_json in accounts_json {
        let date_created: chrono::DateTime<chrono::Utc> = match account_json.date_created.parse() {
            Ok(ok) => ok,
            Err(error) => return Err(format!("Could not parse creation datetime \"{}\" of Account \"{}\": {}", account_json.date_created, account_json.name, error)),
        };
        accounts.push(AcornAccount {
            name: account_json.name,
            date_created,
            discord_id: account_json.discord_id,
            access_tokens: account_json.access_tokens,
        })
    }

    Ok(accounts)
}

pub async fn upload_accounts(client: Arc<UserAuthDefaultClient>, accounts: Arc<RwLock<Vec<AcornAccount>>>) -> Result<(), String> {
    let accounts_guard = accounts.read().await;
    info!("Trying to save {} accounts to DropBox", accounts_guard.len());

    let mut accounts_json: Vec<AccountJson> = Vec::with_capacity(accounts_guard.len());
    for account in accounts_guard.iter() {
        accounts_json.push(AccountJson {
            name: account.name.clone(),
            date_created: account.date_created.to_string(),
            discord_id: account.discord_id.clone(),
            access_tokens: account.access_tokens.clone(),
        });
    }
    drop(accounts_guard);

    let json_value: serde_json::Value = serde_json::json!(accounts_json);
    let string: String = match serde_json::to_string(&json_value) {
        Ok(string) => string,
        Err(error) => return Err(format!("Could not convert accounts json to string: {error}")),
    };

    upload_file_string(client.as_ref(), DBX_ACCOUNTS_PATH.to_string(), string).await?;
    Ok(())
}

