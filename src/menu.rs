// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    theme,
    widget::menu::{self, key_bind::KeyBind, ItemHeight, ItemWidth, MenuBar},
    Element,
};
use std::collections::HashMap;

use crate::{fl, Action, Config, Message};

pub fn menu_bar<'a>(config: &Config, key_binds: &HashMap<KeyBind, Action>) -> Element<'a, Message> {
    let mut recent_items = Vec::new();

    MenuBar::new(vec![menu::Tree::with_children(
        menu::root(fl!("file")),
        menu::items(
            key_binds,
            vec![
                menu::Item::Button(fl!("open-media"), Action::FileOpen),
                menu::Item::Folder(fl!("open-recent-media"), recent_items),
                menu::Item::Button(fl!("close-file"), Action::FileClose),
                menu::Item::Divider,
                menu::Item::Button(fl!("quit"), Action::WindowClose),
            ],
        ),
    )])
    .item_height(ItemHeight::Dynamic(40))
    .item_width(ItemWidth::Uniform(240))
    .spacing(theme::active().cosmic().spacing.space_xxxs.into())
    .into()
}
