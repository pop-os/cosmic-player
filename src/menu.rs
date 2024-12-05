// SPDX-License-Identifier: GPL-3.0-only

use cosmic::widget::menu::key_bind::KeyBind;
use cosmic::widget::menu::{items as menu_items, root as menu_root, Item as MenuItem};
use cosmic::{
    widget::menu::{ItemHeight, ItemWidth, MenuBar, Tree as MenuTree},
    Element,
};
use std::collections::HashMap;

use crate::{fl, Action, Config, Message};

pub fn menu_bar<'a>(config: &Config, key_binds: &HashMap<KeyBind, Action>) -> Element<'a, Message> {
    let mut recent_items = Vec::new();

    MenuBar::new(vec![MenuTree::with_children(
        menu_root(fl!("file")),
        menu_items(
            key_binds,
            vec![
                MenuItem::Button(fl!("open-media"), Action::FileOpen),
                MenuItem::Folder(fl!("open-recent-media"), recent_items),
                MenuItem::Button(fl!("close-file"), Action::FileClose),
                MenuItem::Divider,
                MenuItem::Button(fl!("quit"), Action::WindowClose),
            ],
        ),
    )])
    .item_height(ItemHeight::Dynamic(40))
    .item_width(ItemWidth::Uniform(240))
    .spacing(4.0)
    .into()
}
