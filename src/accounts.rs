use std::collections::HashMap;
use dropbox_sdk::default_async_client::UserAuthDefaultClient;
use serde::{Deserialize, Serialize};
use crate::dropbox::{download_file_string, upload_file_string};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    host_name: String,
    distro_pretty: String,
    platform_pretty: String,
    desktop_environment_pretty: String,
    cpu_architecture: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountJson {
    name: String,
    date_created: String,      // will be converted to chrono timestamp later
    discord_id: String,
    discord_refresh_token: String,
    access_tokens: HashMap<String, DeviceInfo>,
}

#[derive(Debug, Clone)]
pub struct AcornAccount {
    pub name: String,
    pub date_created: chrono::DateTime<chrono::Utc>,
    pub discord_id: String,
    pub discord_refresh_token: String,
    pub access_tokens: HashMap<String, DeviceInfo>,
}


const DBX_ACCOUNTS_PATH: &'static str = "/accounts.json";

pub async fn download_accounts(client: UserAuthDefaultClient) -> Result<Vec<AcornAccount>, String> {
    let string = download_file_string(client, DBX_ACCOUNTS_PATH.to_string()).await?;
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
            discord_refresh_token: account_json.discord_refresh_token,
            access_tokens: account_json.access_tokens,
        })
    }

    Ok(accounts)
}

pub async fn upload_accounts(client: UserAuthDefaultClient, accounts: &[AcornAccount]) -> Result<(), String> {
    let mut accounts_json: Vec<AccountJson> = Vec::with_capacity(accounts.len());
    for account in accounts {
        accounts_json.push(AccountJson {
            name: account.name.clone(),
            date_created: account.date_created.to_string(),
            discord_id: account.discord_id.clone(),
            discord_refresh_token: account.discord_refresh_token.clone(),
            access_tokens: account.access_tokens.clone(),
        });
    }

    let json_value: serde_json::Value = serde_json::json!(accounts_json);
    let string: String = match serde_json::to_string(&json_value) {
        Ok(string) => string,
        Err(error) => return Err(format!("Could not convert accounts json to string: {error}")),
    };

    upload_file_string(client, DBX_ACCOUNTS_PATH.to_string(), string).await?;
    Ok(())
}

