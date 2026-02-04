# AV1Converter

A simple tool to convert videos into the AV1 video codec using FFMPEG

## Table conversion

| Resolution | HDR | Dolby Vision | Action                               |
| ---------- | --- | ------------ | ------------------------------------ |
| 1080p      | No  | No           | **1080p SDR AV1**                    |
| 1080p      | Yes | No           | **1080p HDR AV1**                    |
| 1080p      | Yes | Yes          | **1080p HDR AV1**                    |
| 4K         | No  | No           | **4K SDR AV1**                       |
| 4K         | Yes | No           | **4K HDR AV1**                       |
| 4K         | Yes | Yes          | **4K HDR AV1**                       |

## Prerequisites

- ffprobe
- ffmpeg
- libvmaf
