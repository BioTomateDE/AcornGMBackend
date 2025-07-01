use std::str::FromStr;
use rocket::Data;
use rocket::form::validate::Contains;
use rocket::http::{ContentType, Status};
use rocket_multipart_form_data::{MultipartFormData, MultipartFormDataField, MultipartFormDataOptions};
use sqlx::QueryBuilder;
use uuid::Uuid;
use crate::{pool, respond_err, respond_ok_empty, RespType};
use crate::accounts::ensure_account_authentication;
use crate::sanitize::sanitize_string;


const MAX_FILE_SIZE: u64 = 16 * 1024 * 1024;   // 16 MB


#[put("/mod", data = "<data>")]
pub async fn api_upload_mod(content_type: &ContentType, data: Data<'_>) -> RespType {
    let err_400 = |e: String| respond_err(Status::BadRequest, &e);
    info!("Handling `PUT` mod");

    // Check MIME type
    if !content_type.is_binary() {
        warn!("Unsupported content type: {content_type}");
        return Err(respond_err(Status::BadRequest, &format!("Unsupported content type {}", content_type)));
    }

    let form_options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
        MultipartFormDataField::text("username"),
        MultipartFormDataField::text("access_token"),
        MultipartFormDataField::raw("file_data").size_limit(MAX_FILE_SIZE),
        MultipartFormDataField::text("title"),
        MultipartFormDataField::text("description"),
        MultipartFormDataField::text("game_name"),
        MultipartFormDataField::text("game_version"),
    ]);
    let form_data: MultipartFormData = MultipartFormData::parse(content_type, data, form_options).await
        .map_err(|e| format!("Could not parse form data: {e}")).map_err(err_400)?;
    
    let username: &String = get_text_form_field(&form_data, "username").map_err(err_400)?;
    let access_token: &String = get_text_form_field(&form_data, "access_token").map_err(err_400)?;
    let file_data: &Vec<u8> = get_bytes_form_field(&form_data, "file_data").map_err(err_400)?;
    let title: &String = get_text_form_field(&form_data, "title").map_err(err_400)?;
    let description: &String = get_text_form_field(&form_data, "description").map_err(err_400)?;
    let game_name: &String = get_text_form_field(&form_data, "game_name").map_err(err_400)?;
    let game_version: &String = get_text_form_field(&form_data, "game_version").map_err(err_400)?;

    ensure_account_authentication(&username, &access_token).await?;

    let title: String = sanitize_string(title).ok_or_else(|| respond_err(Status::BadRequest, "Invalid title"))?;
    if title.len() > 256 || title.len() < 8 {
        return Err(respond_err(Status::BadRequest, "Title should be 8-256 chars long"))
    }
    if title.contains("\n") || title.contains("\r") {
        return Err(respond_err(Status::BadRequest, "Title must not contain newlines"))
    }

    let description: String = sanitize_string(description).ok_or_else(|| respond_err(Status::BadRequest, "Invalid description"))?;
    
    if !matches!(game_name.as_str(), "Undertale" | "Deltarune") {
        return Err(respond_err(Status::BadRequest, "Invalid or unknown game name"))
    }
    
    let err_game_ver = || respond_err(Status::BadRequest, "Invalid game version");
    let mut game_version_parts = game_version.split('.');
    let game_version_minor: i32 = game_version_parts.next().ok_or_else(err_game_ver)?.parse::<u32>().map_err(|_| err_game_ver())? as i32;
    let game_version_major: i32 = game_version_parts.next().ok_or_else(err_game_ver)?.parse::<u32>().map_err(|_| err_game_ver())? as i32;
    if game_version_parts.next().is_some() {
        return Err(err_game_ver())
    }
    
    sqlx::query!(
        r#"
        INSERT INTO mods (author, file_data, title, description, game_name, game_version_major, game_version_minor)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        username,
        file_data,
        title,
        description,
        game_name,
        game_version_major,
        game_version_minor,
    )
        .execute(pool())
        .await
        .map_err(|e| respond_err(
            Status::InternalServerError,
            &format!("Could not create mod for mod with title \"{title}\": {e}"))
        )?;
    
    respond_ok_empty()
}


#[patch("/mod", data = "<data>")]
pub async fn api_update_mod(content_type: &ContentType, data: Data<'_>) -> RespType {
    let err_400 = |e: String| respond_err(Status::BadRequest, &e);
    info!("Handling `PATCH` mod");

    // Check MIME type
    if !content_type.is_binary() {
        warn!("Unsupported content type: {content_type}");
        return Err(respond_err(Status::BadRequest, &format!("Unsupported content type {}", content_type)));
    }

    let form_options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
        MultipartFormDataField::text("username"),
        MultipartFormDataField::text("access_token"),
        MultipartFormDataField::text("mod_id"),
        MultipartFormDataField::raw("file_data").size_limit(MAX_FILE_SIZE),
        MultipartFormDataField::text("description"),
    ]);
    let form_data: MultipartFormData = MultipartFormData::parse(content_type, data, form_options).await
        .map_err(|e| format!("Could not parse form data: {e}")).map_err(err_400)?;

    let username: &String = get_text_form_field(&form_data, "username").map_err(err_400)?;
    let access_token: &String = get_text_form_field(&form_data, "access_token").map_err(err_400)?;
    ensure_account_authentication(username, access_token).await?;
    
    let mod_id: &String = get_text_form_field(&form_data, "mod_id").map_err(err_400)?;
    let mod_id: Uuid = Uuid::from_str(mod_id).map_err(|e| format!("Invalid Mod UUID: {e}")).map_err(err_400)?;
    ensure_mod_authorization(mod_id, username).await?;
    
    let file_data: Option<&Vec<u8>> = get_bytes_form_field_opt(&form_data, "file_data");
    let description: Option<&String> = get_text_form_field_opt(&form_data, "description");
    
    if file_data.is_none() && description.is_none() {
        return Err(respond_err(Status::BadRequest, "Nothing to update"))
    }
    
    let description: Option<String> = if let Some(desc) = description {
        Some(sanitize_string(desc).ok_or_else(|| respond_err(Status::BadRequest, "Invalid description"))?)
    } else { None };

    let mut query = QueryBuilder::new("UPDATE mods SET ");
    let mut separated = query.separated(", ");
    if let Some(file_data) = file_data {
        separated.push("file_data=").push_bind(file_data);
    }
    if let Some(desc) = description {
        separated.push("description=").push_bind(desc);
    }
    separated.push("mod_version = mod_version + 1");
    
    query.push(" WHERE id=").push_bind(mod_id);
    query.build().execute(pool()).await.map_err(|e| respond_err(
        Status::InternalServerError,
        &format!("Could not update mod: {e}"))
    )?;

    respond_ok_empty()
}


#[delete("/mod", data = "<data>")]
pub async fn api_delete_mod(content_type: &ContentType, data: Data<'_>) -> RespType {
    let err_400 = |e: String| respond_err(Status::BadRequest, &e);
    info!("Handling `DELETE` mod");
    
    let form_options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
        MultipartFormDataField::text("username"),
        MultipartFormDataField::text("access_token"),
        MultipartFormDataField::text("mod_id"),
    ]);
    let form_data: MultipartFormData = MultipartFormData::parse(content_type, data, form_options).await
        .map_err(|e| format!("Could not parse form data: {e}")).map_err(err_400)?;

    let username: &String = get_text_form_field(&form_data, "username").map_err(err_400)?;
    let access_token: &String = get_text_form_field(&form_data, "access_token").map_err(err_400)?;
    ensure_account_authentication(username, access_token).await?;

    let mod_id: &String = get_text_form_field(&form_data, "mod_id").map_err(err_400)?;
    let mod_id: Uuid = Uuid::from_str(mod_id).map_err(|e| format!("Invalid Mod UUID: {e}")).map_err(err_400)?;
    ensure_mod_authorization(mod_id, username).await?;

    sqlx::query!(
        r#"
        DELETE FROM mods
        WHERE id = $1
        "#,
        mod_id,
    )
        .execute(pool())
        .await
        .map_err(|e| respond_err(Status::InternalServerError, &format!("Could not delete mod: {e}")))?;

    respond_ok_empty()
}

pub fn get_text_form_field<'a>(form_data: &'a MultipartFormData, field_name: &str) -> Result<&'a String, String> {
    form_data.texts.get(field_name)
        .and_then(|i| i.get(0))
        .map(|field| &field.text)
        .ok_or_else(|| format!("Text field `{field_name}` missing from request form!"))
}

pub fn get_text_form_field_opt<'a>(form_data: &'a MultipartFormData, field_name: &str) -> Option<&'a String> {
    form_data.texts.get(field_name)
        .and_then(|i| i.get(0))
        .map(|field| &field.text)
}

pub fn get_bytes_form_field<'a>(form_data: &'a MultipartFormData, field_name: &str) -> Result<&'a Vec<u8>, String> {
    form_data.raw.get(field_name)
        .and_then(|i| i.get(0))
        .map(|field| &field.raw)
        .ok_or_else(|| format!("Raw field `{field_name}` missing from request form!"))
}

pub fn get_bytes_form_field_opt<'a>(form_data: &'a MultipartFormData, field_name: &str) -> Option<&'a Vec<u8>> {
    form_data.raw.get(field_name)
        .and_then(|i| i.get(0))
        .map(|field| &field.raw)
}


async fn ensure_mod_authorization(mod_id: Uuid, username: &str) -> RespType {
    let exists: bool = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM mods
            WHERE id = $1 AND author = $2
        )
        "#,
        mod_id,
        username
    )
        .fetch_one(pool())
        .await
        .map_err(|e| respond_err(Status::InternalServerError, &format!("Could not verify mod ownership: {e}")))?
        .ok_or_else(|| respond_err(Status::InternalServerError, "The specified mod or user does not exist."))?;

    if !exists {
        return Err(respond_err(Status::Forbidden, "Unauthorized; you do not have permission to modify this mod"))
    }
    respond_ok_empty()
}

