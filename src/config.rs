// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry},
    theme,
};
use lexopt::prelude::*;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process};

use crate::hardware::DeviceType;

pub const CONFIG_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AppTheme {
    Dark,
    Light,
    System,
}

impl AppTheme {
    pub fn theme(&self) -> theme::Theme {
        match self {
            Self::Dark => theme::Theme::dark(),
            Self::Light => theme::Theme::light(),
            Self::System => theme::system_preference(),
        }
    }
}

#[derive(Clone, CosmicConfigEntry, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Config {
    pub app_theme: AppTheme,
    pub hw_decoder: DeviceType,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app_theme: AppTheme::System,
            hw_decoder: DeviceType::default(),
        }
    }
}

impl Config {
    pub fn with_args(&mut self, args: &mut Args) {
        if let Some(decoder) = args.decoder {
            self.hw_decoder = decoder;
        }
    }
}

pub struct Args {
    pub paths: Vec<PathBuf>,
    pub decoder: Option<DeviceType>,
}

impl Args {
    pub fn parse_args() -> Result<Self, lexopt::Error> {
        let mut paths = Vec::new();
        let mut decoder = None;

        let mut parser = lexopt::Parser::from_env();
        while let Some(arg) = parser.next()? {
            match arg {
                Long("list-hwdec") => {
                    println!("Supported hardware decoders:");
                    for hwdec in DeviceType::supported_devices() {
                        println!("\t* [{}] {hwdec}", hwdec.short_name());
                    }
                    process::exit(0);
                }
                Long("hwdec") => {
                    decoder = Some(parser.value()?.parse()?);
                }
                Value(path) => {
                    let path = path.parse()?;
                    paths.push(path);
                }
                _ => return Err(arg.unexpected()),
            }
        }

        if paths.is_empty() {
            return Err(lexopt::Error::MissingValue {
                option: Some("missing video path".into()),
            });
        }

        Ok(Self { paths, decoder })
    }
}
