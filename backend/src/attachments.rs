//! Quarantined chat attachments with fail-closed malware scanning.

mod task_files;

use std::{
    collections::HashSet,
    io::{ErrorKind, SeekFrom},
    net::SocketAddr,
    path::{Path as FilePath, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore},
    time::timeout,
};
use tokio_util::io::ReaderStream;
use tracing::warn;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{ActorContext, WidgetSessionContext},
    config::{AttachmentConfig, AttachmentScanMode},
    conversations::{self, MessageResponse},
    error::AppError,
};

pub const MAX_IMAGE_BYTES: usize = 10 * 1_024 * 1_024;
pub const MAX_PDF_BYTES: usize = 20 * 1_024 * 1_024;
pub const MAX_FILE_BYTES: usize = 20 * 1_024 * 1_024;
pub const MAX_VIDEO_BYTES: usize = 50 * 1_024 * 1_024;
pub const MAX_AUDIO_BYTES: usize = 20 * 1_024 * 1_024;
const MULTIPART_OVERHEAD_BYTES: usize = 64 * 1_024;
pub(crate) const MAX_UPLOAD_REQUEST_BYTES: usize = MAX_VIDEO_BYTES + MULTIPART_OVERHEAD_BYTES;
const UPLOAD_RECEIVE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_IMAGE_DIMENSION: u32 = 8_192;
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const INSPECTION_BYTES: usize = 8 * 1_024;
const CLAMAV_CHUNK_BYTES: usize = 64 * 1_024;
const MAX_CLAMAV_RESPONSE_BYTES: u64 = 4 * 1_024;
const PURGE_BATCH_SIZE: i64 = 100;
const STORAGE_RECONCILE_BATCH_SIZE: usize = 100;
const STALE_STORAGE_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/widget/v1/conversations/{conversation_id}/attachments",
            post(upload_widget_attachment).layer(DefaultBodyLimit::max(MAX_UPLOAD_REQUEST_BYTES)),
        )
        .route(
            "/api/v1/conversations/{conversation_id}/attachments",
            post(upload_operator_attachment).layer(DefaultBodyLimit::max(MAX_UPLOAD_REQUEST_BYTES)),
        )
        .route(
            "/widget/v1/attachments/{attachment_id}",
            get(download_widget_attachment),
        )
        .route(
            "/api/v1/attachments/{attachment_id}",
            get(download_operator_attachment),
        )
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AttachmentResponse {
    pub id: Uuid,
    pub file_name: String,
    pub content_type: String,
    pub byte_size: i64,
    pub available: bool,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub(crate) struct PreparedAttachment {
    pub id: Uuid,
    pub file_name: String,
    pub content_type: &'static str,
    pub byte_size: i64,
    pub checksum_sha256: String,
    path: PathBuf,
    published: bool,
    _quarantine: QuarantinedFile,
    upload_permit: Option<OwnedSemaphorePermit>,
}

/// Owns the temporary name through validation, multipart parsing, and cancellation.
#[derive(Debug)]
struct QuarantinedFile {
    path: PathBuf,
}

impl Drop for QuarantinedFile {
    fn drop(&mut self) {
        // Drop must unlink synchronously: an async cleanup task can itself be cancelled.
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != ErrorKind::NotFound
        {
            warn!(?error, path = %self.path.display(), "attachment cleanup failed");
        }
    }
}

/// Private filesystem roots and bounded scanner capacity shared by API requests.
#[derive(Debug)]
pub struct AttachmentStorage {
    enabled: bool,
    quarantine_path: PathBuf,
    object_path: PathBuf,
    scanner: Option<ClamAvScanner>,
    uploads: Arc<Semaphore>,
}

impl AttachmentStorage {
    /// Prepares private storage directories when the feature is explicitly enabled.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured storage directories cannot be created or secured.
    pub async fn prepare(config: &AttachmentConfig) -> Result<Self> {
        let quarantine_path = config.storage_path.join("quarantine");
        let object_path = config.storage_path.join("objects");
        if config.enabled {
            create_private_directory(&config.storage_path).await?;
            create_private_directory(&quarantine_path).await?;
            create_private_directory(&object_path).await?;
        }
        let scanner = if config.enabled {
            match (config.scan_mode, config.clamav_address) {
                (AttachmentScanMode::Required, Some(address)) => {
                    Some(ClamAvScanner::new(address, config.scan_timeout()))
                }
                (AttachmentScanMode::Disabled, None) => {
                    warn!("attachment malware scanning is disabled by configuration");
                    None
                }
                (AttachmentScanMode::Required, None) => {
                    anyhow::bail!("attachment malware scanning requires a scanner address");
                }
                (AttachmentScanMode::Disabled, Some(_)) => {
                    anyhow::bail!("attachment scanner address is set while scanning is disabled");
                }
            }
        } else {
            None
        };
        Ok(Self {
            enabled: config.enabled,
            quarantine_path,
            object_path,
            scanner,
            uploads: Arc::new(Semaphore::new(config.max_concurrent_uploads)),
        })
    }

    pub(crate) async fn receive(
        &self,
        multipart: Multipart,
    ) -> Result<PreparedAttachment, AppError> {
        self.receive_with_policy(multipart, UploadPolicy::Chat)
            .await
    }

    pub(crate) async fn receive_task(
        &self,
        multipart: Multipart,
    ) -> Result<PreparedAttachment, AppError> {
        self.receive_with_policy(multipart, UploadPolicy::Task)
            .await
    }

    async fn receive_with_policy(
        &self,
        multipart: Multipart,
        policy: UploadPolicy,
    ) -> Result<PreparedAttachment, AppError> {
        self.require_enabled()?;
        let permit = self.acquire_upload_permit()?;
        timeout(
            UPLOAD_RECEIVE_TIMEOUT,
            self.receive_inner(multipart, permit, policy),
        )
        .await
        .map_err(|_| AppError::BadRequest("file upload exceeded the time limit".to_owned()))?
    }

    async fn receive_inner(
        &self,
        mut multipart: Multipart,
        permit: OwnedSemaphorePermit,
        policy: UploadPolicy,
    ) -> Result<PreparedAttachment, AppError> {
        let field = multipart
            .next_field()
            .await
            .map_err(|_| AppError::BadRequest("invalid multipart upload".to_owned()))?
            .ok_or_else(|| AppError::BadRequest("one file field is required".to_owned()))?;
        if field.name() != Some("file") {
            return Err(AppError::BadRequest(
                "the multipart field must be named file".to_owned(),
            ));
        }
        let file_name = field
            .file_name()
            .ok_or_else(|| AppError::BadRequest("file name is required".to_owned()))?
            .to_owned();
        let claimed_content_type = field
            .content_type()
            .or(matches!(policy, UploadPolicy::Task).then_some(""))
            .ok_or_else(|| {
                AppError::UnsupportedMediaType("file Content-Type is required".to_owned())
            })?
            .to_owned();
        let (file_name, expected_media) = match policy {
            UploadPolicy::Chat => validate_file_metadata(&file_name, &claimed_content_type)?,
            UploadPolicy::Task => validate_task_file_metadata(&file_name, &claimed_content_type)?,
        };
        let prepared = self
            .receive_field(field, file_name, expected_media, permit)
            .await?;
        if multipart
            .next_field()
            .await
            .map_err(|_| AppError::BadRequest("invalid multipart upload".to_owned()))?
            .is_some()
        {
            self.discard(&prepared).await;
            return Err(AppError::BadRequest(
                "exactly one file is allowed per upload".to_owned(),
            ));
        }
        Ok(prepared)
    }

    async fn receive_field(
        &self,
        mut field: axum::extract::multipart::Field<'_>,
        file_name: String,
        expected_media: ExpectedMedia,
        permit: OwnedSemaphorePermit,
    ) -> Result<PreparedAttachment, AppError> {
        let id = Uuid::now_v7();
        let path = self.quarantine_path.join(format!("{id}.upload"));
        let result = async {
            let (mut file, quarantine) = create_private_file(&path)?;
            let mut digest = Sha256::new();
            let mut first = Vec::with_capacity(INSPECTION_BYTES);
            let mut tail = Vec::with_capacity(INSPECTION_BYTES);
            let mut byte_size = 0_usize;

            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| AppError::BadRequest("file upload was interrupted".to_owned()))?
            {
                byte_size = byte_size.checked_add(chunk.len()).ok_or_else(|| {
                    AppError::PayloadTooLarge("file size exceeds the supported limit".to_owned())
                })?;
                if byte_size > expected_media.max_bytes() {
                    return Err(AppError::PayloadTooLarge(format!(
                        "{} files cannot exceed {} bytes",
                        expected_media.label(),
                        expected_media.max_bytes()
                    )));
                }
                append_inspection_bytes(&mut first, &mut tail, &chunk);
                digest.update(&chunk);
                file.write_all(&chunk).await.map_err(AppError::internal)?;
            }
            if byte_size == 0 {
                return Err(AppError::BadRequest("file cannot be empty".to_owned()));
            }
            file.sync_all().await.map_err(AppError::internal)?;
            drop(file);

            validate_file_content(&path, expected_media, &first, &tail, byte_size).await?;
            if let Some(scanner) = self.scanner.as_ref() {
                scanner.scan(&path).await?;
            }
            let byte_size = i64::try_from(byte_size).map_err(AppError::internal)?;
            Ok(PreparedAttachment {
                id,
                file_name,
                content_type: expected_media.content_type(),
                byte_size,
                checksum_sha256: URL_SAFE_NO_PAD.encode(digest.finalize()),
                path: path.clone(),
                published: false,
                _quarantine: quarantine,
                upload_permit: Some(permit),
            })
        }
        .await;

        if result.is_err() {
            remove_file_if_present(&path).await;
        }
        result
    }

    pub(crate) async fn publish(&self, prepared: &mut PreparedAttachment) -> Result<(), AppError> {
        let destination = self.object_path.join(prepared.id.to_string());
        fs::hard_link(&prepared.path, &destination)
            .await
            .map_err(AppError::internal)?;
        if let Err(error) = make_object_read_only(&destination).await {
            remove_file_if_present(&destination).await;
            return Err(AppError::internal(error));
        }
        if let Err(error) = fs::remove_file(&prepared.path).await {
            remove_file_if_present(&destination).await;
            return Err(AppError::internal(error));
        }
        prepared.path = destination;
        prepared.published = true;
        Ok(())
    }

    pub(crate) async fn discard(&self, prepared: &PreparedAttachment) {
        remove_file_if_present(&prepared.path).await;
    }

    pub(crate) const fn is_published(prepared: &PreparedAttachment) -> bool {
        prepared.published
    }

    pub(crate) fn release_upload_permit(prepared: &mut PreparedAttachment) {
        prepared.upload_permit.take();
    }

    fn object_file_path(&self, attachment_id: Uuid) -> PathBuf {
        self.object_path.join(attachment_id.to_string())
    }

    fn require_enabled(&self) -> Result<(), AppError> {
        if self.enabled {
            Ok(())
        } else {
            Err(AppError::ServiceUnavailable(
                "secure file uploads are not enabled".to_owned(),
            ))
        }
    }

    fn acquire_upload_permit(&self) -> Result<OwnedSemaphorePermit, AppError> {
        Arc::clone(&self.uploads)
            .try_acquire_owned()
            .map_err(|_| AppError::TooManyRequests)
    }
}

#[derive(Clone, Copy)]
enum UploadPolicy {
    Chat,
    Task,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectedMedia {
    Jpeg,
    Png,
    WebP,
    Pdf,
    Mp4,
    WebM,
    Business(task_files::BusinessFile),
}

impl ExpectedMedia {
    const fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::WebP => "image/webp",
            Self::Pdf => "application/pdf",
            Self::Mp4 => "video/mp4",
            Self::WebM => "video/webm",
            Self::Business(_) => "application/octet-stream",
        }
    }

    const fn max_bytes(self) -> usize {
        match self {
            Self::Jpeg | Self::Png | Self::WebP => MAX_IMAGE_BYTES,
            Self::Pdf => MAX_PDF_BYTES,
            Self::Mp4 | Self::WebM => MAX_VIDEO_BYTES,
            Self::Business(_) => MAX_FILE_BYTES,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Jpeg | Self::Png | Self::WebP => "image",
            Self::Pdf => "PDF",
            Self::Mp4 | Self::WebM => "video",
            Self::Business(_) => "file",
        }
    }
}

fn validate_file_name(raw_file_name: &str) -> Result<String, AppError> {
    let file_name = raw_file_name.trim();
    if file_name.is_empty()
        || file_name.chars().count() > 200
        || file_name == "."
        || file_name == ".."
        || file_name.chars().any(is_dangerous_filename_character)
    {
        return Err(AppError::BadRequest("file name is not allowed".to_owned()));
    }
    Ok(file_name.to_owned())
}

fn validate_file_metadata(
    raw_file_name: &str,
    content_type: &str,
) -> Result<(String, ExpectedMedia), AppError> {
    let file_name = validate_file_name(raw_file_name)?;
    let extension = FilePath::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| AppError::UnsupportedMediaType("file extension is required".to_owned()))?;
    let expected = match (content_type, extension.as_str()) {
        ("image/jpeg", "jpg" | "jpeg") => ExpectedMedia::Jpeg,
        ("image/png", "png") => ExpectedMedia::Png,
        ("image/webp", "webp") => ExpectedMedia::WebP,
        ("application/pdf", "pdf") => ExpectedMedia::Pdf,
        ("video/mp4", "mp4") => ExpectedMedia::Mp4,
        ("video/webm", "webm") => ExpectedMedia::WebM,
        _ => {
            return Err(AppError::UnsupportedMediaType(
                "allowed files are JPEG, PNG, WebP, PDF, MP4, and WebM with matching extensions"
                    .to_owned(),
            ));
        }
    };
    Ok((file_name, expected))
}

fn validate_task_file_metadata(
    raw_file_name: &str,
    content_type: &str,
) -> Result<(String, ExpectedMedia), AppError> {
    let file_name = validate_file_name(raw_file_name)?;
    let extension = FilePath::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    let expected = match extension.as_deref() {
        Some("jpg" | "jpeg") => ExpectedMedia::Jpeg,
        Some("png") => ExpectedMedia::Png,
        Some("webp") => ExpectedMedia::WebP,
        Some("pdf") => ExpectedMedia::Pdf,
        Some("mp4") => ExpectedMedia::Mp4,
        Some("webm") => ExpectedMedia::WebM,
        _ => ExpectedMedia::Business(
            extension
                .as_deref()
                .and_then(task_files::BusinessFile::from_extension)
                .ok_or_else(|| {
                    AppError::UnsupportedMediaType(
                        "file format is not allowed for task attachments".to_owned(),
                    )
                })?,
        ),
    };
    let matches_mime = match expected {
        ExpectedMedia::Business(kind) => kind.accepts_mime(content_type),
        _ => content_type == expected.content_type(),
    };
    if !content_type.is_empty() && content_type != "application/octet-stream" && !matches_mime {
        return Err(AppError::UnsupportedMediaType(
            "file type does not match its extension".to_owned(),
        ));
    }
    Ok((file_name, expected))
}

fn is_dangerous_filename_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '/' | '\\' | ':'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
        )
}

async fn validate_file_content(
    path: &FilePath,
    expected: ExpectedMedia,
    first: &[u8],
    tail: &[u8],
    byte_size: usize,
) -> Result<(), AppError> {
    let valid = match expected {
        ExpectedMedia::Jpeg | ExpectedMedia::Png | ExpectedMedia::WebP => {
            let bytes = fs::read(path).await.map_err(AppError::internal)?;
            validate_image(&bytes, expected)
        }
        ExpectedMedia::Pdf => validate_pdf(first, tail),
        ExpectedMedia::Mp4 => validate_mp4(first, byte_size),
        ExpectedMedia::WebM => validate_webm(first),
        ExpectedMedia::Business(kind) => {
            task_files::validate(path, kind).await?;
            true
        }
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::UnsupportedMediaType(
            "file contents do not match the declared safe file type".to_owned(),
        ))
    }
}

fn validate_image(bytes: &[u8], expected: ExpectedMedia) -> bool {
    let Some((content_type, width, height)) = crate::avatars::image_metadata(bytes) else {
        return false;
    };
    if content_type != expected.content_type()
        || width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return false;
    }
    match expected {
        ExpectedMedia::Png => bytes.ends_with(b"\x00\x00\x00\x00IEND\xaeB\x60\x82"),
        ExpectedMedia::WebP => bytes
            .get(4..8)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|declared| u64::from(declared) + 8 == bytes.len() as u64),
        ExpectedMedia::Jpeg => bytes.ends_with(&[0xff, 0xd9]),
        ExpectedMedia::Pdf
        | ExpectedMedia::Mp4
        | ExpectedMedia::WebM
        | ExpectedMedia::Business(_) => false,
    }
}

fn validate_pdf(first: &[u8], tail: &[u8]) -> bool {
    (first.starts_with(b"%PDF-1.") || first.starts_with(b"%PDF-2."))
        && trim_ascii_whitespace_end(tail).ends_with(b"%%EOF")
}

fn validate_mp4(first: &[u8], byte_size: usize) -> bool {
    if first.len() < 12 || &first[4..8] != b"ftyp" {
        return false;
    }
    let declared = u32::from_be_bytes(first[0..4].try_into().unwrap_or_default());
    declared >= 8 && u64::from(declared) <= byte_size as u64
}

fn validate_webm(first: &[u8]) -> bool {
    first.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) && first.windows(4).any(|window| window == b"webm")
}

fn trim_ascii_whitespace_end(mut value: &[u8]) -> &[u8] {
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

fn append_inspection_bytes(first: &mut Vec<u8>, tail: &mut Vec<u8>, chunk: &[u8]) {
    if first.len() < INSPECTION_BYTES {
        let remaining = INSPECTION_BYTES - first.len();
        first.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    if chunk.len() >= INSPECTION_BYTES {
        tail.clear();
        tail.extend_from_slice(&chunk[chunk.len() - INSPECTION_BYTES..]);
        return;
    }
    let overflow = tail
        .len()
        .saturating_add(chunk.len())
        .saturating_sub(INSPECTION_BYTES);
    if overflow > 0 {
        tail.drain(..overflow);
    }
    tail.extend_from_slice(chunk);
}

#[derive(Debug)]
struct ClamAvScanner {
    address: SocketAddr,
    timeout: Duration,
}

impl ClamAvScanner {
    const fn new(address: SocketAddr, timeout: Duration) -> Self {
        Self { address, timeout }
    }

    async fn scan(&self, path: &FilePath) -> Result<(), AppError> {
        let result = timeout(self.timeout, self.scan_inner(path))
            .await
            .map_err(|_| scan_unavailable())?;
        let response = result.map_err(|_| scan_unavailable())?;
        parse_clamav_response(&response)
    }

    async fn scan_inner(&self, path: &FilePath) -> Result<Vec<u8>> {
        let mut socket = TcpStream::connect(self.address)
            .await
            .with_context(|| format!("failed to connect to ClamAV at {}", self.address))?;
        socket.write_all(b"zINSTREAM\0").await?;
        let mut file = File::open(path).await?;
        let mut buffer = vec![0_u8; CLAMAV_CHUNK_BYTES];
        loop {
            let count = file.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            let count = u32::try_from(count).context("ClamAV chunk size overflow")?;
            socket.write_all(&count.to_be_bytes()).await?;
            socket.write_all(&buffer[..count as usize]).await?;
        }
        socket.write_all(&0_u32.to_be_bytes()).await?;
        socket.shutdown().await?;
        let mut response = Vec::new();
        socket
            .take(MAX_CLAMAV_RESPONSE_BYTES + 1)
            .read_to_end(&mut response)
            .await?;
        if response.len() as u64 > MAX_CLAMAV_RESPONSE_BYTES {
            anyhow::bail!("ClamAV response exceeded the configured bound");
        }
        Ok(response)
    }
}

fn parse_clamav_response(response: &[u8]) -> Result<(), AppError> {
    let response = response.strip_suffix(b"\0").unwrap_or(response);
    if response == b"stream: OK" {
        return Ok(());
    }
    if response.starts_with(b"stream: ") && response.ends_with(b" FOUND") {
        return Err(AppError::BadRequest(
            "file was rejected by the security scan".to_owned(),
        ));
    }
    Err(scan_unavailable())
}

fn scan_unavailable() -> AppError {
    AppError::ServiceUnavailable(
        "the file security scanner is unavailable; the upload was not stored".to_owned(),
    )
}

async fn upload_widget_attachment(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Path(conversation_id): Path<Uuid>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    let client_message_id = idempotency_key(&headers)?;
    conversations::authorize_widget_attachment_upload(&state, &session, conversation_id).await?;
    let mut prepared = state.attachment_storage.receive(multipart).await?;
    // Once processing starts, finish the database/filesystem transaction even if
    // the client disconnects. The owned upload keeps its capacity permit until then.
    tokio::spawn(async move {
        let result = conversations::create_widget_attachment_message(
            &state,
            &session,
            conversation_id,
            client_message_id,
            &mut prepared,
        )
        .await;
        match result {
            Ok(message) => {
                if !AttachmentStorage::is_published(&prepared) {
                    state.attachment_storage.discard(&prepared).await;
                }
                Ok((StatusCode::CREATED, Json(message)))
            }
            Err(error) => {
                state.attachment_storage.discard(&prepared).await;
                Err(error)
            }
        }
    })
    .await
    .map_err(AppError::internal)?
}

async fn upload_operator_attachment(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(conversation_id): Path<Uuid>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<(StatusCode, Json<MessageResponse>), AppError> {
    let client_message_id = idempotency_key(&headers)?;
    conversations::authorize_operator_attachment_upload(&state, &actor, conversation_id).await?;
    let mut prepared = state.attachment_storage.receive(multipart).await?;
    tokio::spawn(async move {
        let result = conversations::create_operator_attachment_message(
            &state,
            &actor,
            conversation_id,
            client_message_id,
            &mut prepared,
        )
        .await;
        match result {
            Ok(message) => {
                if !AttachmentStorage::is_published(&prepared) {
                    state.attachment_storage.discard(&prepared).await;
                }
                Ok((StatusCode::CREATED, Json(message)))
            }
            Err(error) => {
                state.attachment_storage.discard(&prepared).await;
                Err(error)
            }
        }
    })
    .await
    .map_err(AppError::internal)?
}

fn idempotency_key(headers: &HeaderMap) -> Result<Uuid, AppError> {
    let value = headers
        .get("idempotency-key")
        .ok_or_else(|| AppError::BadRequest("Idempotency-Key header is required".to_owned()))?
        .to_str()
        .map_err(|_| AppError::BadRequest("Idempotency-Key header is invalid".to_owned()))?;
    let key = Uuid::parse_str(value)
        .map_err(|_| AppError::BadRequest("Idempotency-Key must be a UUID".to_owned()))?;
    if key.is_nil() {
        return Err(AppError::BadRequest(
            "Idempotency-Key must not be nil".to_owned(),
        ));
    }
    Ok(key)
}

#[derive(Debug, FromRow)]
struct AttachmentFileRow {
    id: Uuid,
    project_id: Uuid,
    inbox_id: Uuid,
    content_type: String,
    byte_size: i64,
    checksum_sha256: String,
}

pub(crate) struct AttachmentObject {
    pub id: Uuid,
    pub content_type: String,
    pub byte_size: i64,
    pub checksum_sha256: String,
}

impl From<AttachmentFileRow> for AttachmentObject {
    fn from(row: AttachmentFileRow) -> Self {
        Self {
            id: row.id,
            content_type: row.content_type,
            byte_size: row.byte_size,
            checksum_sha256: row.checksum_sha256,
        }
    }
}

async fn download_widget_attachment(
    State(state): State<AppState>,
    session: WidgetSessionContext,
    Path(attachment_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let attachment = sqlx::query_as::<_, AttachmentFileRow>(
        r#"
        SELECT attachment.id, message.project_id, message.inbox_id,
               attachment.content_type, attachment.byte_size, attachment.checksum_sha256
        FROM message_attachments AS attachment
        JOIN messages AS message
          ON message.tenant_id = attachment.tenant_id
         AND message.id = attachment.message_id
        JOIN conversations AS conversation
          ON conversation.tenant_id = message.tenant_id
         AND conversation.id = message.conversation_id
        WHERE attachment.tenant_id = $1 AND attachment.id = $2
          AND message.project_id = $3 AND message.inbox_id = $4
          AND conversation.contact_id = $5
          AND attachment.scan_status = 'clean'
          AND attachment.expires_at > now() AND attachment.purged_at IS NULL
        "#,
    )
    .bind(session.tenant_id)
    .bind(attachment_id)
    .bind(session.project_id)
    .bind(session.inbox_id)
    .bind(session.contact_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    attachment_response(&state, attachment.into(), &headers).await
}

async fn download_operator_attachment(
    State(state): State<AppState>,
    actor: ActorContext,
    Path(attachment_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    actor.require("conversations:read")?;
    let attachment = sqlx::query_as::<_, AttachmentFileRow>(
        r#"
        SELECT attachment.id, message.project_id, message.inbox_id,
               attachment.content_type, attachment.byte_size, attachment.checksum_sha256
        FROM message_attachments AS attachment
        JOIN messages AS message
          ON message.tenant_id = attachment.tenant_id
         AND message.id = attachment.message_id
        WHERE attachment.tenant_id = $1 AND attachment.id = $2
          AND attachment.scan_status = 'clean'
          AND attachment.expires_at > now() AND attachment.purged_at IS NULL
        "#,
    )
    .bind(actor.tenant_id)
    .bind(attachment_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    actor.require_inbox(attachment.project_id, attachment.inbox_id)?;
    attachment_response(&state, attachment.into(), &headers).await
}

pub(crate) async fn attachment_response(
    state: &AppState,
    attachment: AttachmentObject,
    request_headers: &HeaderMap,
) -> Result<Response, AppError> {
    state.attachment_storage.require_enabled()?;
    let path = state.attachment_storage.object_file_path(attachment.id);
    let path_metadata = fs::symlink_metadata(&path).await.map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            AppError::NotFound
        } else {
            AppError::internal(error)
        }
    })?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(AppError::internal(anyhow::anyhow!(
            "stored attachment object is not a regular file"
        )));
    }
    let mut file = File::open(&path).await.map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            AppError::NotFound
        } else {
            AppError::internal(error)
        }
    })?;
    let metadata = file.metadata().await.map_err(AppError::internal)?;
    let expected_size = u64::try_from(attachment.byte_size).map_err(AppError::internal)?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(AppError::internal(anyhow::anyhow!(
            "stored attachment does not match database metadata"
        )));
    }

    let mut digest = Sha256::new();
    let mut verified_size = 0_u64;
    let mut buffer = vec![0_u8; CLAMAV_CHUNK_BYTES];
    loop {
        let count = file.read(&mut buffer).await.map_err(AppError::internal)?;
        if count == 0 {
            break;
        }
        verified_size = verified_size
            .checked_add(u64::try_from(count).map_err(AppError::internal)?)
            .ok_or_else(|| AppError::internal(anyhow::anyhow!("attachment size overflow")))?;
        digest.update(&buffer[..count]);
    }
    let verified_checksum = URL_SAFE_NO_PAD.encode(digest.finalize());
    if verified_size != expected_size || verified_checksum != attachment.checksum_sha256 {
        return Err(AppError::internal(anyhow::anyhow!(
            "stored attachment checksum does not match the scanned object"
        )));
    }

    let etag = format!("\"sha256-{}\"", attachment.checksum_sha256);
    if request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag))
    {
        return build_attachment_response(
            StatusCode::NOT_MODIFIED,
            &attachment,
            &etag,
            Body::empty(),
        );
    }
    file.seek(SeekFrom::Start(0))
        .await
        .map_err(AppError::internal)?;
    let body = Body::from_stream(ReaderStream::new(file));
    build_attachment_response(StatusCode::OK, &attachment, &etag, body)
}

fn build_attachment_response(
    status: StatusCode,
    attachment: &AttachmentObject,
    etag: &str,
    body: Body,
) -> Result<Response, AppError> {
    let (content_type, extension) = response_media(&attachment.content_type)?;
    let etag = HeaderValue::from_str(etag).map_err(AppError::internal)?;
    let disposition = HeaderValue::from_str(&format!(
        "attachment; filename=\"attachment-{}.{}\"",
        attachment.id, extension
    ))
    .map_err(AppError::internal)?;
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CONTENT_DISPOSITION, disposition);
    headers.insert(header::ETAG, etag);
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("cross-origin"),
    );
    if status == StatusCode::OK {
        headers.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&attachment.byte_size.to_string()).map_err(AppError::internal)?,
        );
    }
    Ok(response)
}

fn response_media(content_type: &str) -> Result<(HeaderValue, &'static str), AppError> {
    let value = match content_type {
        "image/jpeg" => (HeaderValue::from_static("image/jpeg"), "jpg"),
        "image/png" => (HeaderValue::from_static("image/png"), "png"),
        "image/webp" => (HeaderValue::from_static("image/webp"), "webp"),
        "application/pdf" => (HeaderValue::from_static("application/pdf"), "pdf"),
        "video/mp4" => (HeaderValue::from_static("video/mp4"), "mp4"),
        "video/webm" => (HeaderValue::from_static("video/webm"), "webm"),
        "audio/ogg" => (HeaderValue::from_static("audio/ogg"), "ogg"),
        "application/octet-stream" => (HeaderValue::from_static("application/octet-stream"), "bin"),
        _ => {
            return Err(AppError::internal(anyhow::anyhow!(
                "unsupported stored attachment media type"
            )));
        }
    };
    Ok(value)
}

#[derive(Debug, FromRow)]
struct ExpiredAttachment {
    id: Uuid,
}

/// Marks expired objects unavailable, deletes their bytes, and records completion.
///
/// # Errors
///
/// Returns an error when the database cannot be updated. Individual filesystem deletion failures
/// are logged and left eligible for the next purge pass.
pub async fn purge_expired(state: &AppState) -> Result<usize> {
    if !state.config.attachments.enabled {
        return Ok(0);
    }
    let mut transaction = state.db.begin().await?;
    let expired = sqlx::query_as::<_, ExpiredAttachment>(
        r#"
        WITH candidates AS (
            SELECT id
            FROM message_attachments
            WHERE purged_at IS NULL
              AND (scan_status = 'expired' OR (scan_status = 'clean' AND expires_at <= now()))
            ORDER BY expires_at, id
            FOR UPDATE SKIP LOCKED
            LIMIT $1
        )
        UPDATE message_attachments AS attachment
        SET scan_status = 'expired'
        FROM candidates
        WHERE attachment.id = candidates.id
        RETURNING attachment.id
        "#,
    )
    .bind(PURGE_BATCH_SIZE)
    .fetch_all(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let mut purged = 0_usize;
    for attachment in expired {
        let path = state.attachment_storage.object_file_path(attachment.id);
        match fs::remove_file(path).await {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                warn!(?error, attachment_id = %attachment.id, "expired attachment removal failed");
                continue;
            }
        }
        sqlx::query(
            "UPDATE message_attachments SET purged_at = now() WHERE id = $1 AND scan_status = 'expired'",
        )
        .bind(attachment.id)
        .execute(&state.db)
        .await?;
        purged += 1;
    }
    Ok(purged)
}

/// Removes stale upload remnants that cannot be reached through attachment metadata.
///
/// Only regular files with the exact application-owned UUID naming convention are considered,
/// and a 24-hour grace period prevents races with active uploads and database transactions.
///
/// # Errors
///
/// Returns an error when a storage directory or the database cannot be inspected.
pub async fn purge_stale_storage(state: &AppState) -> Result<usize> {
    if !state.config.attachments.enabled {
        return Ok(0);
    }
    sqlx::query("DELETE FROM ai_task_attachments WHERE task_id IS NULL AND created_at <= now()-interval '24 hours'")
        .execute(&state.db).await?;
    let cutoff = SystemTime::now()
        .checked_sub(STALE_STORAGE_FILE_AGE)
        .context("failed to calculate stale attachment cutoff")?;
    let mut removed =
        purge_stale_quarantine(&state.attachment_storage.quarantine_path, cutoff).await?;
    removed += purge_orphaned_objects(state, cutoff).await?;
    Ok(removed)
}

async fn purge_stale_quarantine(path: &FilePath, cutoff: SystemTime) -> Result<usize> {
    let mut entries = fs::read_dir(path)
        .await
        .with_context(|| format!("failed to read attachment quarantine {}", path.display()))?;
    let mut removed = 0_usize;
    while removed < STORAGE_RECONCILE_BATCH_SIZE {
        let Some(entry) = entries.next_entry().await? else {
            break;
        };
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(id) = file_name.strip_suffix(".upload") else {
            continue;
        };
        if Uuid::parse_str(id).is_err() || !is_stale_regular_file(&entry.path(), cutoff).await? {
            continue;
        }
        if remove_reconciled_file(&entry.path()).await? {
            removed += 1;
        }
    }
    Ok(removed)
}

async fn purge_orphaned_objects(state: &AppState, cutoff: SystemTime) -> Result<usize> {
    let path = &state.attachment_storage.object_path;
    let mut entries = fs::read_dir(path)
        .await
        .with_context(|| format!("failed to read attachment objects {}", path.display()))?;
    let mut removed = 0_usize;
    let mut candidates = Vec::with_capacity(STORAGE_RECONCILE_BATCH_SIZE);
    while removed < STORAGE_RECONCILE_BATCH_SIZE {
        let Some(entry) = entries.next_entry().await? else {
            break;
        };
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Ok(attachment_id) = Uuid::parse_str(file_name) else {
            continue;
        };
        if !is_stale_regular_file(&entry.path(), cutoff).await? {
            continue;
        }
        candidates.push((attachment_id, entry.path()));
        if candidates.len() == STORAGE_RECONCILE_BATCH_SIZE {
            removed +=
                purge_orphan_batch(state, &candidates, STORAGE_RECONCILE_BATCH_SIZE - removed)
                    .await?;
            candidates.clear();
        }
    }
    if removed < STORAGE_RECONCILE_BATCH_SIZE && !candidates.is_empty() {
        removed +=
            purge_orphan_batch(state, &candidates, STORAGE_RECONCILE_BATCH_SIZE - removed).await?;
    }
    Ok(removed)
}

async fn purge_orphan_batch(
    state: &AppState,
    candidates: &[(Uuid, PathBuf)],
    removal_limit: usize,
) -> Result<usize> {
    let candidate_ids = candidates.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let retained = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM message_attachments WHERE id = ANY($1) AND purged_at IS NULL UNION SELECT id FROM ai_task_attachments WHERE id = ANY($1)",
    )
    .bind(&candidate_ids)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect::<HashSet<_>>();
    let mut removed = 0_usize;
    for (id, path) in candidates {
        if removed == removal_limit {
            break;
        }
        if !retained.contains(id) && remove_reconciled_file(path).await? {
            removed += 1;
        }
    }
    Ok(removed)
}

async fn is_stale_regular_file(path: &FilePath, cutoff: SystemTime) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)
        .await
        .with_context(|| format!("failed to inspect attachment object {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(false);
    }
    Ok(metadata
        .modified()
        .with_context(|| format!("failed to read attachment timestamp {}", path.display()))?
        <= cutoff)
}

async fn remove_reconciled_file(path: &FilePath) -> Result<bool> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to remove stale attachment {}", path.display())),
    }
}

async fn create_private_directory(path: &FilePath) -> Result<()> {
    let existed = match fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!(
                    "attachment storage path must be a real directory: {}",
                    path.display()
                );
            }
            true
        }
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to inspect attachment directory {}", path.display())
            });
        }
    };
    fs::create_dir_all(path)
        .await
        .with_context(|| format!("failed to create attachment directory {}", path.display()))?;
    let metadata = fs::symlink_metadata(path)
        .await
        .with_context(|| format!("failed to inspect attachment directory {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        anyhow::bail!(
            "attachment storage path must be a real directory: {}",
            path.display()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if existed {
            if metadata.permissions().mode() & 0o077 != 0 {
                anyhow::bail!(
                    "existing attachment directory must not grant group or world access: {}",
                    path.display()
                );
            }
        } else {
            fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .await
                .with_context(|| {
                    format!("failed to secure attachment directory {}", path.display())
                })?;
        }
    }
    #[cfg(not(unix))]
    let _ = existed;
    Ok(())
}

async fn make_object_read_only(path: &FilePath) -> Result<()> {
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o400)
    };
    #[cfg(not(unix))]
    let permissions = {
        let mut permissions = fs::metadata(path).await?.permissions();
        permissions.set_readonly(true);
        permissions
    };
    fs::set_permissions(path, permissions)
        .await
        .with_context(|| {
            format!(
                "failed to make attachment object read-only: {}",
                path.display()
            )
        })
}

fn create_private_file(path: &FilePath) -> Result<(File, QuarantinedFile), AppError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // No await between creating the file and assigning its cleanup owner.
    let file = options.open(path).map_err(AppError::internal)?;
    Ok((
        File::from_std(file),
        QuarantinedFile {
            path: path.to_owned(),
        },
    ))
}

async fn remove_file_if_present(path: &FilePath) {
    if let Err(error) = fs::remove_file(path).await
        && error.kind() != ErrorKind::NotFound
    {
        warn!(?error, path = %path.display(), "attachment cleanup failed");
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, extract::FromRequest, http::Request};

    use super::{
        ExpectedMedia, append_inspection_bytes, parse_clamav_response, validate_file_metadata,
        validate_image, validate_mp4, validate_pdf, validate_task_file_metadata, validate_webm,
    };

    #[test]
    fn filename_and_media_allowlist_rejects_traversal_and_mismatches() {
        assert_eq!(
            validate_file_metadata("screen.png", "image/png").unwrap().1,
            ExpectedMedia::Png
        );
        assert!(validate_file_metadata("../screen.png", "image/png").is_err());
        assert!(validate_file_metadata("screen.png", "text/html").is_err());
        assert!(validate_file_metadata("document.pdf.exe", "application/pdf").is_err());
        assert!(validate_file_metadata("line\nbreak.pdf", "application/pdf").is_err());
        assert!(validate_file_metadata("invoice\u{202e}fdp.exe", "application/pdf").is_err());
    }

    #[test]
    fn task_files_allow_business_types_and_download_them_as_binary() {
        for (name, mime) in [
            (
                "requirements.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (
                "budget.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            ("notes.txt", "text/plain"),
            ("archive.zip", "application/zip"),
        ] {
            let (file_name, media) = validate_task_file_metadata(name, mime).unwrap();
            assert_eq!(file_name, name);
            assert!(matches!(media, ExpectedMedia::Business(_)));
            assert_eq!(media.content_type(), "application/octet-stream");
            assert_eq!(media.max_bytes(), 20 * 1_024 * 1_024);
            assert!(validate_file_metadata(name, mime).is_err());
        }
        for mime in ["image/png", "application/octet-stream", ""] {
            assert_eq!(
                validate_task_file_metadata("screen.PNG", mime).unwrap().1,
                ExpectedMedia::Png
            );
        }
        assert!(validate_task_file_metadata("screen.png", "text/html").is_err());
        for name in [
            "page.html",
            "screen.svg",
            "README",
            ".png",
            "setup.exe",
            "contract.docm",
            "budget.xlsm",
            "legacy.doc",
            "archive.rar",
            "report.docx.exe",
            "notes.txt:payload.exe",
        ] {
            assert!(validate_task_file_metadata(name, "").is_err(), "{name}");
        }
        assert!(validate_task_file_metadata("notes.docx", "text/html").is_err());
        for name in ["../notes.txt", "line\nbreak.txt", "invoice\u{202e}txt", ""] {
            assert!(validate_task_file_metadata(name, "text/plain").is_err());
        }
    }

    #[test]
    fn signatures_must_match_the_declared_safe_type() {
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&64_u32.to_be_bytes());
        png.extend_from_slice(&48_u32.to_be_bytes());
        png.extend_from_slice(b"\x00\x00\x00\x00IEND\xaeB\x60\x82");
        assert!(validate_image(&png, ExpectedMedia::Png));
        assert!(!validate_image(&png, ExpectedMedia::Jpeg));
        assert!(validate_pdf(b"%PDF-1.7\n", b"body\n%%EOF\n"));
        assert!(!validate_pdf(b"<html>", b"%%EOF"));

        let mp4 = [0, 0, 0, 12, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm'];
        assert!(validate_mp4(&mp4, mp4.len()));
        assert!(validate_webm(b"\x1a\x45\xdf\xa3\x42\x82\x84webm"));
    }

    #[test]
    fn inspection_buffer_keeps_only_the_bounded_edges() {
        let mut first = Vec::new();
        let mut tail = Vec::new();
        let data = vec![7_u8; super::INSPECTION_BYTES * 2];
        append_inspection_bytes(&mut first, &mut tail, &data);
        assert_eq!(first.len(), super::INSPECTION_BYTES);
        assert_eq!(tail.len(), super::INSPECTION_BYTES);
    }

    #[test]
    fn malware_scan_is_fail_closed() {
        assert!(parse_clamav_response(b"stream: OK\0").is_ok());
        assert!(parse_clamav_response(b"stream: Eicar-Test-Signature FOUND\0").is_err());
        assert!(parse_clamav_response(b"stream: scanner error ERROR\0").is_err());
        assert!(parse_clamav_response(b"").is_err());
    }

    async fn test_storage() -> super::AttachmentStorage {
        super::AttachmentStorage::prepare(&crate::config::AttachmentConfig {
            enabled: true,
            scan_mode: crate::config::AttachmentScanMode::Disabled,
            storage_path: std::env::temp_dir()
                .join(format!("tzomet-upload-test-{}", uuid::Uuid::now_v7())),
            ..Default::default()
        })
        .await
        .unwrap()
    }

    async fn multipart(body: Body) -> axum::extract::Multipart {
        axum::extract::Multipart::from_request(
            Request::builder()
                .header(
                    "content-type",
                    "multipart/form-data; boundary=test-boundary",
                )
                .body(body)
                .unwrap(),
            &(),
        )
        .await
        .unwrap()
    }

    fn upload_prefix() -> Vec<u8> {
        let mut body = b"--test-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.mp4\"\r\nContent-Type: video/mp4\r\n\r\n".to_vec();
        body.extend_from_slice(&[0, 0, 0, 12, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm']);
        body
    }

    #[tokio::test]
    async fn malformed_trailing_multipart_removes_quarantined_file() {
        let storage = test_storage().await;
        let mut body = upload_prefix();
        body.extend_from_slice(
            b"\r\n--test-boundary\r\nInvalid header\r\n\r\nx\r\n--test-boundary--\r\n",
        );
        let result = storage.receive(multipart(Body::from(body)).await).await;
        let leftover = tokio::fs::read_dir(&storage.quarantine_path)
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap();
        tokio::fs::remove_dir_all(storage.quarantine_path.parent().unwrap())
            .await
            .unwrap();
        assert!(result.is_err());
        assert!(
            leftover.is_none(),
            "invalid multipart must not leave unaccounted files"
        );
    }

    #[tokio::test]
    async fn cancelled_upload_removes_partial_file_and_releases_capacity() {
        use futures_util::{StreamExt, stream};
        let storage = test_storage().await;
        let body = Body::from_stream(
            stream::once(async { Ok::<_, std::io::Error>(upload_prefix()) })
                .chain(stream::pending()),
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            storage.receive(multipart(body).await),
        )
        .await;
        let leftover = tokio::fs::read_dir(&storage.quarantine_path)
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap();
        let permits = storage.uploads.available_permits();
        tokio::fs::remove_dir_all(storage.quarantine_path.parent().unwrap())
            .await
            .unwrap();
        assert!(result.is_err());
        assert_eq!(permits, 4);
        assert!(
            leftover.is_none(),
            "cancelled uploads must remove partial files"
        );
    }

    #[tokio::test]
    async fn successful_upload_remains_available_until_published() {
        let storage = test_storage().await;
        let mut body = upload_prefix();
        body.extend_from_slice(b"\r\n--test-boundary--\r\n");
        let mut prepared = storage
            .receive(multipart(Body::from(body)).await)
            .await
            .unwrap();
        assert_eq!(tokio::fs::metadata(&prepared.path).await.unwrap().len(), 12);
        storage.publish(&mut prepared).await.unwrap();
        assert_eq!(storage.uploads.available_permits(), 3);
        super::AttachmentStorage::release_upload_permit(&mut prepared);
        assert_eq!(storage.uploads.available_permits(), 4);
        let path = prepared.path.clone();
        drop(prepared);
        assert_eq!(tokio::fs::metadata(path).await.unwrap().len(), 12);
        tokio::fs::remove_dir_all(storage.quarantine_path.parent().unwrap())
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn existing_shared_directory_is_rejected_without_changing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!(
            "tzomet-attachment-permissions-{}",
            uuid::Uuid::now_v7()
        ));
        tokio::fs::create_dir(&path).await.unwrap();
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();

        let result = super::create_private_directory(&path).await;
        let mode = tokio::fs::symlink_metadata(&path)
            .await
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        tokio::fs::remove_dir(&path).await.unwrap();

        assert!(result.is_err());
        assert_eq!(mode, 0o755);
    }
}
