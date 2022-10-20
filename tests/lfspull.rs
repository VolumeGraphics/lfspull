use cucumber::{given, then, when, World};
use lfspull::prelude::*;
use std::env::temp_dir;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, World, Default)]
pub struct LFSWorld {
    current_fake_repo: Option<PathBuf>,
    pull_result: Option<FilePullMode>,
}

const TEST_GIT_CONFIG: &str = r#"[core]
        repositoryformatversion = 0
        filemode = true
        bare = false
        logallrefupdates = true
[remote "origin"]
        url = https://dev.azure.com/rohdealx/devops/_git/git-lfs-test
        fetch = +refs/heads/*:refs/remotes/origin/*
[branch "main"]
        remote = origin
        merge = refs/heads/main
[lfs]
        repositoryformatversion = 0
"#;

const TEST_LFS_FILE: &str = r#"version https://git-lfs.github.com/spec/v1
oid sha256:4329aab31bc9c72a897f57e038fe60655d31df6e5ddf2cf897669a845d64edbc
size 665694
"#;

const TEST_LFS_FILE_NAME: &str = "beer_tornado.mp4";

#[given(expr = "the test-repository is correctly setup")]
fn setup_fake_git_repo(world: &mut LFSWorld) {
    let temp_folder = temp_dir().join(Uuid::new_v4().to_string());
    world.current_fake_repo = Some(temp_folder);
    let repo_base = world.current_fake_repo.as_ref().unwrap();
    fs::create_dir_all(repo_base).expect("could not create temp-dir");
    fs::write(repo_base.join(TEST_LFS_FILE_NAME), TEST_LFS_FILE).expect("Unable to write file");
    fs::create_dir_all(repo_base.join(".git")).expect("could not create .git inside temp folder");
    fs::write(repo_base.join(".git").join("config"), TEST_GIT_CONFIG)
        .expect("Unable to write file");
}

#[given(expr = "the file was pulled already")]
#[when(expr = "pulling the file")]
async fn pull_file_step(world: &mut LFSWorld) {
    let file_path = world
        .current_fake_repo
        .as_ref()
        .unwrap()
        .clone()
        .join(TEST_LFS_FILE_NAME);
    world.pull_result = Some(
        lfspull::pull_file(file_path, None)
            .await
            .expect("Could not pull file"),
    );
}

#[when(expr = "pulling the complete directory")]
async fn pull_directory(world: &mut LFSWorld) {
    let fake_repo = world.current_fake_repo.as_ref().unwrap().to_string_lossy();
    let pattern = format!("{}/**/*", fake_repo);
    let recurse_pull = lfspull::glob_recurse_pull_directory(&pattern, None)
        .await
        .expect("Could not pull directory")
        .into_iter()
        .find(|(i, _)| i.contains(TEST_LFS_FILE_NAME))
        .expect("did not pull desired file");
    world.pull_result = Some(recurse_pull.1);
}

#[when(expr = "resetting the file")]
fn resetting_the_file(world: &mut LFSWorld) {
    let file_path = world
        .current_fake_repo
        .as_ref()
        .unwrap()
        .join(TEST_LFS_FILE_NAME);
    fs::remove_file(&file_path).ok();
    fs::write(file_path, TEST_LFS_FILE).expect("Unable to write file");
}

#[then(expr = "the file was pulled from origin")]
fn assert_origin_pull(world: &mut LFSWorld) {
    let last_result = world.pull_result.as_ref().expect("not pulled anything yet");
    assert_eq!(last_result, &FilePullMode::DownloadedFromRemote);
}

#[then(expr = "the file was pulled from local cache")]
fn assert_cached_pull(world: &mut LFSWorld) {
    let last_result = world.pull_result.as_ref().expect("not pulled anything yet");
    assert_eq!(last_result, &FilePullMode::UsedLocalCache);
}

#[then(expr = "the file was already there")]
fn assert_already_pulled(world: &mut LFSWorld) {
    let last_result = world.pull_result.as_ref().expect("not pulled anything yet");
    assert_eq!(last_result, &FilePullMode::WasAlreadyPresent);
}

#[then(expr = "the file size is {int}")]
fn check_file_size(world: &mut LFSWorld, size: u64) {
    let metadata = std::fs::metadata(
        world
            .current_fake_repo
            .as_ref()
            .unwrap()
            .join(TEST_LFS_FILE_NAME),
    )
    .expect("Could not get target file metadata");
    assert_eq!(metadata.len(), size);
}

#[tokio::main]
async fn main() {
    LFSWorld::run("tests/features").await;
}
