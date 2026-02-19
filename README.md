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

## Prerequisites

- ffprobe
- ffmpeg
- libvmaf
