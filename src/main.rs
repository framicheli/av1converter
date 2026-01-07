mod error;
use error::AppError;
use std::process::Command;

/// Video resolutions enum
#[derive(Debug, Clone, Copy, PartialEq)]
enum Resolution {
    HD1080p,
    HD1080pHDR,
    HD1080pDV, // Don't convert
    UHD2160p,
    UHD2160pHDR,
    UHD2160pDV, // Don't convert
}

impl Resolution {}

struct Converter {
    resolution: Resolution,
}

impl Converter {
    pub fn new() -> Self {
        Self {
            resolution: Resolution::HD1080p,
        }
    }

    pub fn set_resolution() -> Result<(), AppError> {
        Ok(())
    }

    /// Execute the shell command
    fn execute(&self, command: &str, args: Vec<&str>) -> Result<(), AppError> {
        match Command::new(command).args(args).spawn() {
            Ok(_) => Ok(()),
            Err(e) => Err(AppError::CommandExecutionError {
                message: e.to_string(),
            }),
        }
    }

    /// Analyze video file and get a JSON file response
    pub fn analyze(&self, input_path: &str) -> Result<(), AppError> {
        let command = "ffprobe";
        let input = format!("{}", input_path);
        let args = [
            "-v error",
            "-select_streams v:0",
            "-show_entries stream=width,height,pix_fmt,color_primaries,color_transfer,color_space,side_data_list",
            "-of json",
            input.as_str()
            ].to_vec();
        self.execute(command, args)?;
        Ok(())
    }

    /// Evaluate the video quality using the VMAF score
    pub fn evaluate(&self, input: &str, output: &str) -> Result<(), AppError> {
        let main_command = format!("ffmpeg");
        let input_1_arg = format!("-i {}", input); // Original
        let input_2_arg = format!("-i {}", output); // AV1 encoded file
        let vmaf_arg = format!("-lavfi libvmaf");
        let final_arg = format!("-f null -");

        let args = [
            input_1_arg.as_str(),
            input_2_arg.as_str(),
            vmaf_arg.as_str(),
            final_arg.as_str(),
        ]
        .to_vec();

        self.execute(main_command.as_str(), args)?;

        Ok(())
    }
}

fn main() {
    println!("Hello, world!");
}
