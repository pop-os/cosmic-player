use cosmic::{iced::keyboard::Key, iced_core::keyboard::key::Named};
use std::collections::HashMap;

use crate::Action;

pub use cosmic::widget::menu::key_bind::{KeyBind, Modifier};

//TODO: load from config
pub fn key_binds() -> HashMap<KeyBind, Action> {
    let mut key_binds = HashMap::new();

    macro_rules! bind {
        ([$($modifier:ident),* $(,)?], $key:expr, $action:ident) => {{
            key_binds.insert(
                KeyBind {
                    modifiers: vec![$(Modifier::$modifier),*],
                    key: $key,
                },
                Action::$action,
            );
        }};
    }

    //TODO: key bindings
    bind!([], Key::Character("f".into()), Fullscreen);
    bind!([Alt], Key::Named(Named::Enter), Fullscreen);
    bind!([], Key::Named(Named::Space), PlayPause);
    bind!([], Key::Named(Named::ArrowLeft), SeekBackward);
    bind!([], Key::Named(Named::ArrowRight), SeekForward);

    key_binds
}
