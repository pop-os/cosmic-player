// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    theme,
    widget::menu::{self, key_bind::KeyBind, ItemHeight, ItemWidth, MenuBar},
    Element,
};
use std::{collections::HashMap, path::PathBuf};

use crate::{fl, Action, Config, ConfigState, Message};

pub fn menu_bar<'a>(
    config: &Config,
    config_state: &ConfigState,
    key_binds: &HashMap<KeyBind, Action>,
    projects: &Vec<(String, PathBuf)>,
) -> Element<'a, Message> {
    let home_dir_opt = dirs::home_dir();
    let format_path = |path: &PathBuf| -> String {
        if let Some(home_dir) = &home_dir_opt {
            if let Ok(part) = path.strip_prefix(home_dir) {
                return format!("~/{}", part.display());
            }
        }
        path.display().to_string()
    };
    let format_url = |url: &url::Url| -> String {
        match url.to_file_path() {
            Ok(path) => format_path(&path),
            Err(()) => url.to_string(),
        }
    };

    let mut recent_files = Vec::with_capacity(config_state.recent_files.len());
    for (i, path) in config_state.recent_files.iter().enumerate() {
        recent_files.push(menu::Item::Button(
            format_url(path),
            Action::FileOpenRecent(i),
        ));
    }

    let mut recent_projects = Vec::with_capacity(config_state.recent_projects.len());
    for (i, path) in config_state.recent_projects.iter().enumerate() {
        recent_projects.push(menu::Item::Button(
            format_path(path),
            Action::FolderOpenRecent(i),
        ));
    }

    let mut close_projects = Vec::with_capacity(projects.len());
    for (folder_i, (name, _path)) in projects.iter().enumerate() {
        close_projects.push(menu::Item::Button(
            name.clone(),
            Action::FolderClose(folder_i),
        ));
    }

    MenuBar::new(vec![menu::Tree::with_children(
        menu::root(fl!("file")),
        menu::items(
            key_binds,
            vec![
                menu::Item::Button(fl!("open-media"), Action::FileOpen),
                menu::Item::Folder(fl!("open-recent-media"), recent_files),
                menu::Item::Button(fl!("close-file"), Action::FileClose),
                menu::Item::Divider,
                menu::Item::Button(fl!("open-media-folder"), Action::FolderOpen),
                menu::Item::Folder(fl!("open-recent-media-folder"), recent_projects),
                menu::Item::Folder(fl!("close-media-folder"), close_projects),
                menu::Item::Divider,
                menu::Item::Button(fl!("quit"), Action::WindowClose),
            ],
        ),
    )])
    .item_height(ItemHeight::Dynamic(40))
    .item_width(ItemWidth::Uniform(320))
    .spacing(theme::active().cosmic().spacing.space_xxxs.into())
    .into()
}
