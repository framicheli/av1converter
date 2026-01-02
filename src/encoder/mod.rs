pub mod config;
pub mod encode;

/// Represents different video codecs
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoCodec {
    HEVC,
    H264,
    VP9,
    AV1,
    Unknown,
}
