# LFSPull - a simple pulling tool for git-lfs
[![Crates.io](https://img.shields.io/crates/d/lfspull?style=flat)](https://crates.io/crates/lfspull)
[![Documentation](https://docs.rs/lfspull/badge.svg)](https://docs.rs/lfspull)
![CI](https://github.com/VolumeGraphics/lfspull/actions/workflows/rust.yml/badge.svg?branch=main "CI")
[![Coverage Status](https://coveralls.io/repos/github/VolumeGraphics/lfspull/badge.svg?branch=main)](https://coveralls.io/github/VolumeGraphics/lfspull?branch=main)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat)](LICENSE)

## Features
LFSPull allows you to pull files from git-lfs. 
It currently supports:
- Token-auth only
- Pulling single files
- Globbing patterns and pulling all matches
- Cache-compatible with the original git-lfs
- Hash verification of the downloaded file

## CLI guide

The CLI is pretty straight forward.
- `-f / --file-to-pull [FILE]` single file download mode
  - e.g. `lfspull -f my_file.tar.gz` downloads the file
- '-r / --recurse-pattern [PATTERN]' downloads everything that matches the pattern
  - e.g. 'lfspull -r "*.tgz"' downloads all .tgz files in this folder
  - e.g. 'lfspull -r "**/*.tgz"' downloads all .tgz files this folder and all subfolders
- '-b / --random-bytes [RANDOM_BYTES]' for temp file name. See https://docs.rs/tempfile/latest/tempfile/struct.Builder.html#method.rand_bytes
- '-a / --access-token [TOKEN]' sets the token - can also be set via $ACCESS_TOKEN from env
- '-m / --max-retry [NUMBER]' max number of download attempts if fail
- '-t / --timeout [NUMBER]' set timeout in seconds for git lfs pull request
  - When None given, the timeout is calculated automatically based on lfs object size
  - When 0 given, there is no timeout
- '-v' for verbose mode

## Library API guide

Please see our docs.rs for example code and the gherkin tests for how to check the origin of the file.

## Changelog

### 0.4.2

- create temp file in the cached folder instead of working directory
- detect whether cached file and repo are in the same drive/device. If yes, use hard link, if not, file will be copied

### 0.4.1

- add rust-toolchain 1.88
- read git config for lfs storage path
- add timeout

### 0.4.0

- upgrade a few dependencies
- add retry attempt when failing fetching from git

### 0.3.1

- fix bug when trying to rename temp file to cache file, but cache file is already created and locked by other parallel job

### 0.3.0

- use stream_bytes to download object directly into a temporary files and avoid 'memory allocation of x bytes failed'