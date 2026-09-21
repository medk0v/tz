//! Validated profile-avatar storage and public delivery of small public images.

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{AppState, error::AppError};

pub const MAX_AVATAR_BYTES: usize = 2 * 1_024 * 1_024;
const MAX_AVATAR_DIMENSION: u32 = 4_096;
const MAX_AVATAR_PIXELS: u64 = 16_777_216;

pub fn router() -> Router<AppState> {
    Router::new().route("/public/v1/avatars/{avatar_id}", get(get_avatar))
}

#[derive(Debug)]
pub(crate) struct ValidatedAvatar {
    pub(crate) public_id: Uuid,
    pub(crate) media_type: &'static str,
    pub(crate) content: Vec<u8>,
    pub(crate) sha256: Vec<u8>,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl ValidatedAvatar {
    pub(crate) fn from_bytes(bytes: Bytes) -> Result<Self, AppError> {
        Self::from_image_bytes(bytes, "avatar")
    }

    /// Validates any small public raster image under the avatar limits;
    /// `subject` names the image in error messages.
    pub(crate) fn from_image_bytes(bytes: Bytes, subject: &str) -> Result<Self, AppError> {
        if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
            return Err(AppError::BadRequest(format!(
                "{subject} must contain between 1 and {MAX_AVATAR_BYTES} bytes"
            )));
        }

        let (media_type, width, height) = image_metadata(&bytes).ok_or_else(|| {
            AppError::BadRequest(format!(
                "{subject} must be a valid JPEG, PNG, or WebP image"
            ))
        })?;
        if width == 0
            || height == 0
            || width > MAX_AVATAR_DIMENSION
            || height > MAX_AVATAR_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_AVATAR_PIXELS
        {
            return Err(AppError::BadRequest(format!(
                "{subject} dimensions must be between 1 and {MAX_AVATAR_DIMENSION} pixels"
            )));
        }

        let sha256 = Sha256::digest(&bytes).to_vec();
        Ok(Self {
            public_id: Uuid::now_v7(),
            media_type,
            content: bytes.to_vec(),
            sha256,
            width: i32::try_from(width)
                .map_err(|error| AppError::internal(anyhow::Error::new(error)))?,
            height: i32::try_from(height)
                .map_err(|error| AppError::internal(anyhow::Error::new(error)))?,
        })
    }
}

pub(crate) fn public_avatar_url(public_id: Uuid) -> String {
    format!("/public/v1/avatars/{public_id}")
}

#[derive(Debug, FromRow)]
struct AvatarRow {
    media_type: String,
    content: Vec<u8>,
    sha256: Vec<u8>,
}

async fn get_avatar(
    State(state): State<AppState>,
    Path(avatar_id): Path<Uuid>,
    request_headers: HeaderMap,
) -> Result<Response, AppError> {
    let avatar = sqlx::query_as::<_, AvatarRow>(
        r#"
        SELECT media_type, content, sha256
        FROM operator_profile_avatars
        WHERE public_id = $1
        UNION ALL
        SELECT media_type, content, sha256
        FROM ai_profile_avatars
        WHERE public_id = $1
        UNION ALL
        SELECT media_type, content, sha256
        FROM ai_profile_public_identity_avatars
        WHERE public_id = $1
        LIMIT 1
        "#,
    )
    .bind(avatar_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    public_image_response(
        &request_headers,
        &avatar.media_type,
        &avatar.sha256,
        avatar.content,
    )
}

/// Serves a stored image under an immutable public id, answering
/// revalidation from its content hash.
pub(crate) fn public_image_response(
    request_headers: &HeaderMap,
    media_type: &str,
    sha256: &[u8],
    content: Vec<u8>,
) -> Result<Response, AppError> {
    let etag = format!("\"{}\"", URL_SAFE_NO_PAD.encode(sha256));
    if request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag))
    {
        return avatar_response(StatusCode::NOT_MODIFIED, media_type, &etag, Body::empty());
    }

    avatar_response(StatusCode::OK, media_type, &etag, Body::from(content))
}

fn avatar_response(
    status: StatusCode,
    media_type: &str,
    etag: &str,
    body: Body,
) -> Result<Response, AppError> {
    let content_type = match media_type {
        "image/jpeg" => HeaderValue::from_static("image/jpeg"),
        "image/png" => HeaderValue::from_static("image/png"),
        "image/webp" => HeaderValue::from_static("image/webp"),
        _ => {
            return Err(AppError::internal(anyhow::anyhow!(
                "unsupported stored avatar media type"
            )));
        }
    };
    let etag = HeaderValue::from_str(etag)
        .map_err(|error| AppError::internal(anyhow::Error::new(error)))?;
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    headers.insert(header::ETAG, etag);
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("cross-origin"),
    );
    Ok(response)
}

pub(crate) fn image_metadata(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    png_dimensions(bytes)
        .map(|(width, height)| ("image/png", width, height))
        .or_else(|| jpeg_dimensions(bytes).map(|(width, height)| ("image/jpeg", width, height)))
        .or_else(|| webp_dimensions(bytes).map(|(width, height)| ("image/webp", width, height)))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    ))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 12 || !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        return None;
    }
    let mut cursor = 2;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != 0xff {
            cursor += 1;
            continue;
        }
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        let marker = *bytes.get(cursor)?;
        cursor += 1;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if marker == 0x01 || marker == 0xd8 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(u16::from_be_bytes([
            *bytes.get(cursor)?,
            *bytes.get(cursor + 1)?,
        ]));
        if length < 2 || cursor + length > bytes.len() {
            return None;
        }
        if is_jpeg_start_of_frame(marker) {
            if length < 7 {
                return None;
            }
            let height = u32::from(u16::from_be_bytes([bytes[cursor + 3], bytes[cursor + 4]]));
            let width = u32::from(u16::from_be_bytes([bytes[cursor + 5], bytes[cursor + 6]]));
            return Some((width, height));
        }
        cursor += length;
    }
    None
}

fn is_jpeg_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
    )
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }
    match &bytes[12..16] {
        b"VP8X" => Some((
            1 + little_endian_u24(&bytes[24..27]),
            1 + little_endian_u24(&bytes[27..30]),
        )),
        b"VP8L" if bytes[20] == 0x2f => {
            let width = 1 + u32::from(bytes[21]) + (u32::from(bytes[22] & 0x3f) << 8);
            let height = 1
                + u32::from(bytes[22] >> 6)
                + (u32::from(bytes[23]) << 2)
                + (u32::from(bytes[24] & 0x0f) << 10);
            Some((width, height))
        }
        b"VP8 " if bytes[23..26] == [0x9d, 0x01, 0x2a] => Some((
            u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff),
            u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff),
        )),
        _ => None,
    }
}

fn little_endian_u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;

    use super::ValidatedAvatar;

    #[test]
    fn accepts_supported_images_and_reads_dimensions() {
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&64_u32.to_be_bytes());
        png.extend_from_slice(&48_u32.to_be_bytes());
        let avatar = ValidatedAvatar::from_bytes(Bytes::from(png)).unwrap();
        assert_eq!(avatar.media_type, "image/png");
        assert_eq!((avatar.width, avatar.height), (64, 48));

        let jpeg = Bytes::from_static(&[
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x30, 0x00, 0x40, 0x01, 0x01, 0x11,
            0x00, 0xff, 0xd9,
        ]);
        let avatar = ValidatedAvatar::from_bytes(jpeg).unwrap();
        assert_eq!(avatar.media_type, "image/jpeg");
        assert_eq!((avatar.width, avatar.height), (64, 48));
    }

    #[test]
    fn rejects_active_or_excessive_image_inputs() {
        assert!(ValidatedAvatar::from_bytes(Bytes::from_static(b"<svg></svg>")).is_err());

        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&5_000_u32.to_be_bytes());
        png.extend_from_slice(&48_u32.to_be_bytes());
        assert!(ValidatedAvatar::from_bytes(Bytes::from(png)).is_err());
    }
}
