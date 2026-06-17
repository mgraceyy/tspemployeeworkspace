use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub const DEFAULT_MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct StoredUpload {
    pub original_name: String,
    pub stored_path: String,
    pub mime_type: String,
    pub size_bytes: i64,
}

pub fn normalize_upload_dir(path: &str) -> PathBuf {
    PathBuf::from(path)
}

pub fn validate_requirement_upload(
    original_name: &str,
    mime_type: Option<&str>,
    bytes: &[u8],
    max_bytes: usize,
) -> AppResult<(String, String)> {
    if bytes.is_empty() {
        return Err(AppError::bad_request("Uploaded file is empty"));
    }
    if bytes.len() > max_bytes {
        return Err(AppError::bad_request(format!(
            "File is too large (max {} MB)",
            max_bytes / (1024 * 1024)
        )));
    }

    let sanitized = sanitize_filename(original_name);
    if sanitized.is_empty() {
        return Err(AppError::bad_request("Invalid file name"));
    }

    let ext = Path::new(&sanitized)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| AppError::bad_request("File must have an extension"))?;

    let allowed_ext = ["pdf", "jpg", "jpeg", "png", "webp", "doc", "docx"];
    if !allowed_ext.contains(&ext.as_str()) {
        return Err(AppError::bad_request(
            "Allowed file types: PDF, JPG, PNG, WEBP, DOC, DOCX",
        ));
    }

    let mime = mime_type.unwrap_or("").to_ascii_lowercase();
    let allowed_mimes = [
        "application/pdf",
        "image/jpeg",
        "image/png",
        "image/webp",
        "application/msword",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ];
    if !mime.is_empty() && !allowed_mimes.iter().any(|allowed| *allowed == mime) {
        return Err(AppError::bad_request(
            "Unsupported file type — use PDF, image, or Word document",
        ));
    }

    let sniffed = sniff_file_kind(bytes).ok_or_else(|| {
        AppError::bad_request(
            "Unrecognized file content — upload a valid PDF, image, or Word document",
        )
    })?;
    if !kinds_match_ext(sniffed, &ext) {
        return Err(AppError::bad_request(
            "File content does not match its extension",
        ));
    }

    Ok((sanitized, ext))
}

fn kinds_match_ext(sniffed: &str, ext: &str) -> bool {
    matches!(
        (sniffed, ext),
        ("pdf", "pdf")
            | ("jpeg", "jpg")
            | ("jpeg", "jpeg")
            | ("png", "png")
            | ("webp", "webp")
            | ("doc", "doc")
            | ("docx", "docx")
    )
}

fn sniff_file_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF-") {
        return Some("pdf");
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("jpeg");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if bytes.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") {
        return Some("doc");
    }
    if is_docx_zip(bytes) {
        return Some("docx");
    }
    None
}

fn is_docx_zip(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"PK\x03\x04") {
        return false;
    }
    bytes
        .windows(b"word/document.xml".len())
        .any(|window| window == b"word/document.xml")
}

pub async fn store_requirement_file(
    upload_dir: &Path,
    employee_id: Uuid,
    requirement_id: Uuid,
    original_name: &str,
    mime_type: &str,
    bytes: &[u8],
    max_bytes: usize,
) -> AppResult<StoredUpload> {
    let (sanitized, ext) =
        validate_requirement_upload(original_name, Some(mime_type), bytes, max_bytes)?;
    let stored_name = format!("{}.{}", Uuid::new_v4(), ext);
    let relative = format!("requirements/{employee_id}/{requirement_id}/{stored_name}");
    let absolute = upload_dir.join(&relative);

    if let Some(parent) = absolute.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }

    tokio::fs::write(&absolute, bytes)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    Ok(StoredUpload {
        original_name: sanitized,
        stored_path: relative,
        mime_type: mime_type.to_string(),
        size_bytes: bytes.len() as i64,
    })
}

pub async fn read_stored_file(upload_dir: &Path, stored_path: &str) -> AppResult<Vec<u8>> {
    if stored_path.contains("..") || stored_path.starts_with('/') || stored_path.starts_with('\\') {
        return Err(AppError::bad_request("Invalid file path"));
    }

    let absolute = upload_dir.join(stored_path);
    let canonical_base = upload_dir
        .canonicalize()
        .map_err(|e| AppError::Internal(e.into()))?;
    let canonical_file = absolute.canonicalize().map_err(|_| AppError::NotFound)?;

    if !canonical_file.starts_with(&canonical_base) {
        return Err(AppError::Forbidden);
    }

    tokio::fs::read(&canonical_file)
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

pub async fn delete_stored_file(upload_dir: &Path, stored_path: &str) -> AppResult<()> {
    if stored_path.contains("..") {
        return Ok(());
    }

    let absolute = upload_dir.join(stored_path);
    if absolute.exists() {
        tokio::fs::remove_file(&absolute)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }
    Ok(())
}

fn sanitize_filename(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    base.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        .take(120)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_extension() {
        let err = validate_requirement_upload(
            "virus.exe",
            Some("application/octet-stream"),
            b"data",
            1024,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn accepts_pdf_upload() {
        let (name, ext) =
            validate_requirement_upload("id.pdf", Some("application/pdf"), b"%PDF-", 1024).unwrap();
        assert_eq!(name, "id.pdf");
        assert_eq!(ext, "pdf");
    }

    #[test]
    fn rejects_mismatched_file_content() {
        let err = validate_requirement_upload("fake.pdf", Some("application/pdf"), b"NOTPDF", 1024)
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn accepts_valid_docx_upload() {
        let bytes = minimal_docx_bytes();
        let (name, ext) = validate_requirement_upload(
            "resume.docx",
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            &bytes,
            1024 * 1024,
        )
        .unwrap();
        assert_eq!(name, "resume.docx");
        assert_eq!(ext, "docx");
    }

    #[test]
    fn rejects_zip_without_word_document_as_docx() {
        let bytes = b"PK\x03\x04\x00\x00generic zip payload without ooxml paths";
        let err = validate_requirement_upload(
            "fake.docx",
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            bytes,
            1024,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    fn minimal_docx_bytes() -> Vec<u8> {
        let mut bytes = vec![0x50, 0x4B, 0x03, 0x04];
        bytes.extend_from_slice(b"local header padding");
        bytes.extend_from_slice(b"word/document.xml");
        bytes.extend_from_slice(b"<w:document/>");
        bytes
    }
}
