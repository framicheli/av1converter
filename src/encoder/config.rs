/// Configuration for AV1 encoding
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub crf: i32,          // Constant Rate Factor (0-63, lower = better quality)
    pub cpu_used: i32,     // CPU usage (0-8 for libaom-av1)
    pub row_mt: bool,      // Enable row-based multithreading
    pub tile_columns: i32, // Number of tile columns (for parallel encoding)
    pub tile_rows: i32,    // Number of tile rows
    pub threads: usize,    // Number of threads
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            crf: 30,
            cpu_used: 4,
            row_mt: true,
            tile_columns: 2,
            tile_rows: 2,
            threads: 0, // 0 = auto-detect
        }
    }
}

impl EncoderConfig {
    /// Preset for high quality (slower encoding)
    pub fn high_quality() -> Self {
        Self {
            crf: 23,
            cpu_used: 2,
            row_mt: true,
            tile_columns: 2,
            tile_rows: 2,
            threads: 0,
        }
    }

    /// Preset for balanced quality/speed
    pub fn balanced() -> Self {
        Self::default()
    }

    /// Preset for fast encoding (lower quality)
    pub fn fast() -> Self {
        Self {
            crf: 35,
            cpu_used: 6,
            row_mt: true,
            tile_columns: 4,
            tile_rows: 4,
            threads: 0,
        }
    }
}
