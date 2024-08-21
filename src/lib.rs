#![crate_name = "lfspull"]
//! # Pulling lfs files from git
//! This crate allows to download lfs files.
//! Please check [`repo_tools::glob_recurse_pull_directory`] for complete directories and [`repo_tools::pull_file`] for single files.
//! It really is the minimal viable example featuring only our own simple use-case: token / bearer auth and pulling a single file per request.
#![warn(missing_docs)]
#![warn(unused_qualifications)]
#![deny(deprecated)]

mod repo_tools;

/// The prelude to set everything up for calling any crate functions
pub mod prelude {
    use std::fmt::{Display, Formatter};
    use vg_errortools::FatIOError;

    /// This enum specifies the source of the file that has been placed inside the repository.
    #[derive(Debug, PartialEq, Eq, Copy, Clone)]
    pub enum FilePullMode {
        /// Remote was used
        DownloadedFromRemote,
        /// Local git-lfs cache was used
        UsedLocalCache,
        /// File was already pulled
        WasAlreadyPresent,
    }

    impl Display for FilePullMode {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            match self {
                FilePullMode::DownloadedFromRemote => write!(f, "Downloaded from lfs server"),
                FilePullMode::UsedLocalCache => write!(f, "Taken from local cache"),
                FilePullMode::WasAlreadyPresent => write!(f, "File already pulled"),
            }
        }
    }

    #[derive(thiserror::Error, Debug)]
    /// Errors that can happen during pulling the file
    pub enum LFSError {
        /// We received 401 or 403 from the lfs server
        #[error("Remote server responded with 401 or 403")]
        AccessDenied,
        /// We received 401 or 403 from the lfs server
        #[error("Remote server responded with not-okay code: {0}")]
        ResponseNotOkay(String),
        /// Some IO error happened, `FatFileIOError` does store a PathBuf to the source of the problem
        #[error("File IO error: {0}")]
        FatFileIOError(#[from] FatIOError),
        /// Parsing of some kind of file failed, see error message for more information
        #[error("Could not parse file: {0}")]
        InvalidFormat(&'static str),
        /// Forward from the `reqwest` package, something failed while executing the fetch
        #[error("Request-error: {0}")]
        RequestError(#[from] reqwest::Error),
        /// You tried to pull a non-existing file from the remote
        #[error("Remote file not found: {0}")]
        RemoteFileNotFound(&'static str),
        /// Download looked good, but somehow the checksum mismatches, please be careful of this one:
        /// It may indicate a mitm-attack
        #[error("Checksum incorrect")]
        ChecksumMismatch,
        /// Somehow decoding the oid in the file was not possible, maybe repo integrity is not ensured
        #[error("Could not decode oid-string to bytes: {0}")]
        OidNotValidHex(#[from] hex::FromHexError),
        /// Something went wrong when traversing the repository, e.g. files not in expected places
        #[error("Problem traversing directory structure: {0}")]
        DirectoryTraversalError(String),
        /// The remote url from the git repo config was not parseable
        #[error("Could not parse remote URL: {0}")]
        UrlParsingError(#[from] url::ParseError),
        /// Received invalid http headers
        #[error("Invalid header value: {0}")]
        InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),
        /// Somehow we received a non-ok http code on the transaction
        #[error("HTTP error: {0}")]
        HTTP(#[from] http::Error),
        /// Strange / malformed http response
        #[error("Invalid HTTP response: {0}")]
        InvalidResponse(String),
        /// something failed while creating tempfile
        #[error("TempFile error: {0}")]
        TempFile(String),
    }
}
pub use prelude::FilePullMode;
pub use prelude::LFSError;

#[doc(inline)]
pub use repo_tools::glob_recurse_pull_directory;
#[doc(inline)]
pub use repo_tools::pull_file;

impl From<&'static str> for LFSError {
    fn from(message: &'static str) -> Self {
        LFSError::InvalidFormat(message)
    }
}
