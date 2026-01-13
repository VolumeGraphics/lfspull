use crate::prelude::*;
use futures_util::stream::StreamExt;
use http::StatusCode;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info};
use url::Url;
use vg_errortools::{fat_io_wrap_tokio, FatIOError};

const SIZE_PREFIX: &str = "size";
const VERSION_PREFIX: &str = "version";
const OID_PREFIX: &str = "oid";
const FILE_HEADER: &str = "version https://git-lfs.github.com/spec/v1";

/// Finds the git repository root folder of the given file
pub async fn get_repo_root<P: AsRef<Path>>(file_or_path: P) -> Result<PathBuf, LFSError> {
    info!(
        "Searching git repo root from path {}",
        file_or_path.as_ref().to_string_lossy()
    );
    let repo_dir = fs::canonicalize(file_or_path.as_ref()).await.map_err(|e| {
        LFSError::DirectoryTraversalError(format!(
            "Problem getting the absolute path of {}: {}",
            file_or_path.as_ref().to_string_lossy(),
            e.to_string().as_str()
        ))
    })?;
    let components: Vec<_> = repo_dir.components().collect();
    for i in (0..components.len()).rev() {
        let path = components
            .iter()
            .take(i)
            .fold(PathBuf::new(), |a, b| a.join(b));
        if path.join(".git").exists() {
            return Ok(path);
        }
    }

    Err(LFSError::DirectoryTraversalError(format!(
        "Could not find .git in any parent path of the given path ({})",
        file_or_path.as_ref().to_string_lossy()
    )))
}

#[derive(PartialEq, Eq, Debug)]
pub enum Hash {
    SHA256,
    Other,
}

#[derive(Debug)]
pub struct MetaData {
    pub version: String,
    pub oid: String,
    pub size: usize,
    pub hash: Option<Hash>,
}

pub async fn parse_lfs_file<P: AsRef<Path>>(path: P) -> Result<MetaData, LFSError> {
    let contents = fat_io_wrap_tokio(path, fs::read_to_string).await?;
    parse_lfs_string(contents.as_str())
}

fn parse_lfs_string(input: &str) -> Result<MetaData, LFSError> {
    let lines: HashMap<_, _> = input
        .lines()
        .map(|line| line.split(' ').collect::<Vec<_>>())
        .filter_map(|split_line| Some((*split_line.first()?, *split_line.last()?)))
        .collect();

    let size = lines
        .get(SIZE_PREFIX)
        .ok_or("Could not find size entry")?
        .parse::<usize>()
        .map_err(|_| "Could not convert file size to usize")?;

    let version = *lines
        .get(VERSION_PREFIX)
        .ok_or("Could not find version-entry")?;

    let mut oid = *lines.get(OID_PREFIX).ok_or("Could not find oid-entry")?;

    let mut hash = None;
    if oid.contains(':') {
        let lines: Vec<_> = oid.split(':').collect();
        if lines.first().ok_or("Problem parsing oid entry for hash")? == &"sha256" {
            hash = Some(Hash::SHA256);
        } else {
            hash = Some(Hash::Other);
        }
        oid = *lines.last().ok_or("Problem parsing oid entry for oid")?;
    }

    Ok(MetaData {
        size,
        oid: oid.to_string(),
        hash,
        version: version.to_string(),
    })
}

fn url_with_auth(url: &str, access_token: Option<&str>) -> Result<Url, LFSError> {
    let mut url = Url::parse(url)?;
    let username = if access_token.is_some() { "oauth2" } else { "" };
    let result = url.set_username(username);
    assert!(result.is_ok());
    let result = url.set_password(access_token);
    assert!(result.is_ok());
    Ok(url)
}

async fn handle_download(
    meta_data: &MetaData,
    repo_remote_url: &str,
    access_token: Option<&str>,
    randomizer_bytes: Option<usize>,
    temp_dir: &Option<impl AsRef<Path>>,
) -> Result<NamedTempFile, LFSError> {
    const MEDIA_TYPE: &str = "application/vnd.git-lfs+json";
    let client = Client::builder().build()?;
    assert_eq!(meta_data.hash, Some(Hash::SHA256));
    // we are implementing git-lfs batch API here: https://github.com/git-lfs/git-lfs/blob/main/docs/api/batch.md
    let request = json!({
        "operation": "download",
        "transfers": [ "basic" ],
        "ref": {"name" : "refs/heads/main" },
        "objects": vec!{Object::from_metadata(meta_data)},
        "hash_algo": "sha256"
    });

    let request_url = repo_remote_url.to_owned() + "/info/lfs/objects/batch";
    let request_url = url_with_auth(&request_url, access_token)?;
    let response = client
        .post(request_url.clone())
        .header("Accept", MEDIA_TYPE)
        .header("Content-Type", MEDIA_TYPE)
        .json(&request)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        error!(
            "Failed to request git lfs actions with status code {} and body {}",
            status,
            response.text().await?,
        );
        return if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
            Err(LFSError::AccessDenied)
        } else {
            Err(LFSError::ResponseNotOkay(format!("{status}")))
        };
    }
    let parsed_result = response.json::<ApiResult>().await?;

    // download already, this could be moved out and made async
    let object = parsed_result
        .objects
        .first()
        .ok_or(LFSError::RemoteFileNotFound(
            "Empty object list response from LFS server",
        ))?;

    let action = object.actions.as_ref().ok_or(LFSError::RemoteFileNotFound(
        "No action received from LFS server",
    ))?;

    let url = url_with_auth(&action.download.href, access_token)?;
    let headers: http::HeaderMap = (&action.download.header).try_into()?;
    let download_request_builder = client.get(url).headers(headers);
    let response = download_request_builder.send().await?;
    let download_status = response.status();
    if !download_status.is_success() {
        let message = format!(
            "Download failed: {} - body {}",
            download_status,
            response.text().await.unwrap_or_default()
        );
        return Err(LFSError::InvalidResponse(message));
    }

    debug!("creating temp file in current dir");

    const TEMP_SUFFIX: &str = ".lfstmp";

    let temp_dir = if let Some(dir) = temp_dir {
        dir.as_ref()
    } else {
        Path::new("./")
    };

    let tmp_path = PathBuf::from(temp_dir).join(format!("{}{TEMP_SUFFIX}", &meta_data.oid));
    if randomizer_bytes.is_none() && tmp_path.exists() {
        debug!("temp file exists. Deleting");
        fat_io_wrap_tokio(&tmp_path, fs::remove_file).await?;
    }
    let temp_file = tempfile::Builder::new()
        .prefix(&meta_data.oid)
        .suffix(TEMP_SUFFIX)
        .rand_bytes(randomizer_bytes.unwrap_or_default())
        .tempfile_in(temp_dir)
        .map_err(|e| LFSError::TempFile(e.to_string()))?;

    debug!("created tempfile: {:?}", &temp_file);

    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        temp_file.as_file().write_all(&chunk).map_err(|e| {
            error!("Could not write tempfile");
            LFSError::FatFileIOError(FatIOError::from_std_io_err(
                e,
                temp_file.path().to_path_buf(),
            ))
        })?;
        hasher.update(chunk);
    }
    temp_file.as_file().flush().map_err(|e| {
        error!("Could not flush tempfile");
        LFSError::FatFileIOError(FatIOError::from_std_io_err(
            e,
            temp_file.path().to_path_buf(),
        ))
    })?;

    debug!("checking hash");

    let result = hasher.finalize();
    let hex_data = hex::decode(object.oid.as_bytes())?;
    if result[..] == hex_data {
        Ok(temp_file)
    } else {
        Err(LFSError::ChecksumMismatch)
    }
}

pub async fn download_file(
    meta_data: &MetaData,
    repo_remote_url: &str,
    access_token: Option<&str>,
    max_retry: u32,
    randomizer_bytes: Option<usize>,
    connection_timeout: Option<u64>,
    temp_dir: Option<impl AsRef<Path>>,
) -> Result<NamedTempFile, LFSError> {
    let effective_timeout = get_effective_timeout(connection_timeout, meta_data.size);
    for attempt in 1..=max_retry {
        debug!("Download attempt {attempt}");
        let download = handle_download(
            meta_data,
            repo_remote_url,
            access_token,
            randomizer_bytes,
            &temp_dir,
        );
        let result = if let Some(seconds) = effective_timeout {
            timeout(Duration::from_secs(seconds), download).await
        } else {
            Ok(download.await)
        };

        match result {
            Ok(download_result) => match download_result {
                Ok(tempfile) => {
                    return Ok(tempfile);
                }
                Err(e) => {
                    if matches!(e, LFSError::AccessDenied) {
                        return Err(e);
                    }
                    error!("Download error: {e}");
                }
            },
            Err(timeout_err) => {
                error!("Timeout reached: {timeout_err}");
            }
        }
        sleep(Duration::from_secs(1)).await;
    }

    Err(LFSError::ReachedMaxDownloadAttempt)
}

/// Some(0) => no timeout
/// Some(x) => x seconds timeout
/// None => automatic
fn get_effective_timeout(timeout: Option<u64>, file_size_in_kb: usize) -> Option<u64> {
    match timeout {
        Some(0) => {
            debug!("No timeout");
            None
        }
        Some(val) => {
            debug!("Set timeout to {val} s");
            Some(val)
        }
        None => {
            let min_upload_speed_mb_per_sec = 1.0;
            let min_timeout_secs = 30;
            let file_size_mb = file_size_in_kb as f64 / (1024.0 * 1024.0);
            let timeout_secs = (file_size_mb / min_upload_speed_mb_per_sec).ceil() as u64;
            let timeout_secs = timeout_secs.max(min_timeout_secs);

            debug!("Automatic calculated timeout: {timeout_secs} s");

            Some(timeout_secs)
        }
    }
}

pub async fn is_lfs_node_file<P: AsRef<Path>>(path: P) -> Result<bool, LFSError> {
    if path.as_ref().is_dir() {
        return Ok(false);
    }
    let mut reader = fat_io_wrap_tokio(&path, fs::File::open).await?;
    let mut buf: Vec<u8> = vec![0; FILE_HEADER.len()];
    let read_result = reader.read_exact(buf.as_mut_slice()).await;
    if let Err(e) = read_result {
        match e.kind() {
            std::io::ErrorKind::UnexpectedEof => Ok(false),
            _ => Err(LFSError::FatFileIOError(FatIOError::from_std_io_err(
                e,
                path.as_ref().to_path_buf(),
            ))),
        }
    } else {
        Ok(buf == FILE_HEADER.as_bytes())
    }
}

#[derive(Deserialize, Debug)]
struct ApiResult {
    objects: Vec<Object>,
}

#[derive(Deserialize, Serialize, Debug)]
struct Object {
    oid: String,
    size: usize,
    actions: Option<Action>,
    authenticated: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug)]
struct Action {
    download: Download,
}

#[derive(Deserialize, Serialize, Debug)]
struct Download {
    href: String,
    header: HashMap<String, String>,
}

impl Object {
    fn from_metadata(input: &MetaData) -> Self {
        Object {
            oid: input.oid.clone(),
            size: input.size,
            actions: None,
            authenticated: None,
        }
    }
}

#[cfg(test)]
mod tests {
    const URL: &str = "https://dev.azure.com/buildvgmpsmi/buildvg/_git/git-lfs-test";
    use super::*;
    const LFS_TEST_DATA: &str = r#"version https://git-lfs.github.com/spec/v1
oid sha256:0fae26606afd128d4d2f730462c8451b90931d25813e06e55239a2ca00e74c74
size 226848"#;
    #[test]
    fn test_parsing_of_string() {
        let parsed = parse_lfs_string(LFS_TEST_DATA).expect("Could not parse demo-string!");
        assert_eq!(parsed.size, 226848);
        assert_eq!(parsed.version, "https://git-lfs.github.com/spec/v1");
        assert_eq!(
            parsed.oid,
            "0fae26606afd128d4d2f730462c8451b90931d25813e06e55239a2ca00e74c74"
        );
        assert_eq!(parsed.hash, Some(Hash::SHA256));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn try_pull_from_demo_repo() {
        let parsed = parse_lfs_string(LFS_TEST_DATA).expect("Could not parse demo-string!");
        let temp_file = download_file(&parsed, URL, None, 3, None, Some(0), None::<&str>)
            .await
            .expect("could not download file");
        let temp_size = temp_file
            .as_file()
            .metadata()
            .expect("could not get temp file size")
            .len();
        assert_eq!(temp_size as usize, parsed.size);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn identify_lfs_file() {
        let lfs_test_file_name = "test.lfs.file";
        fs::write(lfs_test_file_name, LFS_TEST_DATA)
            .await
            .expect("Unable to write file");
        let result = is_lfs_node_file(lfs_test_file_name)
            .await
            .expect("File was not readable");
        fs::remove_file(lfs_test_file_name)
            .await
            .expect("Could not clean up file");
        assert!(result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn identify_not_lfs_file() {
        let current_file_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file!());
        let result = is_lfs_node_file(current_file_path)
            .await
            .expect("File was not readable");
        assert!(!result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_get_effective_timeout() {
        let timeout = get_effective_timeout(Some(9), 1000);
        assert_eq!(timeout, Some(9));

        let timeout = get_effective_timeout(Some(0), 1000);
        assert_eq!(timeout, None);

        let timeout = get_effective_timeout(None, 1000);
        assert_eq!(timeout, Some(30));

        let timeout = get_effective_timeout(None, 200000000);
        assert_eq!(timeout, Some(191));
    }
}
