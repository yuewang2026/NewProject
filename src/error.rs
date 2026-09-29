//! Error type shared by every deckr stage.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("not a PowerPoint file (.pptx): {0}")]
    NotOpenXml(PathBuf),

    #[error("ZIP structure: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("malformed XML in {part}: {source}")]
    Xml {
        part: String,
        #[source]
        source: quick_xml::Error,
    },

    #[error("required part missing from the package: {0}")]
    MissingPart(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
