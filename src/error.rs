use thiserror::Error;

/// Unified error type for the application.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("document not found")]
    NotFound,

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("security policy blocked: {0}")]
    SecurityBlocked(String),

    #[error("scraper error: {0}")]
    Scraper(String),

    #[error("PDF extraction error: {0}")]
    PdfExtract(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::Json;
        use serde_json::json;

        let (status, message) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::SecurityBlocked(_) => (StatusCode::FORBIDDEN, self.to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
