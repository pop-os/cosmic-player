use cosmic::iced_core::image::Data;
use iced_video_player::Position;
use image::{DynamicImage, ImageFormat, RgbaImage};
use std::{error::Error, num::NonZero, path::Path, time::Duration};
use url::Url;

use super::video;

pub fn main(
    input: &Url,
    output: &Path,
    size_opt: Option<(u32, u32)>,
) -> Result<(), Box<dyn Error>> {
    let mut image = {
        let thumbnails = {
            let mut video = match video::new_video(input, video::VideoSettings { mute: true }) {
                Ok(ok) => ok,
                Err(_err) => return Err(Into::into(format!("missing required plugin"))),
            };

            let duration = video.duration();
            //TODO: how best to decide time?
            let position = if duration.as_secs_f64() < 20.0 {
                // If less than 20 seconds, divide duration by 2
                Position::Time(duration / 2)
            } else {
                // If more than 20 seconds, thumbnail at 10 seconds
                Position::Time(Duration::new(10, 0))
            };
            video.thumbnails([position], NonZero::new(1).unwrap())?
        };
        //TODO: do not require clone of pixels data
        match thumbnails[0].data() {
            Data::Rgba {
                width,
                height,
                pixels,
            } => RgbaImage::from_raw(*width, *height, pixels.to_vec())
                .map(DynamicImage::ImageRgba8)
                .ok_or_else(|| format!("failed to convert thumbnail")),
            _ => Err(format!("unsupported thumbnail handle {:?}", thumbnails[0])),
        }
    }?;

    if let Some((width, height)) = size_opt {
        image = image.thumbnail(width, height);
    }

    image.save_with_format(output, ImageFormat::Png)?;

    Ok(())
}
