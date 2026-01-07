mod error;
use error::AppError;
use serde::{Deserialize, Serialize};
use std::process::Command;

/// Video resolutions enum
#[derive(Debug, Clone, Copy, PartialEq)]
enum Resolution {
    HD1080p,
    HD1080pHDR,
    HD1080pDV,
    UHD2160p,
    UHD2160pHDR,
    UHD2160pDV,
}

impl Resolution {
    /// Returns true if this resolution should skip conversion
    fn should_skip_conversion(&self) -> bool {
        matches!(self, Resolution::HD1080pDV | Resolution::UHD2160pDV)
    }

    /// Get target dimensions for this resolution
    fn dimensions(&self) -> (u32, u32) {
        match self {
            Resolution::HD1080p | Resolution::HD1080pHDR | Resolution::HD1080pDV => (1920, 1080),
            Resolution::UHD2160p | Resolution::UHD2160pHDR | Resolution::UHD2160pDV => (3840, 2160),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct VideoMetadata {
    width: u32,
    height: u32,
    pix_fmt: String,
    color_primaries: Option<String>,
    color_transfer: Option<String>,
    color_space: Option<String>,
}

struct Converter {
    resolution: Resolution,
}

impl Converter {
    pub fn new() -> Self {
        Self {
            resolution: Resolution::HD1080p,
        }
    }

    /// Set the target resolution for conversion
    pub fn set_resolution(&mut self, resolution: Resolution) {
        self.resolution = resolution;
    }

    /// Get the current resolution
    pub fn get_resolution(&self) -> Resolution {
        self.resolution
    }

    /// Execute a shell command and wait for completion
    fn execute(&self, command: &str, args: Vec<&str>) -> Result<String, AppError> {
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

    /// Analyze video file and return parsed metadata
    pub fn analyze(&self, input_path: &str) -> Result<String, AppError> {
        let args = vec![
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

        self.execute("ffprobe", args)
    }

    /// Evaluate the video quality using the VMAF score
    pub fn evaluate(&self, original: &str, encoded: &str, log_path: &str) -> Result<(), AppError> {
        let vmaf_filter = format!("libvmaf=log_path={}:log_fmt=json", log_path);

        let args = vec![
            "-i",
            original,
            "-i",
            encoded,
            "-lavfi",
            &vmaf_filter,
            "-f",
            "null",
            "-",
        ];

        self.execute("ffmpeg", args)?;
        Ok(())
    }

    /// Convert video to AV1 format with target resolution
    pub fn convert(&self, input_path: &str, output_path: &str) -> Result<(), AppError> {
        if self.resolution.should_skip_conversion() {
            return Err(AppError::CommandExecutionError {
                message: format!(
                    "Dolby Vision content ({:?}) should not be converted",
                    self.resolution
                ),
            });
        }

        let (width, height) = self.resolution.dimensions();
        let scale_filter = format!("scale={}:{}", width, height);

        let args = vec![
            "-i",
            input_path,
            "-c:v",
            "libsvtav1",
            "-crf",
            "30",
            "-preset",
            "6",
            "-vf",
            &scale_filter,
            "-c:a",
            "copy",
            output_path,
        ];

        self.execute("ffmpeg", args)?;
        Ok(())
    }

    /// Check if input video needs conversion based on current resolution setting
    pub fn needs_conversion(&self, input_path: &str) -> Result<bool, AppError> {
        let metadata_json = self.analyze(input_path)?;

        // Parse the JSON to check current resolution
        // This is simplified - you'd want proper JSON parsing here
        let target_dims = self.resolution.dimensions();

        // For now, return true to indicate conversion needed
        // In practice, you'd parse the JSON and compare dimensions
        Ok(true)
    }
}

impl Default for Converter {
    fn default() -> Self {
        Self::new()
    }
}

fn main() {
    let mut converter = Converter::new();
    converter.set_resolution(Resolution::HD1080p);

    println!(
        "Video Converter initialized with {:?}",
        converter.get_resolution()
    );

    // Example usage:
    // match converter.analyze("input.mp4") {
    //     Ok(metadata) => println!("Video metadata: {}", metadata),
    //     Err(e) => eprintln!("Analysis failed: {:?}", e),
    // }

    // match converter.convert("input.mp4", "output.mkv") {
    //     Ok(_) => println!("Conversion successful"),
    //     Err(e) => eprintln!("Conversion failed: {:?}", e),
    // }

    // match converter.evaluate("input.mp4", "output.mkv", "vmaf_log.json") {
    //     Ok(_) => println!("Quality evaluation complete"),
    //     Err(e) => eprintln!("Evaluation failed: {:?}", e),
    // }
}
