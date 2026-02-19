use crate::analyzer::{HdrType, ResolutionTier, VideoMetadata};
use crate::config::{AppConfig, Encoder};
use crate::tracks::TrackSelection;

/// Parameters for encoding a video file
#[derive(Debug, Clone)]
pub struct EncodingParams {
    pub input: String,
    pub output: String,
    pub encoder: Encoder,
    pub crf: u8,
    pub film_grain: u8,
    pub hdr_type: HdrType,
    pub tracks: TrackSelection,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub svt_preset: u8,
    pub nvenc_preset: String,
}

impl EncodingParams {
    /// Create encoding params from video metadata and config
    pub fn from_metadata(
        input: &str,
        output: &str,
        metadata: &VideoMetadata,
        config: &AppConfig,
        tracks: TrackSelection,
        crf_override: Option<u8>,
    ) -> Self {
        let tier = ResolutionTier::from_dimensions(metadata.width, metadata.height);
        let preset = config.preset_for(&tier, metadata.hdr_type);

        let crf = crf_override.unwrap_or(match config.encoder {
            Encoder::SvtAv1 => preset.crf,
            Encoder::Nvenc => preset.nvenc_cq,
            Encoder::Qsv => preset.qsv_quality,
            Encoder::Amf => preset.amf_quality,
        });

        Self {
            input: input.to_string(),
            output: output.to_string(),
            encoder: config.encoder,
            crf,
            film_grain: preset.film_grain,
            hdr_type: metadata.hdr_type,
            tracks,
            frame_rate_num: metadata.frame_rate_num,
            frame_rate_den: metadata.frame_rate_den,
            svt_preset: config.performance.svt_preset,
            nvenc_preset: config.performance.nvenc_preset.clone(),
        }
    }
}

/// Build FFmpeg arguments for encoding
pub fn build_ffmpeg_args(params: &EncodingParams) -> Vec<String> {
    let mut args = vec![
        "-y".to_string(),
        "-nostdin".to_string(),
        "-i".to_string(),
        params.input.clone(),
        "-map".to_string(),
        "0:v:0".to_string(),
    ];

    // Track mapping
    if params.tracks.audio_indices.is_empty() && params.tracks.subtitle_indices.is_empty() {
        args.extend(["-map".to_string(), "0:a?".to_string()]);
        args.extend(["-map".to_string(), "0:s?".to_string()]);
    } else {
        for idx in &params.tracks.audio_indices {
            args.extend(["-map".to_string(), format!("0:a:{}", idx)]);
        }
        for idx in &params.tracks.subtitle_indices {
            args.extend(["-map".to_string(), format!("0:s:{}", idx)]);
        }
    }

    // Video encoder
    args.extend(["-c:v".to_string(), params.encoder.ffmpeg_name().to_string()]);

    // Build video filter chain (explicit filter graph is more robust than -pix_fmt auto-insertion)
    let vf = build_video_filter(params.hdr_type);
    args.extend(["-vf".to_string(), vf]);

    // Explicit frame rate preservation
    if params.frame_rate_num > 0 && params.frame_rate_den > 0 {
        args.extend([
            "-r".to_string(),
            format!("{}/{}", params.frame_rate_num, params.frame_rate_den),
        ]);
    }

    // Copy audio and subtitles
    args.extend([
        "-c:a".to_string(),
        "copy".to_string(),
        "-c:s".to_string(),
        "copy".to_string(),
    ]);

    // Encoder-specific quality parameters
    args.extend(get_quality_params(params));

    // HDR/color parameters (metadata only, filter is handled above)
    match params.hdr_type {
        HdrType::DolbyVision => args.extend(get_dolby_vision_color_params()),
        HdrType::Pq => args.extend(get_pq_params()),
        HdrType::Hlg => args.extend(get_hlg_params()),
        HdrType::Sdr => {}
    }

    args.push(params.output.clone());
    args
}

/// Get encoder-specific quality parameters
fn get_quality_params(params: &EncodingParams) -> Vec<String> {
    match params.encoder {
        Encoder::SvtAv1 => get_svtav1_params(params),
        Encoder::Nvenc => get_nvenc_params(params),
        Encoder::Qsv => get_qsv_params(params),
        Encoder::Amf => get_amf_params(params),
    }
}

fn get_svtav1_params(params: &EncodingParams) -> Vec<String> {
    let svt_params = if params.film_grain > 0 {
        format!(
            "tune=0:film-grain={}:film-grain-denoise=1:enable-overlays=1:scd=1",
            params.film_grain
        )
    } else {
        "tune=0:film-grain=0:enable-overlays=1:scd=1:enable-tf=1".to_string()
    };

    vec![
        "-crf".to_string(),
        params.crf.to_string(),
        "-preset".to_string(),
        params.svt_preset.to_string(),
        "-svtav1-params".to_string(),
        svt_params,
    ]
}

fn get_nvenc_params(params: &EncodingParams) -> Vec<String> {
    let lookahead = if params.crf <= 23 { "48" } else { "32" };

    vec![
        "-cq".to_string(),
        params.crf.to_string(),
        "-preset".to_string(),
        params.nvenc_preset.clone(),
        "-tune".to_string(),
        "hq".to_string(),
        "-multipass".to_string(),
        "fullres".to_string(),
        "-rc-lookahead".to_string(),
        lookahead.to_string(),
        "-spatial-aq".to_string(),
        "1".to_string(),
        "-temporal-aq".to_string(),
        "1".to_string(),
    ]
}

fn get_qsv_params(params: &EncodingParams) -> Vec<String> {
    vec![
        "-global_quality".to_string(),
        params.crf.to_string(),
        "-preset".to_string(),
        "veryslow".to_string(),
        "-look_ahead".to_string(),
        "1".to_string(),
        "-look_ahead_depth".to_string(),
        "40".to_string(),
    ]
}

fn get_amf_params(params: &EncodingParams) -> Vec<String> {
    vec![
        "-quality".to_string(),
        params.crf.to_string(),
        "-usage".to_string(),
        "transcoding".to_string(),
        "-rc".to_string(),
        "cqp".to_string(),
    ]
}

fn get_pq_params() -> Vec<String> {
    vec![
        "-color_primaries".to_string(),
        "bt2020".to_string(),
        "-color_trc".to_string(),
        "smpte2084".to_string(),
        "-colorspace".to_string(),
        "bt2020nc".to_string(),
        "-map_metadata".to_string(),
        "0".to_string(),
    ]
}

fn get_hlg_params() -> Vec<String> {
    vec![
        "-color_primaries".to_string(),
        "bt2020".to_string(),
        "-color_trc".to_string(),
        "arib-std-b67".to_string(),
        "-colorspace".to_string(),
        "bt2020nc".to_string(),
        "-map_metadata".to_string(),
        "0".to_string(),
    ]
}

/// Build the video filter chain for format conversion and HDR metadata
fn build_video_filter(hdr_type: HdrType) -> String {
    let mut filters = vec!["format=yuv420p10le".to_string()];

    if hdr_type == HdrType::DolbyVision {
        filters.push(
            "setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc=smpte2084".to_string(),
        );
    }

    filters.join(",")
}

/// Dolby Vision color metadata parameters (filter is handled in build_video_filter)
fn get_dolby_vision_color_params() -> Vec<String> {
    vec![
        "-color_primaries".to_string(),
        "bt2020".to_string(),
        "-color_trc".to_string(),
        "smpte2084".to_string(),
        "-colorspace".to_string(),
        "bt2020nc".to_string(),
    ]
}
