use crate::config::Encoder;
use crate::error::AppError;
use regex::Regex;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::info;

/// Result of CRF search via ab-av1
#[derive(Debug, Clone)]
pub struct CrfSearchResult {
    /// Optimal CRF value found
    pub crf: u8,
    /// Predicted VMAF score
    pub predicted_vmaf: f64,
}

/// Find optimal CRF using ab-av1 auto-crf
pub fn find_optimal_crf(
    input: &str,
    encoder: Encoder,
    min_vmaf: f64,
    cancel_flag: Arc<AtomicBool>,
) -> Result<CrfSearchResult, AppError> {
    let encoder_name = match encoder {
        Encoder::SvtAv1 => "libsvtav1",
        Encoder::Nvenc => "av1_nvenc",
        Encoder::Qsv => "av1_qsv",
        Encoder::Amf => "av1_amf",
    };

    if !is_available() {
        return Err(AppError::AbAv1("ab-av1 not available".to_string()));
    }
    info!(
        "Running ab-av1 CRF search for {} with encoder {}",
        input, encoder_name
    );

    let mut child = Command::new("ab-av1")
        .args([
            "auto-encode",
            "--input",
            input,
            "--encoder",
            encoder_name,
            "--min-vmaf",
            &min_vmaf.to_string(),
            "--quiet",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::AbAv1(format!("Failed to run ab-av1: {}", e)))?;

    loop {
        // Check if cancelled
        if cancel_flag.load(Ordering::Relaxed) {
            info!("CRF search cancelled, killing ab-av1 process");
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::AbAv1("CRF search cancelled".to_string()));
        }

        // Check if process has finished
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process finished
                let mut stdout = String::new();
                let mut stderr = String::new();

                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut stderr);
                }

                if !status.success() {
                    return Err(AppError::AbAv1(format!("ab-av1 failed: {}", stderr)));
                }

                return parse_ab_av1_output(&stdout);
            }
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(AppError::AbAv1(format!("Error waiting for ab-av1: {}", e)));
            }
        }
    }
}

/// Parse ab-av1 output to extract CRF and predicted VMAF
fn parse_ab_av1_output(output: &str) -> Result<CrfSearchResult, AppError> {
    // ab-av1 outputs: "crf 23 VMAF 95.42 ..."
    let crf_re = Regex::new(r"crf\s+(\d+)").map_err(|e| AppError::AbAv1(e.to_string()))?;
    let vmaf_re = Regex::new(r"VMAF\s+([\d.]+)").map_err(|e| AppError::AbAv1(e.to_string()))?;

    let crf = crf_re
        .captures(output)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u8>().ok())
        .ok_or_else(|| AppError::AbAv1("Could not parse CRF from ab-av1 output".to_string()))?;

    let vmaf = vmaf_re
        .captures(output)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(0.0);

    info!(
        "ab-av1 found optimal CRF: {} (predicted VMAF: {:.2})",
        crf, vmaf
    );

    Ok(CrfSearchResult {
        crf,
        predicted_vmaf: vmaf,
    })
}

/// Check if ab-av1 is available
pub fn is_available() -> bool {
    Command::new("ab-av1")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
