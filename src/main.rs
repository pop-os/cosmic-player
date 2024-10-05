// Copyright 2023 System76 <info@system76.com>
// SPDX-License-Identifier: GPL-3.0-only

mod config;
mod key_bind;
mod localize;

#[cfg(feature = "ffmpeg")]
#[path = "ffmpeg/mod.rs"]
mod app;

#[cfg(feature = "gstreamer")]
#[path = "gstreamer/mod.rs"]
mod app;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::main()
}
