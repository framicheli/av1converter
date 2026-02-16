# AV1Converter

A simple tool to convert videos into the AV1 video codec using FFMPEG

## Table conversion

| Resolution | HDR | Dolby Vision | Preset          | VMAF Model         |
| ---------- | --- | ------------ | --------------- | ------------------ |
| 1080p      | No  | No           | **1080p SDR**   | vmaf_v0.6.1        |
| 1080p      | Yes | No           | **1080p HDR**   | vmaf_v0.6.1neg     |
| 1080p      | Yes | Yes          | **1080p DV**    | vmaf_v0.6.1neg     |
| 4K         | No  | No           | **4K SDR**      | vmaf_v0.6.1        |
| 4K         | Yes | No           | **4K HDR**      | vmaf_v0.6.1neg     |
| 4K         | Yes | Yes          | **4K DV**       | vmaf_v0.6.1neg     |

> **Note:** Dolby Vision content uses dedicated encoding presets with lower CRF values
> to preserve DV grading quality. HDR and DV content use the `vmaf_v0.6.1neg` model
> which is trained for HDR content, providing more accurate VMAF scores than the
> default SDR model.

## Prerequisites

- ffprobe
- ffmpeg
- libvmaf
