use std::str::FromStr;
use chrono::{DateTime, Utc};
use rocket::Data;
use rocket::data::{ByteUnit, DataStream};
use rocket::http::{ContentType, Status};
use rocket::serde::Deserialize;
use rocket_multipart_form_data::{MultipartFormData, MultipartFormDataField, MultipartFormDataOptions};
use tokio::io::AsyncReadExt;
use uuid::{Error, Uuid};
use crate::{pool, respond_err, respond_ok_empty, respond_ok_value, RespType};
use crate::accounts::{check_account_auth, AcornAccessToken};

// #[derive(Deserialize)]
// #[serde(crate = "rocket::serde")]
// struct UploadModRequest<'a> {
//     username: String,
//     acorn_access_token: String,
//     mod_variant_id: Option<String>,
//     file_data: Data<'a>,
// }

#[post("/upload_mod", data = "<data>")]
pub async fn api_upload_mod_file(content_type: &ContentType, data: Data<'_>) -> RespType {
    const MAX_SIZE: u64 = 16 * 1024 * 1024;   // 16 MB
    info!("Handling `POST upload_mod`");

    // Check MIME type
    if !content_type.is_binary() {
        warn!("Unsupported content type: {content_type}");
        return Err(respond_err(Status::BadRequest, &format!("Unsupported content type {}", content_type)));
    }

    let form_options = MultipartFormDataOptions::with_multipart_form_data_fields(vec![
        MultipartFormDataField::raw("file_data").size_limit(MAX_SIZE),
        MultipartFormDataField::text("username"),
        MultipartFormDataField::text("access_token"),
        MultipartFormDataField::text("mod_variant_id"),
        MultipartFormDataField::text("mod_id"),
    ]);
    let form_data: MultipartFormData = MultipartFormData::parse(content_type, data, form_options).await
        .map_err(|e| respond_err(Status::BadRequest, &format!("Could not parse form data: {e}")))?;

    let file_data: &Vec<u8> = get_bytes_form_field(&form_data, "file_data")
        .map_err(|e| respond_err(Status::BadRequest, &e))?;
    let username: &String = get_text_form_field(&form_data, "username")
        .map_err(|e| respond_err(Status::BadRequest, &e))?;
    let access_token: &String = get_text_form_field(&form_data, "access_token")
        .map_err(|e| respond_err(Status::BadRequest, &e))?;
    let mod_variant_id: Option<&String> = get_text_form_field_optional(&form_data, "mod_variant_id");
    let mod_id: Option<&String> = get_text_form_field_optional(&form_data, "mod_id");   // only needs to be set if mod_variant_id ISN'T set

    let authorized: bool =  check_account_auth(&username, &access_token).await
        .map_err(|e| respond_err(Status::InternalServerError, &e))?;
    if !authorized {
        return Err(respond_err(Status::Unauthorized, "Not authorized; invalid username or access token."))
    }

    if let Some(mod_variant_id) = mod_variant_id {
        // update existing mod variant
        let mod_variant_id: Uuid = Uuid::from_str(&mod_variant_id)
            .map_err(|e| respond_err(Status::BadRequest, &format!("Could not parse mod uuid \"{}\": {e}", mod_variant_id)))?;

        let authenticated = check_mod_variant_ownership(mod_variant_id, &username).await
            .map_err(|e| respond_err(Status::InternalServerError, &e))?;
        if !authenticated {
            return Err(respond_err(Status::Forbidden, "Not authenticated; you do not have permission to edit this mod."))
        }

        update_mod_variant(mod_variant_id, file_data).await
            .map_err(|e| respond_err(Status::InternalServerError, &format!("Could not update mod variant: {e}")))?;

    } else {
        // create new mod variant
        let mod_id: Uuid = Uuid::from_str(mod_id
            .ok_or_else(|| respond_err(Status::BadRequest, "Mod ID needs to be set if Mod Variant ID isn't set."))?
        ).map_err(|e| respond_err(Status::BadRequest, &format!("Mod ID needs to be a valid UUID: {e}")))?;
        insert_mod_variant(mod_id, file_data).await
            .map_err(|e| respond_err(Status::InternalServerError, &e))?;
    }


    respond_ok_empty()
}


pub fn get_text_form_field<'a>(form_data: &'a MultipartFormData, field_name: &str) -> Result<&'a String, String> {
    form_data.texts.get(field_name)
        .and_then(|i| i.get(0))
        .map(|field| &field.text)
        .ok_or_else(|| format!("Text field `{field_name}` missing from request form!"))
}

pub fn get_text_form_field_optional<'a>(form_data: &'a MultipartFormData, field_name: &str) -> Option<&'a String> {
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


async fn check_mod_variant_ownership(mod_variant_id: Uuid, username: &str) -> Result<bool, String> {
    let mod_id: Uuid = get_mod_id_of_variant(mod_variant_id).await?;

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
        .map_err(|e| format!("Could not insert mod file: {e}"))?
        .ok_or_else(|| "The specified mod or user does not exist.")?;

    Ok(exists)
}


async fn get_mod_id_of_variant(mod_variant_id: Uuid) -> Result<Uuid, String> {
    let record = sqlx::query!(
        r#"
        SELECT mod_id
        FROM mod_variants
        WHERE id = $1
        "#,
        mod_variant_id,
    )
        .fetch_one(pool())
        .await
        .map_err(|e|format!("Could not fetch mod id for mod variant id: {e}"))?;

    Ok(record.mod_id)
}


async fn update_mod_variant(mod_variant_id: Uuid, file_data: &Vec<u8>) -> Result<(), String> {
    let mod_version: i32 = mod_file_get_current_version(mod_variant_id).await?;

    sqlx::query!(
        r#"
        UPDATE mod_variants
        SET version = $1,
            file_data = $2
        "#,
        mod_version + 1,
        file_data,
    )
        .execute(pool())
        .await
        .map_err(|e| {
            error!("Could not fetch mod id for mod variant id: {e}");
            format!("Could not fetch mod id for mod variant id: {e}")
        })?;

    Ok(())
}


async fn insert_mod_variant(mod_id: Uuid, file_data: &Vec<u8>) -> Result<(), String> {
    let id: Uuid = Uuid::new_v4();
    let created_at: DateTime<Utc> = Utc::now();
    let last_updated_at: DateTime<Utc> = Utc::now();
    let file: DateTime<Utc> = Utc::now();
    let version: i32 = 1;

    sqlx::query!(
        r#"
        INSERT INTO mod_variants (id, created_at, last_updated_at, mod_id, file_data, version)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id,
        created_at,
        last_updated_at,
        mod_id,
        file_data,
        version,
    )
        .execute(pool())
        .await
        .map_err(|e| {
            error!("Could not insert mod variant for mod uuid {mod_id}: {e}");
            format!("Could not insert mod variant for mod uuid {mod_id}: {e}")
        })?;

    Ok(())
}


async fn mod_file_get_current_version(mod_variant_id: Uuid) -> Result<i32, String> {
    let record = sqlx::query!(
        r#"
        SELECT version
        FROM mod_variants
        WHERE id = $1
        "#,
        mod_variant_id,
    )
        .fetch_one(pool())
        .await
        .map_err(|e| format!("Could not fetch mod id for mod variant id: {e}"))?;

    Ok(record.version)
}

