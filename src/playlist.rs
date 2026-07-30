// SPDX-License-Identifier: GPL-3.0-only

use std::io;
use std::path::{Path, PathBuf};
use url::Url;

/// The location of a media entry in a playlist.
#[derive(Clone, Debug)]
pub enum PlaylistPath {
    /// A local file path.
    File(PathBuf),
    /// A network URL (http, https, rtsp, etc.).
    Url(Url),
}

/// A single entry in a playlist, optionally with a title from #EXTINF.
#[derive(Clone, Debug)]
pub struct PlaylistEntry {
    /// Optional title from #EXTINF directive.
    pub title: Option<String>,
    /// The media location.
    pub path: PlaylistPath,
}

/// Check if a path has an m3u or m3u8 extension (case-insensitive).
pub fn is_playlist(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("m3u") | Some("m3u8")
    )
}

/// Parse an m3u/m3u8 playlist file.
///
/// Supports:
/// - `#EXTM3U` header (optional, ignored)
/// - `#EXTINF:duration,title` directives (title is extracted)
/// - Local file paths (absolute and relative to the playlist file)
/// - URLs (http, https, rtsp, rtmp, file://)
/// - Other `#` directives are ignored
/// - UTF-8 BOM is stripped if present
pub fn parse_m3u(path: &Path) -> io::Result<Vec<PlaylistEntry>> {
    let content = std::fs::read_to_string(path)?;
    // Strip UTF-8 BOM if present
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let mut entries = Vec::new();
    let mut current_title: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            // Format: #EXTINF:duration,title
            if let Some(comma_pos) = rest.find(',') {
                current_title = Some(rest[comma_pos + 1..].trim().to_string());
            }
            continue;
        }

        if line.starts_with('#') {
            // Other directive (EXTM3U, EXTGRP, EXT-X-*, etc.) - skip
            continue;
        }

        // This is a media path or URL
        let playlist_path = if line.starts_with("http://")
            || line.starts_with("https://")
            || line.starts_with("rtsp://")
            || line.starts_with("rtmp://")
            || line.starts_with("file://")
        {
            match Url::parse(line) {
                Ok(url) => {
                    if url.scheme() == "file" {
                        // Convert file:// URL to local path
                        match url.to_file_path() {
                            Ok(file_path) => PlaylistPath::File(file_path),
                            Err(()) => {
                                log::warn!("failed to convert file URL to path: {}", line);
                                current_title = None;
                                continue;
                            }
                        }
                    } else {
                        PlaylistPath::Url(url)
                    }
                }
                Err(err) => {
                    log::warn!("failed to parse URL '{}': {}", line, err);
                    current_title = None;
                    continue;
                }
            }
        } else {
            // Local file path (absolute or relative)
            let entry_path = if Path::new(line).is_absolute() {
                PathBuf::from(line)
            } else {
                parent.join(line)
            };
            PlaylistPath::File(entry_path)
        };

        entries.push(PlaylistEntry {
            title: current_title.take(),
            path: playlist_path,
        });
    }

    Ok(entries)
}
