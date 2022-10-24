# LFSPull - a simple pulling tool for git-lfs
[![Crates.io](https://img.shields.io/crates/d/lfspull?style=flat)](https://crates.io/crates/lfspull)
[![Documentation](https://docs.rs/lfspull/badge.svg)](https://docs.rs/lfspull)
![CI](https://github.com/VolumeGraphics/lfspull/actions/workflows/rust.yml/badge.svg?branch=main "CI")
[![Coverage Status](https://coveralls.io/repos/github/VolumeGraphics/lfspull/badge.svg?branch=main)](https://coveralls.io/github/VolumeGraphics/lfspull?branch=main)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat)](LICENSE-MIT)

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
- '-a / --access-token [TOKEN]' sets the token - can also be set via $ACCESS_TOKEN from env

## Library API guide

Please see our docs.rs for example code and the gherkin tests for how to check the origin of the file.
