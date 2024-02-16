// SPDX-License-Identifier: GPL-3.0-only

use std::{fmt, iter::FusedIterator, str::FromStr};

use ffmpeg_next::ffi::{av_hwdevice_iterate_types, AVHWDeviceType};
use serde::{
    de::{value::Error as DeError, Error as DeErrorTrait, Unexpected},
    Deserialize, Serialize,
};

/// Delegate type for [`ffmpeg_next::ffi::AVHWDeviceType`] for configs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HWDeviceType {
    None,
    /// Compute Unified Device Architecture
    /// Nvidia only.
    /// https://developer.nvidia.com/video-codec-sdk
    Cuda,
    /// Direct3D 11 Video API
    /// https://learn.microsoft.com/en-us/windows/win32/medfound/direct3d-11-video-apis
    D3d11va,
    /// Direct3D 12 Video API
    /// https://learn.microsoft.com/en-us/windows/win32/medfound/direct3d-12-video-overview
    D3d12va,
    /// DirectX Video Acceleration 2.0
    /// https://learn.microsoft.com/en-us/windows/win32/medfound/about-dxva-2-0
    Dxva2,
    /// Direct Rendering Manager
    /// https://dri.freedesktop.org/wiki/DRM/
    Drm,
    /// MediaCodec
    /// Android only
    /// https://developer.android.com/reference/android/media/MediaCodec
    MediaCodec,
    /// OpenCL
    /// Only used in filters
    /// https://www.khronos.org/opencl/
    OpenCl,
    /// Intel Quick Sync Video
    /// https://www.intel.com/content/www/us/en/developer/tools/vpl/overview.html
    Qsv,
    /// Video Acceleration API
    /// https://www.intel.com/content/www/us/en/developer/articles/technical/linuxmedia-vaapi.html
    Vaapi,
    /// Video Decode and Presentation API for Unix
    /// https://www.freedesktop.org/wiki/Software/VDPAU/
    Vdpau,
    /// Video Toolbox
    /// https://developer.apple.com/documentation/videotoolbox
    VideoToolbox,
    /// Vulkan
    Vulkan,
}

impl HWDeviceType {
    /// Hardware device names for user facing interfaces (logging, configs).
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Cuda => "CUDA",
            Self::Dxva2 => "DirectX Video Acceleration 2.0",
            Self::D3d11va => "DirectX 11 Video Acceleration",
            Self::D3d12va => "DirectX 12 Video Acceleration",
            Self::Drm => "Direct Rendering Manager (DRM)",
            Self::MediaCodec => "MediaCodec",
            Self::OpenCl => "OpenCL",
            Self::Qsv => "Intel Quick Video Sync",
            Self::Vaapi => "VA-API",
            Self::Vdpau => "VDPAU",
            Self::VideoToolbox => "VideoToolbox",
            Self::Vulkan => "Vulkan",
        }
    }

    /// Short name for CLI arguments
    pub const fn short_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Cuda => "cuda",
            Self::Dxva2 => "dxva2",
            Self::D3d11va => "d3d11va",
            Self::D3d12va => "d3d12va",
            Self::Drm => "drm",
            Self::MediaCodec => "mediacodec",
            Self::OpenCl => "opencl",
            Self::Qsv => "qsv",
            Self::Vaapi => "vaapi",
            Self::Vdpau => "vdpau",
            Self::VideoToolbox => "videotoolbox",
            Self::Vulkan => "vulkan",
        }
    }

    /// System's supported hardware decoders
    pub fn supported_devices() -> SupportedDeviceIter {
        SupportedDeviceIter::default()
    }
}

impl FromStr for HWDeviceType {
    type Err = DeError;

    // av_hwdevice_find_type_by_name returns None for invalid device type names, but this type
    // is used for deserializing configs (etc.) so the error is preserved.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "cuda" => Ok(Self::Cuda),
            "dxva2" => Ok(Self::Dxva2),
            "d3d11va" => Ok(Self::D3d11va),
            "d3d12va" => Ok(Self::D3d12va),
            "drm" => Ok(Self::Drm),
            "mediacodec" => Ok(Self::MediaCodec),
            "opencl" => Ok(Self::OpenCl),
            "qsv" => Ok(Self::Qsv),
            "vaapi" => Ok(Self::Vaapi),
            "vdpau" => Ok(Self::Vdpau),
            "videotoolbox" => Ok(Self::VideoToolbox),
            "vulkan" => Ok(Self::Vulkan),
            _ => Err(DeError::invalid_value(
                Unexpected::Str(s),
                &"valid hardware decoder",
            )),
        }
    }
}

impl fmt::Display for HWDeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl From<AVHWDeviceType> for HWDeviceType {
    fn from(value: AVHWDeviceType) -> Self {
        match value {
            AVHWDeviceType::AV_HWDEVICE_TYPE_NONE => Self::None,
            AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA => Self::Cuda,
            AVHWDeviceType::AV_HWDEVICE_TYPE_DXVA2 => Self::Dxva2,
            AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA => Self::D3d11va,
            // This variant exists in ffmpeg's C lib but not in Rust's crate yet.
            // AVHWDeviceType::AV_HWDEVICE_TYPE_D3D12VA => Self::D3d12va
            AVHWDeviceType::AV_HWDEVICE_TYPE_DRM => Self::Drm,
            AVHWDeviceType::AV_HWDEVICE_TYPE_MEDIACODEC => Self::MediaCodec,
            AVHWDeviceType::AV_HWDEVICE_TYPE_OPENCL => Self::OpenCl,
            AVHWDeviceType::AV_HWDEVICE_TYPE_QSV => Self::Qsv,
            AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI => Self::Vaapi,
            AVHWDeviceType::AV_HWDEVICE_TYPE_VDPAU => Self::Vdpau,
            AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX => Self::VideoToolbox,
            AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN => Self::Vulkan,
        }
    }
}

impl Default for HWDeviceType {
    fn default() -> Self {
        Self::Vaapi
    }
}

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
    type Item = HWDeviceType;

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
        for decoder in HWDeviceType::supported_devices() {
            black_box(decoder);
        }

        let _decoders: Vec<_> = black_box(HWDeviceType::supported_devices().collect());
    }
}
