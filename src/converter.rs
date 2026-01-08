use crate::analysis::{AnalysisOutput, AnalysisResult, Resolution};
use crate::error::AppError;
use std::process::Command;

pub enum EncoderProfile {
    HD1080p,
    HD1080pHDR,
    UHD2160p,
    UHD2160pHDR,
}

impl EncoderProfile {
    pub fn ffmpeg_args<'a>(&self, input: &'a str, output: &'a str) -> Vec<&'a str> {
        let mut args = vec![
            "-y",
            "-i",
            input,
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-map",
            "0:s?",
            "-c:v",
            "libsvtav1",
            "-preset",
            "4",
            "-pix_fmt",
            "yuv420p10le",
            "-c:a",
            "copy",
            "-c:s",
            "copy",
        ];

        match self {
            EncoderProfile::HD1080p => {
                args.extend(["-crf", "28"]);
                args.extend(["-svtav1-params", "tune=0:film-grain=0"]);
            }
            EncoderProfile::HD1080pHDR => {
                args.extend(["-crf", "29"]);
                args.extend(["-svtav1-params", "tune=0:film-grain=1"]);
                args.extend([
                    "-color_primaries",
                    "bt2020",
                    "-color_trc",
                    "smpte2084",
                    "-colorspace",
                    "bt2020nc",
                ]);
            }
            EncoderProfile::UHD2160p => {
                args.extend(["-crf", "30"]);
                args.extend(["-svtav1-params", "tune=0:film-grain=1"]);
            }
            EncoderProfile::UHD2160pHDR => {
                args.extend(["-crf", "30"]);
                args.extend(["-svtav1-params", "tune=0:film-grain=1"]);
                args.extend([
                    "-color_primaries",
                    "bt2020",
                    "-color_trc",
                    "smpte2084",
                    "-colorspace",
                    "bt2020nc",
                ]);
            }
        }

        args.push(output);
        args
    }
}

struct Converter {
    resolution: Resolution,
}

impl Converter {
    pub fn new(resolution: Resolution) -> Self {
        Self { resolution }
    }

    /// Execute the shell command
    fn execute(&self, command: &str, args: &[&str]) -> Result<(), AppError> {
        let status = Command::new(command).args(args).status().map_err(|e| {
            AppError::CommandExecutionError {
                message: format!("Failed to execute {}: {}", command, e),
            }
        })?;
        if !status.success() {
            return Err(AppError::CommandExecutionError {
                message: format!("{} failed with status: {}", command, status),
            });
        }

        Ok(())
    }

    /// Execute the shell command and return the output
    fn execute_output(&self, command: &str, args: &[&str]) -> Result<String, AppError> {
        let output = Command::new(command).args(args).output().map_err(|e| {
            AppError::CommandExecutionError {
                message: format!("Failed to execute {}: {}", command, e),
            }
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::CommandExecutionError {
                message: format!("{} failed: {}", command, stderr),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Analyze video file and get a JSON file response
    pub fn analyze(&self, input_path: &str) -> Result<AnalysisResult, AppError> {
        let args = [
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,pix_fmt,color_primaries,color_transfer,color_space,side_data_list",
            "-of",
            "json",
            input_path,
        ];
        let output: AnalysisOutput = serde_json::from_str(&self.execute_output("ffprobe", &args)?)
            .map_err(|e| AppError::CommandExecutionError {
                message: format!("Failed to parse ffprobe output: {}", e),
            })?;
        output
            .streams
            .into_iter()
            .next()
            .ok_or_else(|| AppError::CommandExecutionError {
                message: "No video stream found".to_string(),
            })
    }

    /// Determine if conversion is needed based on resolution
    pub fn should_convert(&self, analysis: &AnalysisResult) -> bool {
        matches!(
            self.resolution,
            Resolution::HD1080p
                | Resolution::HD1080pHDR
                | Resolution::UHD2160p
                | Resolution::UHD2160pHDR
        )
    }

    /// Evaluate the video quality using the VMAF score
    pub fn evaluate(&self, input: &str, output: &str) -> Result<(), AppError> {
        let args = [
            "-i",
            input,
            "-i",
            output,
            "-lavfi",
            "[0:v]format=yuv420p[ref];[1:v]format=yuv420p[dist];[ref][dist]libvmaf",
            "-f",
            "null",
            "-",
        ];

        self.execute("ffmpeg", &args)
    }
}
