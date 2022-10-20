use clap::Parser;
use lfspull::prelude::*;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// This CLI tool allows to download either a single file or a glob'able expression from git-lfs.
#[derive(Parser)]
#[clap(about, long_about = None)]
struct Args {
    ///File to pull, must be a valid lfs-file inside the git repo
    #[clap(short, long)]
    file_to_pull: Option<PathBuf>,
    /// The access token for https-bearer-auth to the repo
    #[clap(short, long, env = "ACCESS_TOKEN")]
    access_token: Option<String>,
    /// A recursive pull pattern with asterisks (e.g. /path/to/repo/**/*.jpg)
    #[clap(short, long)]
    recurse_pattern: Option<String>,
}

#[tokio::main]
pub async fn main() -> Result<(), LFSError> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let args = Args::parse();
    let access_token = args.access_token.as_deref();
    if let Some(file) = args.file_to_pull {
        info!("Single file mode: {}", file.to_string_lossy());
        lfspull::pull_file(file, access_token).await?;
    }
    if let Some(recurse_pattern) = args.recurse_pattern {
        info!("Glob-recurse mode: {}", &recurse_pattern);
        lfspull::glob_recurse_pull_directory(&recurse_pattern, access_token).await?;
    }
    Ok(())
}
