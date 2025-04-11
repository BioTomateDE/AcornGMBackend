use std::ops::DerefMut;
use dropbox_sdk;
use dropbox_sdk::async_client_trait::UserAuthClient;
use dropbox_sdk::async_routes::files;
use dropbox_sdk::default_async_client::{NoauthDefaultClient, UserAuthDefaultClient};
use dropbox_sdk::Error;
use dropbox_sdk::files::{ListFolderError, ListFolderResult};
use dropbox_sdk::oauth2::Authorization;
use tokio::io::AsyncReadExt;
use tokio_util::bytes;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tower_http::follow_redirect::policy::PolicyExt;

enum Operation {
    List(String),
    Download(String),
    Stat(String),
}

const DROPBOX_TIMESTAMP_FORMAT: &'static str = "%a, %d %b %Y %H:%M:%S %z";

pub async fn initialize_dropbox() -> UserAuthDefaultClient {
    let mut auth: Authorization = dropbox_sdk::oauth2::get_auth_from_env_or_prompt();
    if auth.save().is_none() {
        auth.obtain_access_token_async(NoauthDefaultClient::default()).await.unwrap();
        eprintln!("Next time set these environment variables to reuse this authorization:");
        eprintln!("  DBX_CLIENT_ID={}", auth.client_id());
        eprintln!("  DBX_OAUTH={}", auth.save().unwrap());
    }
    let client: UserAuthDefaultClient = UserAuthDefaultClient::new(auth);
    client
}


async fn download_file(client: UserAuthDefaultClient, path: String) -> Result<Vec<u8>, String> {
    match files::download(&client, &files::DownloadArg::new(path.clone()), None, None).await {
        Err(error) => Err(format!("Could not download file {path}: {error}")),
        Ok(result) => {
            match result.body {
                None => Err(format!("Response body was None for file {path}")),
                Some(body_stream) => {
                    let mut buf: Vec<u8> = Vec::new();
                    body_stream.compat().read_to_end(&mut buf).await?;
                    Ok(buf)
                }
            }
        },
    }
}


async fn upload_file(client: UserAuthDefaultClient, mut path: String, data: bytes::Bytes) -> Result<(), String> {
    files::upload(
        &client,
        &files::UploadArg {
            path,
            mode: files::WriteMode::Overwrite,
            autorename: false,
            client_modified: Some(chrono::Utc::now().format(DROPBOX_TIMESTAMP_FORMAT).to_string()),
            mute: false,
            property_groups: None,
            strict_conflict: false,
            content_hash: None,
        },
        data
    )
}


async fn list_files(client: UserAuthDefaultClient, mut path: String) -> Result<ListFolderResult, String> {
    if path == "/" {
        path.clear();
    }

    match files::list_folder(&client, &files::ListFolderArg::new(path.clone()).with_recursive(true)).await {
        Err(error) => {
            Err(format!("Could not get files in folder {path}: {error}"))
        }
        Ok(result) => {
            Ok(result)
        }
    }
}


async fn ts(client: UserAuthDefaultClient) {
    let path = "";
    match Operation::Download {
        Operation::List(mut path) => {
            eprintln!("Listing recursively: {path}");

            // Special case: the root folder is empty string. All other paths need to start with '/'.
            if path == "/" {
                path.clear();
            }

            let mut result: ListFolderResult = match files::list_folder(
                &client,
                &files::ListFolderArg::new(path).with_recursive(true),
            )
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Error from files/list_folder: {e}");
                    return;
                }
            };

            let mut num_entries = result.entries.len();
            let mut num_pages = 1;

            loop {
                for entry in result.entries {
                    match entry {
                        files::Metadata::Folder(entry) => {
                            println!("Folder: {}", entry.path_display.unwrap_or(entry.name));
                        }
                        files::Metadata::File(entry) => {
                            println!("File: {}", entry.path_display.unwrap_or(entry.name));
                        }
                        files::Metadata::Deleted(entry) => {
                            panic!("unexpected deleted entry: {:?}", entry);
                        }
                    }
                }

                if !result.has_more {
                    break;
                }

                result = match files::list_folder_continue(
                    &client,
                    &files::ListFolderContinueArg::new(result.cursor),
                )
                    .await
                {
                    Ok(result) => {
                        num_pages += 1;
                        num_entries += result.entries.len();
                        result
                    }
                    Err(e) => {
                        eprintln!("Error from files/list_folder_continue: {e}");
                        break;
                    }
                }
            }

            eprintln!("{num_entries} entries from {num_pages} result pages");
        }
        Operation::Stat(path) => {
            eprintln!("listing metadata for: {path}");

            let arg = files::GetMetadataArg::new(path)
                .with_include_media_info(true)
                .with_include_deleted(true)
                .with_include_has_explicit_shared_members(true);

            match files::get_metadata(&client, &arg).await {
                Ok(result) => println!("{result:#?}"),
                Err(e) => eprintln!("Error from files/get_metadata: {e}"),
            }
        }
    }
}


