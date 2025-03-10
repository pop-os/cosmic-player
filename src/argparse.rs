// Copyright 2024 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

use std::{fs, io};

use log::warn;
use url::Url;

#[derive(Debug, Default)]
pub struct Arguments {
    /// Files or directory URLs to play
    pub urls: Option<Vec<Url>>,
    /// Single URL only
    pub url_opt: Option<Url>,
}

impl Arguments {
    pub fn from_args() -> Result<Self, pico_args::Error> {
        let mut parser = pico_args::Arguments::from_env();

        // Freestanding arguments are treated as URLs
        let urls: Vec<Url> = std::iter::from_fn(|| {
            parser
                .opt_free_from_fn(|arg| Source::try_from(arg))
                .ok()
                .flatten()
        })
        .map(|source| source.0)
        .collect();

        let remainder = parser.finish();
        for arg in remainder {
            warn!("Unused argument: {arg:?}");
        }

        if urls.len() > 1 {
            Ok(Arguments {
                urls: Some(urls),
                ..Default::default()
            })
        } else {
            Ok(Arguments {
                url_opt: urls.into_iter().next(),
                ..Default::default()
            })
        }
    }
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
