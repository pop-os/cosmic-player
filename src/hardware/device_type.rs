// SPDX-License-Identifier: GPL-3.0-only

use std::{fmt, str::FromStr};

use ffmpeg_next::ffi::AVHWDeviceType;
use serde::{
    de::{value::Error as DeError, Error as DeErrorTrait, Unexpected},
    Deserialize, Serialize,
};

use super::iter::SupportedDeviceIter;

/// Delegate type for [`ffmpeg_next::ffi::AVHWDeviceType`] for configs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceType {
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

impl DeviceType {
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

impl FromStr for DeviceType {
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

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl From<AVHWDeviceType> for DeviceType {
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

impl From<DeviceType> for AVHWDeviceType {
    fn from(value: DeviceType) -> Self {
        match value {
            DeviceType::None => Self::AV_HWDEVICE_TYPE_NONE,
            DeviceType::Cuda => Self::AV_HWDEVICE_TYPE_CUDA,
            DeviceType::D3d11va => Self::AV_HWDEVICE_TYPE_D3D11VA,
            // NOTE: Next FFmpeg release
            DeviceType::D3d12va => Self::AV_HWDEVICE_TYPE_NONE,
            DeviceType::Dxva2 => Self::AV_HWDEVICE_TYPE_DXVA2,
            DeviceType::Drm => Self::AV_HWDEVICE_TYPE_DRM,
            DeviceType::MediaCodec => Self::AV_HWDEVICE_TYPE_MEDIACODEC,
            DeviceType::OpenCl => Self::AV_HWDEVICE_TYPE_OPENCL,
            DeviceType::Qsv => Self::AV_HWDEVICE_TYPE_QSV,
            DeviceType::Vaapi => Self::AV_HWDEVICE_TYPE_VAAPI,
            DeviceType::Vdpau => Self::AV_HWDEVICE_TYPE_VDPAU,
            DeviceType::VideoToolbox => Self::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            DeviceType::Vulkan => Self::AV_HWDEVICE_TYPE_VULKAN,
        }
    }
}

impl Default for DeviceType {
    fn default() -> Self {
        Self::Vaapi
    }
}
