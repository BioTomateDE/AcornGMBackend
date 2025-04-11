use dropbox_sdk::{
    default_async_client::{NoauthDefaultClient, UserAuthDefaultClient},
    async_routes::files,
    files::ListFolderResult,
    oauth2::Authorization,
};
use dropbox_sdk::files::UploadArg;
use tokio_util::{
    compat::FuturesAsyncReadCompatExt,
    bytes,
};
use tokio::io::AsyncReadExt;

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
                    if let Err(error) = body_stream.compat().read_to_end(&mut buf).await {
                        return Err(format!("Could not read body stream of file {path}: {error}"))
                    };
                    Ok(buf)
                }
            }
        },
    }
}


async fn upload_file(client: UserAuthDefaultClient, mut path: String, data: bytes::Bytes) -> Result<(), String> {
    let upload_args: UploadArg = UploadArg::new(path.clone())
        .with_client_modified(chrono::Utc::now().format(DROPBOX_TIMESTAMP_FORMAT).to_string());

    match files::upload(
        &client,
        &upload_args,
        data
    ).await {
        Err(error) => {
            Err(format!("Could not upload file with path {path}: {error}"))
        }
        Ok(_result) => {
            Ok(())
        }
    }
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


