use crate::analyzer::{DvMode, Hdr10StaticMetadata, HdrType, ResolutionTier, VideoMetadata};
use crate::config::{AppConfig, Encoder};
use crate::tracks::{AudioStreamPlan, OpusLayout, OutputTracks};

/// Parameters for encoding a video file
#[derive(Debug, Clone)]
pub struct EncodingParams {
    pub input: String,
    pub output: String,
    pub encoder: Encoder,
    pub crf: u8,
    pub film_grain: u8,
    pub hdr_type: HdrType,
    pub dv_mode: DvMode,
    pub dv_profile: Option<u8>,
    pub hdr10_static: Option<Hdr10StaticMetadata>,
    pub tracks: OutputTracks,
    /// Source frame rate. Not passed to ffmpeg: timestamps pass through.
    #[allow(dead_code)]
    pub frame_rate_num: u32,
    #[allow(dead_code)]
    pub frame_rate_den: u32,
    pub svt_preset: u8,
    pub nvenc_preset: String,
    pub remux_only: bool,
    /// Subtitle codecs in output stream order.
    pub subtitle_codecs: Vec<&'static str>,
}

impl EncodingParams {
    /// Create encoding params from video metadata and config
    #[allow(clippy::too_many_arguments)]
    pub fn from_metadata(
        input: &str,
        output: &str,
        metadata: &VideoMetadata,
        config: &AppConfig,
        tracks: OutputTracks,
        dv_mode: DvMode,
        remux_only: bool,
        subtitle_codecs: Vec<&'static str>,
    ) -> Self {
        let tier = ResolutionTier::from_dimensions(metadata.width, metadata.height);
        let preset = config.preset_for(tier, metadata.hdr_type);

        let crf = match config.encoder {
            Encoder::SvtAv1 => preset.crf,
            Encoder::Nvenc => preset.nvenc_cq,
            Encoder::Qsv => preset.qsv_quality,
            Encoder::Amf => preset.amf_quality,
        };

        // DV passthrough needs an encoder that can write the RPU (SVT-AV1)
        let dv_mode = if config.encoder == Encoder::SvtAv1 {
            dv_mode
        } else {
            DvMode::ToHdr10
        };

        Self {
            input: input.to_string(),
            output: output.to_string(),
            encoder: config.encoder,
            crf,
            film_grain: preset.film_grain,
            hdr_type: metadata.hdr_type,
            dv_mode,
            dv_profile: metadata.dv_profile,
            hdr10_static: metadata.hdr10_static,
            tracks,
            frame_rate_num: metadata.frame_rate_num,
            frame_rate_den: metadata.frame_rate_den,
            svt_preset: config.performance.svt_preset,
            nvenc_preset: config.performance.nvenc_preset.clone(),
            remux_only,
            subtitle_codecs,
        }
    }

    /// Dolby Vision source that keeps its RPU in the AV1 output (profile 10)
    fn keeps_dolby_vision(&self) -> bool {
        self.hdr_type == HdrType::DolbyVision && self.dv_mode == DvMode::KeepDolbyVision
    }

    /// Dolby Vision profile 5 converted to HDR10: the pixels are in Dolby's
    /// IPT color space, so a real tone-mapping pass (libplacebo) is required
    fn needs_dv_tonemap(&self) -> bool {
        self.hdr_type == HdrType::DolbyVision
            && self.dv_mode == DvMode::ToHdr10
            && self.dv_profile == Some(5)
    }
}

/// Build `FFmpeg` arguments for encoding
pub fn build_ffmpeg_args(params: &EncodingParams) -> Vec<String> {
    // Stats go to the -progress file
    let mut args = vec![
        "-y".to_string(),
        "-nostdin".to_string(),
        "-nostats".to_string(),
    ];

    // Profile 5 tone-mapping runs on the GPU via libplacebo (Vulkan)
    if !params.remux_only && params.needs_dv_tonemap() {
        args.extend(["-init_hw_device".to_string(), "vulkan".to_string()]);
    }

    args.extend([
        "-i".to_string(),
        params.input.clone(),
        "-map".to_string(),
        "0:v:0".to_string(),
    ]);

    // Track mapping. An empty selection maps nothing: defaults are already
    // resolved by auto-selection before this point.
    for plan in &params.tracks.audio {
        args.extend(["-map".to_string(), format!("0:a:{}", plan.source_index)]);
    }
    for idx in &params.tracks.subtitle_indices {
        args.extend(["-map".to_string(), format!("0:s:{idx}")]);
    }

    if params.remux_only {
        // Remux mode: the video is already AV1 and is copied as-is. Audio
        // still honours the per-track choice.
        args.extend(["-c:v".to_string(), "copy".to_string()]);
        args.extend(build_audio_args(&params.tracks.audio));
        args.extend(build_subtitle_args(&params.subtitle_codecs));
    } else {
        // Video encoder
        args.extend(["-c:v".to_string(), params.encoder.ffmpeg_name().to_string()]);

        // Build video filter chain (explicit filter graph is more robust than -pix_fmt auto-insertion)
        let vf = build_video_filter(params);
        args.extend(["-vf".to_string(), vf]);

        // Source timestamps pass through unchanged, variable frame rate included.
        args.extend(["-fps_mode".to_string(), "passthrough".to_string()]);

        // Audio follows the per-track choice; subtitles only when the
        // container can hold them
        args.extend(build_audio_args(&params.tracks.audio));
        args.extend(build_subtitle_args(&params.subtitle_codecs));

        // Encoder-specific quality parameters
        args.extend(get_quality_params(params));

        // HDR/color parameters (metadata only, filter is handled above)
        match params.hdr_type {
            HdrType::DolbyVision => args.extend(get_dolby_vision_color_params(params)),
            HdrType::Pq => args.extend(get_pq_params()),
            HdrType::Hlg => args.extend(get_hlg_params()),
            HdrType::Sdr => {}
        }
    }

    // The output is a positional argument; a leading `-` is spelled `./-` so
    // FFmpeg does not read it as an option.
    if params.output.starts_with('-') {
        args.push(format!("./{}", params.output));
    } else {
        args.push(params.output.clone());
    }
    args
}

/// Per-stream audio codec options, in output stream order. Every mapped stream
/// gets an explicit `-c:a:N`, never a global `-c:a` with overrides.
fn build_audio_args(plan: &[AudioStreamPlan]) -> Vec<String> {
    if plan.is_empty() {
        return Vec::new();
    }

    let mut args = Vec::new();
    for (n, stream) in plan.iter().enumerate() {
        match stream.opus_kbps {
            None => args.extend([format!("-c:a:{n}"), "copy".to_string()]),
            Some(kbps) => {
                args.extend([format!("-c:a:{n}"), "libopus".to_string()]);
                args.extend([format!("-b:a:{n}"), format!("{kbps}k")]);
                match stream.layout {
                    OpusLayout::AsIs => {}
                    OpusLayout::Relabel(layout) => args.extend([
                        format!("-filter:a:{n}"),
                        format!("aformat=channel_layouts={layout}"),
                    ]),
                    OpusLayout::Independent => {
                        args.extend([format!("-mapping_family:a:{n}"), "255".to_string()]);
                    }
                }
                // Overrides the source title, which FFmpeg copies by default.
                if let Some(title) = &stream.title {
                    args.extend([format!("-metadata:s:a:{n}"), format!("title={title}")]);
                }
            }
        }
    }
    args
}

fn build_subtitle_args(codecs: &[&str]) -> Vec<String> {
    codecs
        .iter()
        .enumerate()
        .flat_map(|(n, codec)| [format!("-c:s:{n}"), (*codec).to_string()])
        .collect()
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
    let mut svt_params = if params.film_grain > 0 {
        format!(
            "tune=0:film-grain={}:film-grain-denoise=1:enable-overlays=1:scd=1",
            params.film_grain
        )
    } else {
        "tune=0:film-grain=0:enable-overlays=1:scd=1:enable-tf=1".to_string()
    };

    // Attach HDR10 static metadata so PQ output is true HDR10, not bare PQ.
    // Applies to native HDR10 sources and to DV sources (both modes: the
    // HDR10 base of a kept-DV stream benefits from it too).
    if matches!(params.hdr_type, HdrType::Pq | HdrType::DolbyVision)
        && let Some(ref hdr10) = params.hdr10_static
    {
        use std::fmt::Write;
        let _ = write!(
            svt_params,
            ":mastering-display={}",
            hdr10.svt_mastering_display()
        );
        if let Some(cll) = hdr10.svt_content_light() {
            let _ = write!(svt_params, ":content-light={cll}");
        }
    }

    let mut args = vec![
        "-crf".to_string(),
        params.crf.to_string(),
        "-preset".to_string(),
        params.svt_preset.to_string(),
        "-svtav1-params".to_string(),
        svt_params,
    ];

    // DV RPU coding is set explicitly; FFmpeg's libsvtav1 defaults to "auto",
    // which passes the RPU through.
    if params.hdr_type == HdrType::DolbyVision {
        let dovi = if params.keeps_dolby_vision() {
            "1"
        } else {
            "0"
        };
        args.extend(["-dolbyvision".to_string(), dovi.to_string()]);
    }

    args
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

/// Constant-QP rate control. `-quality` on `av1_amf` selects a speed preset
/// and never sets the quantizer.
fn get_amf_params(params: &EncodingParams) -> Vec<String> {
    vec![
        "-rc".to_string(),
        "cqp".to_string(),
        "-qp_i".to_string(),
        params.crf.to_string(),
        "-qp_p".to_string(),
        params.crf.to_string(),
        "-usage".to_string(),
        "transcoding".to_string(),
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
fn build_video_filter(params: &EncodingParams) -> String {
    if params.needs_dv_tonemap() {
        // Profile 5: apply the DV RPU and convert IPT-PQ-c2 to PQ/BT.2020.
        // libplacebo consumes the per-frame DV metadata during conversion.
        return "libplacebo=colorspace=bt2020nc:color_primaries=bt2020:\
                color_trc=smpte2084:format=yuv420p10le"
            .to_string();
    }

    let mut filters = vec!["format=yuv420p10le".to_string()];

    if params.hdr_type == HdrType::DolbyVision {
        // Cross-compatible profiles (7/8) have a PQ/BT.2020 base layer, often
        // left untagged in the source. Keep-DV profile 5 stays in Dolby's own
        // space and is left alone.
        if params.dv_profile != Some(5) {
            filters.push(
                "setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc=smpte2084"
                    .to_string(),
            );
        }
    }

    filters.join(",")
}

/// Dolby Vision container-level color metadata (filter handles frame tags)
fn get_dolby_vision_color_params(params: &EncodingParams) -> Vec<String> {
    // Keep-DV profile 5 output is not BT.2020/PQ — leave tags unspecified
    if params.keeps_dolby_vision() && params.dv_profile == Some(5) {
        return Vec::new();
    }

    vec![
        "-color_primaries".to_string(),
        "bt2020".to_string(),
        "-color_trc".to_string(),
        "smpte2084".to_string(),
        "-colorspace".to_string(),
        "bt2020nc".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::Hdr10StaticMetadata;

    fn dv_params(encoder: Encoder, dv_mode: DvMode, dv_profile: Option<u8>) -> EncodingParams {
        EncodingParams {
            input: "in.mkv".to_string(),
            output: "out.mkv".to_string(),
            encoder,
            crf: 27,
            film_grain: 0,
            hdr_type: HdrType::DolbyVision,
            dv_mode,
            dv_profile,
            hdr10_static: None,
            tracks: OutputTracks::default(),
            frame_rate_num: 24000,
            frame_rate_den: 1001,
            svt_preset: 6,
            nvenc_preset: "p4".to_string(),
            remux_only: false,
            subtitle_codecs: Vec::new(),
        }
    }

    fn arg_after(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    }

    #[test]
    fn keep_dv_enables_rpu_coding() {
        let args = build_ffmpeg_args(&dv_params(
            Encoder::SvtAv1,
            DvMode::KeepDolbyVision,
            Some(8),
        ));
        assert_eq!(arg_after(&args, "-dolbyvision"), Some("1".to_string()));
        // Cross-compatible base layer keeps PQ tags
        assert_eq!(
            arg_after(&args, "-color_trc"),
            Some("smpte2084".to_string())
        );
    }

    #[test]
    fn to_hdr10_disables_rpu_coding() {
        let args = build_ffmpeg_args(&dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8)));
        assert_eq!(arg_after(&args, "-dolbyvision"), Some("0".to_string()));
        assert!(arg_after(&args, "-vf").is_some_and(|vf| vf.contains("setparams")));
    }

    #[test]
    fn profile5_to_hdr10_tone_maps_via_libplacebo() {
        let args = build_ffmpeg_args(&dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(5)));
        assert!(args.contains(&"-init_hw_device".to_string()));
        let vf = arg_after(&args, "-vf").unwrap();
        assert!(vf.contains("libplacebo"));
        assert!(!vf.contains("setparams"));
    }

    #[test]
    fn profile5_keep_dv_leaves_tags_unspecified() {
        let args = build_ffmpeg_args(&dv_params(
            Encoder::SvtAv1,
            DvMode::KeepDolbyVision,
            Some(5),
        ));
        assert_eq!(arg_after(&args, "-dolbyvision"), Some("1".to_string()));
        assert!(!args.contains(&"-color_trc".to_string()));
        assert!(!arg_after(&args, "-vf").unwrap().contains("setparams"));
    }

    #[test]
    fn frame_timestamps_pass_through_unchanged() {
        let args = build_ffmpeg_args(&dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8)));
        assert_eq!(
            arg_after(&args, "-fps_mode"),
            Some("passthrough".to_string())
        );
        assert!(!args.contains(&"-r".to_string()));
    }

    #[test]
    fn amf_quality_sets_the_constant_qp() {
        let args = build_ffmpeg_args(&dv_params(Encoder::Amf, DvMode::ToHdr10, None));
        assert_eq!(arg_after(&args, "-rc"), Some("cqp".to_string()));
        assert_eq!(arg_after(&args, "-qp_i"), Some("27".to_string()));
        assert_eq!(arg_after(&args, "-qp_p"), Some("27".to_string()));
        assert!(!args.contains(&"-quality".to_string()));
    }

    #[test]
    fn hardware_encoder_falls_back_to_hdr10() {
        let metadata = VideoMetadata {
            width: 3840,
            height: 2160,
            hdr_type: HdrType::DolbyVision,
            dv_profile: Some(8),
            hdr10_static: None,
            codec_name: "hevc".to_string(),
            frame_rate_num: 24000,
            frame_rate_den: 1001,
            duration_secs: 60.0,
        };
        let config = AppConfig {
            encoder: Encoder::Nvenc,
            ..AppConfig::default()
        };
        let params = EncodingParams::from_metadata(
            "in.mkv",
            "out.mkv",
            &metadata,
            &config,
            OutputTracks::default(),
            DvMode::KeepDolbyVision,
            false,
            Vec::new(),
        );
        assert_eq!(params.dv_mode, DvMode::ToHdr10);
        let args = build_ffmpeg_args(&params);
        assert!(!args.contains(&"-dolbyvision".to_string()));
    }

    fn audio(source_index: usize, opus_kbps: Option<u32>) -> AudioStreamPlan {
        AudioStreamPlan {
            source_index,
            opus_kbps,
            layout: OpusLayout::AsIs,
            title: None,
        }
    }

    /// Codec options are indexed by output position; `-map` by source index.
    #[test]
    fn mixed_audio_selection_addresses_streams_by_output_position() {
        let mut params = dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8));
        params.tracks = OutputTracks {
            audio: vec![audio(1, None), audio(3, Some(384)), audio(4, None)],
            subtitle_indices: vec![0],
        };
        let args = build_ffmpeg_args(&params);

        // Mapping still uses the source's audio-relative indices...
        let maps: Vec<&String> = args
            .iter()
            .enumerate()
            .filter(|(i, _)| i > &0 && args[i - 1] == "-map")
            .map(|(_, a)| a)
            .collect();
        assert_eq!(maps, vec!["0:v:0", "0:a:1", "0:a:3", "0:a:4", "0:s:0"]);

        // ...while the codec options are indexed by output position.
        assert_eq!(arg_after(&args, "-c:a:0"), Some("copy".to_string()));
        assert_eq!(arg_after(&args, "-c:a:1"), Some("libopus".to_string()));
        assert_eq!(arg_after(&args, "-b:a:1"), Some("384k".to_string()));
        assert_eq!(arg_after(&args, "-c:a:2"), Some("copy".to_string()));

        // Only the transcoded stream carries a bitrate.
        assert!(!args.contains(&"-b:a:0".to_string()));
        // No bare `-c:a`, which would override the per-stream choices.
        assert!(!args.contains(&"-c:a".to_string()));
    }

    /// Unsupported standard mappings use independent streams; no filter may
    /// silently downmix or add channels to make the layout fit.
    #[test]
    fn uncommon_opus_layout_preserves_channels() {
        let mut params = dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8));
        params.tracks = OutputTracks {
            audio: vec![AudioStreamPlan {
                layout: OpusLayout::Independent,
                ..audio(0, Some(384))
            }],
            subtitle_indices: Vec::new(),
        };
        let args = build_ffmpeg_args(&params);
        assert_eq!(
            arg_after(&args, "-mapping_family:a:0").as_deref(),
            Some("255")
        );
        assert!(!args.iter().any(|arg| arg.starts_with("-filter:a:")));
    }

    /// A relabelled layout is filtered, and keeps the standard mapping.
    #[test]
    fn relabelled_layout_keeps_the_standard_mapping() {
        let mut params = dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8));
        params.tracks = OutputTracks {
            audio: vec![AudioStreamPlan {
                layout: OpusLayout::Relabel("5.1"),
                ..audio(0, Some(384))
            }],
            subtitle_indices: Vec::new(),
        };
        let args = build_ffmpeg_args(&params);
        assert_eq!(
            arg_after(&args, "-filter:a:0").as_deref(),
            Some("aformat=channel_layouts=5.1")
        );
        assert!(!args.iter().any(|arg| arg.starts_with("-mapping_family")));
    }

    /// Only a stream with a replacement title gets a `-metadata`.
    #[test]
    fn only_retitled_streams_override_the_source_tag() {
        let mut params = dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8));
        params.tracks = OutputTracks {
            audio: vec![
                audio(0, None),
                AudioStreamPlan {
                    title: Some("Opus 5.1".to_string()),
                    ..audio(1, Some(384))
                },
            ],
            subtitle_indices: Vec::new(),
        };
        let args = build_ffmpeg_args(&params);
        assert_eq!(
            arg_after(&args, "-metadata:s:a:1").as_deref(),
            Some("title=Opus 5.1")
        );
        assert!(!args.contains(&"-metadata:s:a:0".to_string()));
    }

    /// Remuxing copies the video but still honours the audio choice.
    #[test]
    fn remux_transcodes_audio_without_touching_the_video() {
        let mut params = dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8));
        params.remux_only = true;
        params.tracks = OutputTracks {
            audio: vec![audio(0, Some(128))],
            subtitle_indices: Vec::new(),
        };
        let args = build_ffmpeg_args(&params);

        assert_eq!(arg_after(&args, "-c:v"), Some("copy".to_string()));
        assert_eq!(arg_after(&args, "-c:a:0"), Some("libopus".to_string()));
        assert_eq!(arg_after(&args, "-b:a:0"), Some("128k".to_string()));
        // Remux must never grow a video filter or encoder settings.
        assert!(!args.contains(&"-vf".to_string()));
        assert!(!args.contains(&"-crf".to_string()));
    }

    /// Empty means the user excluded every optional track.
    #[test]
    fn empty_selection_maps_no_optional_tracks() {
        let args = build_ffmpeg_args(&dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8)));
        assert!(!args.iter().any(|arg| arg.starts_with("0:a")));
        assert!(!args.iter().any(|arg| arg.starts_with("0:s")));
        assert!(!args.contains(&"-c:a".to_string()));
        assert!(!args.contains(&"-c:a:0".to_string()));
    }

    #[test]
    fn mixed_subtitles_keep_per_stream_codecs() {
        assert_eq!(
            build_subtitle_args(&["srt", "copy"]),
            ["-c:s:0", "srt", "-c:s:1", "copy"]
        );
    }

    #[test]
    fn hdr10_static_metadata_lands_in_svt_params() {
        let mut params = dv_params(Encoder::SvtAv1, DvMode::ToHdr10, Some(8));
        params.hdr10_static = Some(Hdr10StaticMetadata {
            red: (0.708, 0.292),
            green: (0.17, 0.797),
            blue: (0.131, 0.046),
            white_point: (0.3127, 0.329),
            max_luminance: 1000.0,
            min_luminance: 0.005,
            max_cll: 1000,
            max_fall: 400,
        });
        let args = build_ffmpeg_args(&params);
        let svt = arg_after(&args, "-svtav1-params").unwrap();
        assert!(svt.contains("mastering-display=G(0.17000,0.79700)"));
        assert!(svt.contains("content-light=1000,400"));
    }
}
