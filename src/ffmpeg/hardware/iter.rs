// SPDX-License-Identifier: GPL-3.0-only

use std::iter::FusedIterator;

use ffmpeg_next::ffi::{av_hwdevice_iterate_types, AVHWDeviceType};

use super::device_type::DeviceType;

/// Iterator over system's supported hardware decoders.
pub struct SupportedDeviceIter {
    current: AVHWDeviceType,
}

impl Default for SupportedDeviceIter {
    fn default() -> Self {
        // SAFETY: FFmpeg's documentation states that the iterator is delimited by AV_HWDEVICE_TYPE_NONE.
        let current = unsafe { av_hwdevice_iterate_types(AVHWDeviceType::AV_HWDEVICE_TYPE_NONE) };
        Self { current }
    }
}

impl Iterator for SupportedDeviceIter {
    type Item = DeviceType;

    fn next(&mut self) -> Option<Self::Item> {
        // None is a sentinel value that indicates the iterator is exhausted
        if self.current == AVHWDeviceType::AV_HWDEVICE_TYPE_NONE {
            None
        } else {
            let prev = self.current;
            // SAFETY: The docs and examples state that the iterator yields the next value
            // when the previous is passed in.
            self.current = unsafe { av_hwdevice_iterate_types(prev) };

            Some(prev.into())
        }
    }
}

impl FusedIterator for SupportedDeviceIter {}

#[cfg(test)]
mod tests {
    use std::hint::black_box;

    use super::*;

    // The iterator's yielded values aren't important since hardware decoders vary by system
    // This is just a sanity check to ensure the iterator works
    #[test]
    fn supported_device_iter_doesnt_seg_fault() {
        for decoder in DeviceType::supported_devices() {
            black_box(decoder);
        }

        let _decoders: Vec<_> = black_box(DeviceType::supported_devices().collect());
    }
}
