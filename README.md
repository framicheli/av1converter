# AV1Converter

A simple tool to convert videos into the AV1 video codec

## Analyze video

```bash
ffprobe -v error \
  -select_streams v:0 \
  -show_entries stream=width,height,pix_fmt,color_primaries,color_transfer,color_space,side_data_list \
  -of json \
  input.mkv
```

## Table conversion

| Resolution | HDR | Dolby Vision | Action                               |
| ---------- | --- | ------------ | ------------------------------------ |
| 1080p      | No  | No           | **1080p SDR AV1**                    |
| 1080p      | Yes | No           | **1080p HDR AV1**                    |
| 1080p      | Yes | Yes          | **DON'T CONVERT**                    |
| 4K         | No  | No           | **4K SDR AV1**                       |
| 4K         | Yes | No           | **4K HDR AV1**                       |
| 4K         | Yes | Yes          | **DON'T CONVERT**                    |


## HD 1080p

```bash
ffmpeg -y -i input.mkv \
  -map 0:v:0 -map '0:a?' -map '0:s?' \
  -c:v libsvtav1 \
  -preset 4 \
  -crf 28 \
  -pix_fmt yuv420p10le \
  -svtav1-params tune=0:film-grain=0 \
  -c:a copy \
  -c:s copy \
  output_av1.mkv
```

## HD 1080p HDR

```bash
ffmpeg -y -i input.mkv \
  -map 0:v:0 -map '0:a?' -map '0:s?' \
  -c:v libsvtav1 \
  -preset 4 \
  -crf 29 \
  -pix_fmt yuv420p10le \
  -svtav1-params tune=0:film-grain=1 \
  -color_primaries bt2020 \
  -color_trc smpte2084 \
  -colorspace bt2020nc \
  -c:a copy \
  -c:s copy \
  output_1080p_hdr_av1.mkv
```

## 4K

```bash
ffmpeg -y -i input.mkv \
  -map 0:v:0 -map '0:a?' -map '0:s?' \
  -c:v libsvtav1 \
  -preset 4 \
  -crf 30 \
  -pix_fmt yuv420p10le \
  -svtav1-params tune=0:film-grain=1 \
  -c:a copy \
  -c:s copy \
  output_4k_av1.mkv

```

## 4K HDR

```bash
ffmpeg -y -i input.mkv \
  -map 0:v:0 -map '0:a?' -map '0:s?' \
  -c:v libsvtav1 \
  -preset 4 \
  -crf 30 \
  -pix_fmt yuv420p10le \
  -svtav1-params tune=0:film-grain=1 \
  -color_primaries bt2020 \
  -color_trc smpte2084 \
  -colorspace bt2020nc \
  -c:a copy \
  -c:s copy \
  output_4k_hdr_av1.mkv
```

## Evaluate
```bash
ffmpeg \
  -i "original.mkv" \
  -i "film_AV1.mkv" \
  -lavfi "[0:v]format=yuv420p[ref];[1:v]format=yuv420p[dist];[ref][dist]libvmaf" \
  -f null -
```
