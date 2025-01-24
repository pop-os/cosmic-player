// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    theme,
    widget::menu::{self, key_bind::KeyBind, ItemHeight, ItemWidth, MenuBar},
    Element,
};
use std::collections::HashMap;

use crate::{fl, Action, Config, ConfigState, Message};

pub fn menu_bar<'a>(
    config: &Config,
    config_state: &ConfigState,
    key_binds: &HashMap<KeyBind, Action>,
) -> Element<'a, Message> {
    let home_dir_opt = dirs::home_dir();
    let format_path = |url: &url::Url| -> String {
        match url.to_file_path() {
            Ok(path) => {
                if let Some(home_dir) = &home_dir_opt {
                    if let Ok(part) = path.strip_prefix(home_dir) {
                        return format!("~/{}", part.display());
                    }
                }
                path.display().to_string()
            }
            Err(()) => url.to_string(),
        }
    };

    let mut recent_files = Vec::with_capacity(config_state.recent_files.len());
    for (i, path) in config_state.recent_files.iter().enumerate() {
        recent_files.push(menu::Item::Button(
            format_path(path),
            Action::FileOpenRecent(i),
        ));
    }

    let mut recent_folders = Vec::with_capacity(config_state.recent_folders.len());
    for (i, path) in config_state.recent_folders.iter().enumerate() {
        recent_folders.push(menu::Item::Button(
            format_path(path),
            Action::FolderOpenRecent(i),
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
                /*TODO: folders
                menu::Item::Button(fl!("open-media-folder"), Action::FolderOpen),
                menu::Item::Folder(fl!("open-recent-media-folder"), recent_folders),
                menu::Item::Folder(fl!("close-media-folder"), close_folders),
                menu::Item::Divider,
                */
                menu::Item::Button(fl!("quit"), Action::WindowClose),
            ],
        ),
    )])
    .item_height(ItemHeight::Dynamic(40))
    .item_width(ItemWidth::Uniform(320))
    .spacing(theme::active().cosmic().spacing.space_xxxs.into())
    .into()
}
