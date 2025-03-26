// Copyright 2024 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use std::{fs, io};

use clap_lex::RawArgs;
use log::warn;
use url::Url;

pub fn parse() -> Arguments {
    let raw_args = RawArgs::from_args();
    let mut cursor = raw_args.cursor();
    let mut arguments = Arguments::default();
    let mut urls = Vec::new();

    // Parse the arguments
    while let Some(arg) = raw_args.next(&mut cursor) {
        if let Some(mut shorts) = arg.to_short() {
            while let Some(short) = shorts.next_flag() {
                match short {
                    Ok('h') => print_help(),
                    Ok('V') => print_version(),
                    Ok(c) => warn!("unexpected flag: -{c}"),
                    Err(os_str) => warn!("unexpected flag: -{}", os_str.to_string_lossy()),
                }
            }
        } else if let Some((long, _opt_value)) = arg.to_long() {
            match long {
                Ok("help") => print_help(),
                Ok("version") => print_version(),
                _ => warn!("unexpected flag: {}", arg.display()),
            }
        } else {
            // Freestanding arguments are treated as URLs
            match arg.to_value().ok().map(Source::try_from) {
                Some(Ok(source)) => urls.push(source.0),
                Some(Err(why)) => {
                    warn!("{}: not a valid URL: {}", arg.display(), why)
                }
                None => {
                    warn!("{}: not a valid string", arg.display())
                }
            }
        }
    }

    if urls.len() > 1 {
        arguments.urls = Some(urls);
    } else {
        urls.truncate(1);
        arguments.url_opt = urls.pop();
    }

    arguments
}

#[derive(Debug, Default)]
pub struct Arguments {
    /// Files or directory URLs to play
    pub urls: Option<Vec<Url>>,
    /// Single URL only
    pub url_opt: Option<Url>,
}

// #[derive(Debug)]
// pub enum Source {
//     File(Url),
//     Directory(Url),
//     // TODO: GStreamer handles streaming out of the box
//     Other(Url),
// }

struct Source(Url);

impl TryFrom<&str> for Source {
    type Error = io::Error;

    fn try_from(arg: &str) -> Result<Self, Self::Error> {
        match url::Url::parse(arg) {
            Ok(url) => Ok(Source(url)),
            Err(_) => match fs::canonicalize(arg) {
                Ok(path) => {
                    match Url::from_file_path(&path).or_else(|_| Url::from_directory_path(&path)) {
                        Ok(url) => Ok(Source(url)),
                        Err(()) => {
                            warn!("failed to parse path {:?}", path);
                            Err(io::Error::other("Invalid URL and path"))
                        }
                    }
                }
                Err(err) => {
                    warn!("failed to parse argument {:?}: {}", arg, err);
                    Err(err)
                }
            },
        }
    }
}

#[cold]
pub fn print_help() -> ! {
    let version = env!("CARGO_PKG_VERSION");
    let git_rev = env!("VERGEN_GIT_SHA");

    println!(
        r#"cosmic-player {version} (git commit {git_rev})
System76 <info@system76.com>

Designed for the COSMIC™ desktop environment, cosmic-player is a
libcosmic-based multimedia player for music and videos.

Project home page: https://github.com/pop-os/cosmic-player

Options:
  -h, --help     Show this message
  -V, --version  Show the version of cosmic-player"#
    );

    std::process::exit(0);
}

#[cold]
pub fn print_version() -> ! {
    println!(
        "cosmic-player {} (git commit {})",
        env!("CARGO_PKG_VERSION"),
        env!("VERGEN_GIT_SHA")
    );

    std::process::exit(0);
}
