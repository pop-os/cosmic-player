use cosmic::iced::keyboard::Key;
use cosmic::iced::keyboard::key::Named;
use ordered_float::OrderedFloat as F;
use std::collections::HashMap;

use crate::Action;

pub use cosmic::widget::menu::key_bind::{KeyBind, Modifier};

//TODO: load from config
pub fn key_binds() -> HashMap<KeyBind, Action> {
    let mut key_binds = HashMap::new();

    macro_rules! bind {
        ([$($modifier:ident),* $(,)?], $key:expr, $action:ident $(($($args:expr),*))?) => {{
            key_binds.insert(
                KeyBind {
                    modifiers: vec![$(Modifier::$modifier),*],
                    key: $key,
                },
                Action::$action $(($($args),*))?,
            );
        }};
    }

    //TODO: key bindings
    bind!([], Key::Character("f".into()), Fullscreen);
    bind!([Alt], Key::Named(Named::Enter), Fullscreen);
    bind!([], Key::Character(" ".into()), PlayPause);
    bind!([], Key::Named(Named::ArrowLeft), SeekBackward);
    bind!([], Key::Named(Named::ArrowRight), SeekForward);
    bind!([], Key::Character(".".into()), NextFrame);
    bind!([], Key::Character(",".into()), PreviousFrame);
    bind!([], Key::Character("a".into()), AbRepeat);
    bind!([], Key::Character("m".into()), AudioToggle);
    bind!([], Key::Character("c".into()), TextCodeToggle);
    bind!([], Key::Named(Named::PageUp), PlayPrev);
    bind!([], Key::Named(Named::PageDown), PlayNext);
    bind!([], Key::Named(Named::ArrowUp), ChangeVolume(F(0.02)));
    bind!([], Key::Named(Named::ArrowDown), ChangeVolume(F(-0.02)));
    bind!([Shift], Key::Character(",".into()), ChangeSpeed(F(-0.25)));
    bind!([Shift], Key::Character(".".into()), ChangeSpeed(F(0.25)));

    bind!([], Key::Character("0".into()), Seek(F(0.0)));
    bind!([], Key::Character("1".into()), Seek(F(0.1)));
    bind!([], Key::Character("2".into()), Seek(F(0.2)));
    bind!([], Key::Character("3".into()), Seek(F(0.3)));
    bind!([], Key::Character("4".into()), Seek(F(0.4)));
    bind!([], Key::Character("5".into()), Seek(F(0.5)));
    bind!([], Key::Character("6".into()), Seek(F(0.6)));
    bind!([], Key::Character("7".into()), Seek(F(0.7)));
    bind!([], Key::Character("8".into()), Seek(F(0.8)));
    bind!([], Key::Character("9".into()), Seek(F(0.9)));

    key_binds
}
