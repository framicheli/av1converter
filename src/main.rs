mod analysis;
mod converter;
mod error;

use crate::analysis::AnalysisResult;
use crate::converter::Converter;
use crate::error::AppError;
use std::env;

fn main() -> Result<(), AppError> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 3 {
        eprintln!("Usage: {} <input_video> <output_video>", args[0]);
        std::process::exit(1);
    }
    
    let input_path = &args[1];
    let output_path = &args[2];
    
    println!("Starting AV1 conversion workflow...");
    println!("Input: {}", input_path);
    println!("Output: {}", output_path);
    
    // Step 1: Analyze
    println!("\n[1/4] Analyzing video...");
    let analysis = analyze_video(input_path)?;
    println!("Video analysis complete: {}x{}", analysis.width, analysis.height);
    
    // Step 2: Decide
    println!("\n[2/4] Classifying video and deciding conversion strategy...");
    let resolution = analysis.classify_video()?;
    println!("Video classified as: {:?}", resolution);
    
    let converter = Converter::new(resolution);
    
    if !converter.should_convert() {
        println!("Video should not be converted (Dolby Vision detected). Exiting.");
        return Ok(());
    }
    
    // Step 3: Encode
    println!("\n[3/4] Encoding video to AV1...");
    converter.encode(input_path, output_path)?;
    println!("Encoding complete!");
    
    // Step 4: Evaluate
    println!("\n[4/4] Evaluating video quality...");
    converter.evaluate(input_path, output_path)?;
    println!("Evaluation complete!");
    
    println!("\n✅ Conversion workflow completed successfully!");
    Ok(())
}

fn analyze_video(input_path: &str) -> Result<AnalysisResult, AppError> {
    // Create a temporary converter just for analysis
    // We'll use HD1080p as a placeholder since we don't know the resolution yet
    let temp_converter = Converter::new(crate::analysis::Resolution::HD1080p);
    temp_converter.analyze(input_path)
}
