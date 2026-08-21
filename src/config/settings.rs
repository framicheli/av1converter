//! Settings metadata shared by configuration interfaces and coverage tests.

/// Every serialized leaf in [`super::AppConfig`].
///
/// Optional values remain leaves when they serialize as `null`. Every preset
/// value has an independently editable leaf path.
pub const SERIALIZED_SETTING_PATHS: &[&str] = &[
    "language",
    "encoder",
    "quality_preset",
    "quality.vmaf_threshold",
    "quality.vmaf_enabled",
    "quality.delete_source_on_success",
    "performance.svt_preset",
    "performance.nvenc_preset",
    "presets.sd.crf",
    "presets.sd.film_grain",
    "presets.sd.nvenc_cq",
    "presets.sd.qsv_quality",
    "presets.sd.amf_quality",
    "presets.hd.crf",
    "presets.hd.film_grain",
    "presets.hd.nvenc_cq",
    "presets.hd.qsv_quality",
    "presets.hd.amf_quality",
    "presets.full_hd.crf",
    "presets.full_hd.film_grain",
    "presets.full_hd.nvenc_cq",
    "presets.full_hd.qsv_quality",
    "presets.full_hd.amf_quality",
    "presets.full_hd_hdr.crf",
    "presets.full_hd_hdr.film_grain",
    "presets.full_hd_hdr.nvenc_cq",
    "presets.full_hd_hdr.qsv_quality",
    "presets.full_hd_hdr.amf_quality",
    "presets.full_hd_dv.crf",
    "presets.full_hd_dv.film_grain",
    "presets.full_hd_dv.nvenc_cq",
    "presets.full_hd_dv.qsv_quality",
    "presets.full_hd_dv.amf_quality",
    "presets.uhd.crf",
    "presets.uhd.film_grain",
    "presets.uhd.nvenc_cq",
    "presets.uhd.qsv_quality",
    "presets.uhd.amf_quality",
    "presets.uhd_hdr.crf",
    "presets.uhd_hdr.film_grain",
    "presets.uhd_hdr.nvenc_cq",
    "presets.uhd_hdr.qsv_quality",
    "presets.uhd_hdr.amf_quality",
    "presets.uhd_dv.crf",
    "presets.uhd_dv.film_grain",
    "presets.uhd_dv.nvenc_cq",
    "presets.uhd_dv.qsv_quality",
    "presets.uhd_dv.amf_quality",
    "output.suffix",
    "output.container",
    "output.same_directory",
    "output.output_directory",
    "tracks.preferred_audio_languages",
    "tracks.preferred_subtitle_languages",
    "tracks.select_all_fallback",
    "audio.default_mode",
    "audio.opus_bitrate_per_channel",
    "audio.skip_already_opus",
    "daemon.enabled",
    "daemon.bind_address",
    "daemon.port",
    "daemon.browse_root",
    "daemon.auth_token",
    "disc.makemkvcon_path",
    "disc.staging_directory",
];

/// Settings that can widen daemon access or choose a host executable.
pub const LOCAL_ONLY_SETTING_PATHS: &[&str] = &[
    "daemon.enabled",
    "daemon.bind_address",
    "daemon.port",
    "daemon.browse_root",
    "daemon.auth_token",
    "disc.makemkvcon_path",
    "disc.staging_directory",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn leaf_paths(value: &Value, prefix: &str, paths: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    leaf_paths(value, &path, paths);
                }
            }
            _ => paths.push(prefix.to_string()),
        }
    }

    #[test]
    fn serialized_config_leaves_match_the_settings_inventory() {
        let value = serde_json::to_value(super::super::AppConfig::default()).unwrap();
        let mut actual = Vec::new();
        leaf_paths(&value, "", &mut actual);
        actual.sort();

        let mut expected: Vec<String> = SERIALIZED_SETTING_PATHS
            .iter()
            .map(|path| (*path).to_string())
            .collect();
        expected.sort();

        assert_eq!(actual, expected);
    }

    #[test]
    fn local_only_paths_are_serialized_settings() {
        assert!(
            LOCAL_ONLY_SETTING_PATHS
                .iter()
                .all(|path| SERIALIZED_SETTING_PATHS.contains(path))
        );
    }
}
