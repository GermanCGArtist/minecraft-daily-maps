use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("This zip was already imported as snapshot {0}")]
    Duplicate(String),
    #[error("Renderer failed: {0}")]
    Renderer(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
