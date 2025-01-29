// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry},
    theme,
};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::PathBuf};

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
#[serde(default)]
pub struct Config {
    pub app_theme: AppTheme,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app_theme: AppTheme::System,
        }
    }
}

#[derive(Clone, CosmicConfigEntry, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigState {
    pub recent_files: VecDeque<url::Url>,
    pub recent_projects: VecDeque<PathBuf>,
}

impl Default for ConfigState {
    fn default() -> Self {
        Self {
            recent_files: VecDeque::new(),
            recent_projects: VecDeque::new(),
        }
    }
}
