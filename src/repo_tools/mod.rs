use crate::prelude::*;
mod primitives;

use futures_util::TryFutureExt;
use glob::glob;
use primitives::get_repo_root;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::fs::read_to_string;
use tracing::{debug, error, info, warn};
use url::Url;
use vg_errortools::{fat_io_wrap_tokio, FatIOError};

async fn get_remote_url_from_file(git_file: impl AsRef<Path>) -> Result<String, LFSError> {
    let file_buffer = fat_io_wrap_tokio(git_file, read_to_string).await?;
    let remote_url = file_buffer
        .lines()
        .find(|&line| line.contains("url"))
        .as_ref()
        .ok_or(LFSError::InvalidFormat(
            ".git/config contains no remote url",
        ))?
        .split('=')
        .next_back()
        .as_ref()
        .ok_or(LFSError::InvalidFormat(".git/config url line malformed"))?
        .trim();
    Ok(remote_url.to_owned())
}

async fn get_real_repo_root<P: AsRef<Path>>(repo_path: P) -> Result<PathBuf, LFSError> {
    let git_path = repo_path.as_ref().join(".git");
    let real_git_path = if repo_path.as_ref().join(".git").is_file() {
        //worktree case
        let worktree_file_contents = fat_io_wrap_tokio(git_path, read_to_string).await?;
        let worktree_path = worktree_file_contents
            .split(':')
            .find(|c| c.contains(".git"))
            .expect("Could not resolve original repo .git/config file from worktree .git file")
            .trim();
        get_repo_root(worktree_path)
            .await
            .expect("Found worktree, but couldn't resolve root-repo")
    } else if git_path.is_dir() {
        //git main copy
        git_path
            .parent()
            .expect("Git path has no parent")
            .to_owned()
    } else {
        //no .git in repo_root - bad
        return Err(LFSError::DirectoryTraversalError(
            "Could not find .git file or folder in directory structure".to_owned(),
        ));
    };

    Ok(real_git_path)
}

async fn get_remote_url<P: AsRef<Path>>(repo_path: P) -> Result<String, LFSError> {
    let config_file = get_real_repo_root(repo_path.as_ref())
        .await?
        .join(".git")
        .join("config");

    get_remote_url_from_file(config_file).await
}

fn remote_url_ssh_to_https(repo_url: String) -> Result<String, LFSError> {
    let input_url = Url::parse(&repo_url)?;
    if input_url.scheme() == "https" {
        return Ok(repo_url);
    } else if input_url.scheme() != "ssh" {
        return Err(LFSError::InvalidFormat("Url is neither https nor ssh"));
    }
    let host = input_url
        .host_str()
        .ok_or(LFSError::InvalidFormat("Url had no valid host"))?;
    let path = input_url.path();
    Ok(format!("https://{host}{path}"))
}

async fn get_cache_dir<P: AsRef<Path>>(
    repo_root: P,
    metadata: &primitives::MetaData,
) -> Result<PathBuf, LFSError> {
    let oid_1 = &metadata.oid[0..2];
    let oid_2 = &metadata.oid[2..4];

    let mut git_folder = get_real_repo_root(repo_root).await?.join(".git");
    let config = git_folder.join("config");
    if config.exists() {
        debug!("Read git config file in {}", config.to_string_lossy());
        let config_content = read_to_string(&config).await.unwrap_or_else(|e| {
            warn!("Could not read git config: {e}");
            String::new()
        });
        let mut config_content = config_content.lines().peekable();

        while config_content.peek().is_some() {
            let line = config_content.next().unwrap_or_default();
            let line = line.trim();
            if line.contains("[lfs]") {
                while config_content.peek().is_some() {
                    let next_line = config_content.next().unwrap_or_default();
                    let next_line = next_line.trim();
                    if let Some(storage_url) = next_line.strip_prefix("storage = ") {
                        debug!("Found git lfs storage path: '{storage_url}'");
                        git_folder = PathBuf::from(storage_url);
                        break;
                    }
                }
                break;
            }
        }
    }

    Ok(git_folder
        .join("lfs")
        .join("objects")
        .join(oid_1)
        .join(oid_2))
}

async fn get_file_cached<P: AsRef<Path>>(
    repo_root: P,
    metadata: &primitives::MetaData,
    access_token: Option<&str>,
    max_retry: u32,
    randomizer_bytes: Option<usize>,
    timeout: Option<u64>,
) -> Result<(PathBuf, FilePullMode), LFSError> {
    debug!("version: {}", &metadata.version);
    let cache_dir = get_cache_dir(&repo_root, metadata).await?;
    debug!("cache dir {:?}", &cache_dir);
    let cache_file = cache_dir.join(&metadata.oid);
    debug!("cache file {:?}", &cache_file);
    let repo_url = remote_url_ssh_to_https(get_remote_url(&repo_root).await?)?;

    if cache_file.is_file() {
        Ok((cache_file, FilePullMode::UsedLocalCache))
    } else {
        fat_io_wrap_tokio(cache_dir, fs::create_dir_all)
            .await
            .map_err(|_| {
                LFSError::DirectoryTraversalError(
                    "Could not create lfs cache directory".to_string(),
                )
            })?;

        let temp_file = primitives::download_file(
            metadata,
            &repo_url,
            access_token,
            max_retry,
            randomizer_bytes,
            timeout,
        )
        .await?;
        if cache_file.exists() {
            info!(
                "cache file {:?} is already written from other process",
                &cache_file
            );
        } else {
            fs::rename(&temp_file.path(), cache_file.as_path())
                .map_err(|e| {
                    error!(
                        "Could not rename {:?} to {:?}: {:?}",
                        temp_file.path(),
                        cache_file.as_path(),
                        &e
                    );
                    LFSError::FatFileIOError(FatIOError::from_std_io_err(
                        e,
                        temp_file.path().to_path_buf(),
                    ))
                })
                .await?;
        }

        Ok((cache_file, FilePullMode::DownloadedFromRemote))
    }
}

/// Ensures a single file is pulled from the lfs
/// Currently only token/https auth is supported.
/// Various errors can occur which are covered by the `LFSError` struct
/// The return value specifies the origin of the file
/// # Arguments
///
/// * `lfs_file` - Anything describing a path to an lfs node file
///
/// * `access_token` - The token for Bearer-Auth via HTTPS
///
pub async fn pull_file<P: AsRef<Path>>(
    lfs_file: P,
    access_token: Option<&str>,
    max_retry: u32,
    randomizer_bytes: Option<usize>,
    timeout: Option<u64>,
) -> Result<FilePullMode, LFSError> {
    info!("Pulling file {}", lfs_file.as_ref().to_string_lossy());
    if !primitives::is_lfs_node_file(&lfs_file).await? {
        info!(
            "File ({}) not an lfs-node file - pulled already.",
            lfs_file.as_ref().file_name().unwrap().to_string_lossy()
        );
        return Ok(FilePullMode::WasAlreadyPresent);
    }

    debug!("parsing metadata");
    let metadata = primitives::parse_lfs_file(&lfs_file).await?;
    debug!("Downloading file");
    let repo_root = get_repo_root(&lfs_file).await.map_err(|e| {
        LFSError::DirectoryTraversalError(format!("Could not find git repo root: {e:?}"))
    })?;
    let (file_name_cached, origin) = get_file_cached(
        &repo_root,
        &metadata,
        access_token,
        max_retry,
        randomizer_bytes,
        timeout,
    )
    .await?;
    info!(
        "Found file (Origin: {:?}), linking to {}",
        origin,
        lfs_file.as_ref().to_string_lossy()
    );
    fat_io_wrap_tokio(&lfs_file, fs::remove_file).await?;
    fs::hard_link(&file_name_cached, lfs_file)
        .await
        .map_err(|e| FatIOError::from_std_io_err(e, file_name_cached.clone()))?;
    Ok(origin)
}

fn glob_recurse(wildcard_pattern: &str) -> Result<Vec<PathBuf>, LFSError> {
    let mut return_vec = Vec::new();

    let glob = glob(wildcard_pattern).map_err(|e| {
        LFSError::DirectoryTraversalError(format!("Could not parse glob pattern: {e}"))
    })?;
    for entry in glob {
        return_vec.push(entry.map_err(|e| {
            LFSError::DirectoryTraversalError(format!("Error in glob result list: {e}"))
        })?);
    }
    Ok(return_vec)
}

/// Pulls a glob recurse expression
/// In addition to the same errors as in `pull_file`, more `LFSError::DirectoryTraversalError` can occur if something is wrong with the pattern
/// # Arguments
///
/// * `wildcard_pattern` - the pattern to glob-recurse
///
/// * `access_token` - the token for Bearer-Auth via HTTPS
///
/// * `max retry` - max number of retry attempt when http request fails
///
/// * `randomizer bytes` - bytes used to create a randomized named temp file
///
/// # Examples
///
/// Load all .jpg files from all subdirectories
/// ```no_run
/// let result = lfspull::glob_recurse_pull_directory("dir/to/pull/**/*.jpg", Some("secret-token"), 3, Some(5), Some(0));
/// ```
///
pub async fn glob_recurse_pull_directory(
    wildcard_pattern: &str,
    access_token: Option<&str>,
    max_retry: u32,
    randomizer_bytes: Option<usize>,
    timeout: Option<u64>,
) -> Result<Vec<(String, FilePullMode)>, LFSError> {
    let mut result_vec = Vec::new();
    let files = glob_recurse(wildcard_pattern)?;
    for path in files {
        result_vec.push((
            path.to_string_lossy().to_string(),
            pull_file(&path, access_token, max_retry, randomizer_bytes, timeout).await?,
        ));
    }

    Ok(result_vec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::error;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_glob_directory() {
        let test_dir = &format!("{}/tests/**/*.rs", env!("CARGO_MANIFEST_DIR"));
        let result = glob_recurse(test_dir).expect("could not recurse our own tests directory");
        assert_eq!(result.len(), 1);
        assert_eq!(result.first().unwrap().file_name().unwrap(), "lfspull.rs");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_glob_subdirectories() {
        let test_dir = &format!("{}/tests/**/*.feature", env!("CARGO_MANIFEST_DIR"));
        let result = glob_recurse(test_dir).expect("could not recurse our own tests directory");
        assert_eq!(result.len(), 1);
        assert_eq!(
            result.first().unwrap().file_name().unwrap(),
            "lfspull.feature"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_find_current_repo_root_from_cargo_directory() {
        let result = get_repo_root(env!("CARGO_MANIFEST_DIR"))
            .await
            .map_err(|e| error!("{:#?}", e));
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_find_current_repo_root_from_source_file() {
        let current_file_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file!());
        let result = get_repo_root(current_file_path)
            .await
            .map_err(|e| error!("{:#?}", e));
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn current_repo_remote_url_correct() {
        let current_file_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file!());

        let repo_root = get_repo_root(current_file_path)
            .await
            .map_err(|e| error!("{:#?}", e))
            .expect("Could not get repo root");

        let repo_remote = get_remote_url(repo_root)
            .await
            .map_err(|e| error!("{:#?}", e))
            .expect("Could not get repo remote");

        assert!(Url::parse(&repo_remote).is_ok());
    }

    const REPO_REMOTE: &str = "ssh://git@github.com/VolumeGraphics/lfspull.git";
    const REPO_REMOTE_HTTPS: &str = "https://github.com/VolumeGraphics/lfspull.git";
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn current_repo_remote_https_transform_works() {
        let repo_url_https =
            remote_url_ssh_to_https(REPO_REMOTE.to_string()).expect("Could not parse url");
        assert_eq!(repo_url_https, REPO_REMOTE_HTTPS);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn current_repo_remote_https_transform_works_identity() {
        let repo_url_https =
            remote_url_ssh_to_https(REPO_REMOTE_HTTPS.to_string()).expect("Could not parse url");
        assert_eq!(repo_url_https.as_str(), REPO_REMOTE_HTTPS);
    }
}
