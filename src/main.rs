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

    /// Print debug information
    #[clap(short, long)]
    verbose: bool,
}

#[tokio::main]
pub async fn main() -> Result<(), LFSError> {
    // enable colors on windows cmd.exe
    // does not fail on powershell, even though powershell can do colors without this
    // will fail on jenkins/qa tough, that's why we need to ignore the result
    let _ = enable_ansi_support::enable_ansi_support();

    let args = Args::parse();
    let level = if args.verbose {
        Level::TRACE
    } else {
        Level::INFO
    };

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let access_token = args.access_token.as_deref();
    if let Some(file) = args.file_to_pull {
        info!("Single file mode: {}", file.to_string_lossy());
        let result = lfspull::pull_file(file, access_token).await?;
        info!("Result: {}", result);
    }
    if let Some(recurse_pattern) = args.recurse_pattern {
        info!("Glob-recurse mode: {}", &recurse_pattern);
        let results = lfspull::glob_recurse_pull_directory(&recurse_pattern, access_token).await?;
        info!("Pulling finished! Listing files and sources: ");

        results.into_iter().enumerate().for_each(|(id, (n, r))| {
            info!("{id} - '{n}': {r}");
        });
    }
    Ok(())
}
