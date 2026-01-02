use ffmpeg_next as ffmpeg;
use std::path::Path;
use tracing::info;

use super::VideoCodec;
use super::config::EncoderConfig;

/// Detects the codec of a video file
pub fn detect_codec(input_path: &str) -> Result<VideoCodec, ffmpeg::Error> {
    let input = ffmpeg::format::input(&Path::new(input_path))?;

    let video_stream = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or(ffmpeg::Error::StreamNotFound)?;

    let codec_id = video_stream.parameters().id();

    Ok(match codec_id {
        ffmpeg::codec::Id::HEVC => VideoCodec::HEVC,
        ffmpeg::codec::Id::H264 => VideoCodec::H264,
        ffmpeg::codec::Id::VP9 => VideoCodec::VP9,
        ffmpeg::codec::Id::AV1 => VideoCodec::AV1,
        _ => VideoCodec::Unknown,
    })
}

/// Transcodes a video file to AV1 using native FFmpeg bindings
pub fn transcode_to_av1(
    input_path: &str,
    output_path: &str,
    config: &EncoderConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize FFmpeg
    ffmpeg::init()?;

    // Open input file
    let mut input = ffmpeg::format::input(&Path::new(input_path))?;

    // Find best video stream
    let video_stream_index = input
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or("No video stream found")?
        .index();

    // Create output context
    let mut output = ffmpeg::format::output(&Path::new(output_path))?;

    // Setup video transcoding
    let video_stream = input.stream(video_stream_index).unwrap();
    let video_time_base = video_stream.time_base();
    let decoder_context =
        ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())?;
    let mut decoder = decoder_context.decoder().video()?;

    // Create AV1 encoder
    let encoder_codec =
        ffmpeg::encoder::find(ffmpeg::codec::Id::AV1).ok_or("AV1 encoder not found")?;

    let output_stream = output.add_stream(encoder_codec)?;
    let output_time_base = output_stream.time_base();
    let encoder_context = ffmpeg::codec::context::Context::new();
    let mut encoder = encoder_context.encoder().video()?;

    // Configure encoder
    encoder.set_width(decoder.width());
    encoder.set_height(decoder.height());
    encoder.set_format(decoder.format());
    encoder.set_time_base(video_time_base);

    if config.threads > 0 {
        encoder.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: config.threads,
        });
    }

    // Set encoder options
    let mut opts = ffmpeg::Dictionary::new();
    opts.set("crf", &config.crf.to_string());
    opts.set("cpu-used", &config.cpu_used.to_string());
    opts.set("row-mt", if config.row_mt { "1" } else { "0" });
    opts.set("tile-columns", &config.tile_columns.to_string());
    opts.set("tile-rows", &config.tile_rows.to_string());

    // Open encoder with options
    let mut encoder = encoder.open_with(opts)?;

    // Copy audio streams without re-encoding
    {
        let audio_streams: Vec<_> = input
            .streams()
            .filter(|s| s.parameters().medium() == ffmpeg::media::Type::Audio)
            .map(|s| (s.parameters().id(), s.parameters()))
            .collect();

        for (codec_id, params) in audio_streams {
            let mut audio_stream = output
                .add_stream(ffmpeg::encoder::find(codec_id).ok_or("Audio codec not found")?)?;
            audio_stream.set_parameters(params);
        }
    }

    // Write output header
    output.write_header()?;
    info!("Starting transcoding");
    info!("Output: {}", output_path);
    info!("Config: {:?}", config);

    // Transcoding loop
    let mut frame_count = 0;
    let receive_and_process_packets =
        |encoder: &mut ffmpeg::encoder::video::Video,
         output: &mut ffmpeg::format::context::Output| {
            let mut encoded_packet = ffmpeg::Packet::empty();
            while encoder.receive_packet(&mut encoded_packet).is_ok() {
                encoded_packet.set_stream(0);
                encoded_packet.rescale_ts(video_time_base, output_time_base);
                encoded_packet.write_interleaved(output)?;
            }
            Ok::<(), ffmpeg::Error>(())
        };

    for (stream, mut packet) in input.packets() {
        if stream.index() == video_stream_index {
            packet.rescale_ts(stream.time_base(), decoder.time_base());
            decoder.send_packet(&packet)?;

            let mut decoded_frame = ffmpeg::frame::Video::empty();
            while decoder.receive_frame(&mut decoded_frame).is_ok() {
                frame_count += 1;
                if frame_count % 100 == 0 {
                    info!("Processed {} frames", frame_count);
                }

                encoder.send_frame(&decoded_frame)?;
                receive_and_process_packets(&mut encoder, &mut output)?;
            }
        } else {
            packet.write_interleaved(&mut output)?;
        }
    }

    // Flush decoder
    decoder.send_eof()?;
    let mut decoded_frame = ffmpeg::frame::Video::empty();
    while decoder.receive_frame(&mut decoded_frame).is_ok() {
        encoder.send_frame(&decoded_frame)?;
        receive_and_process_packets(&mut encoder, &mut output)?;
    }

    // Flush encoder
    encoder.send_eof()?;
    receive_and_process_packets(&mut encoder, &mut output)?;

    // Write trailer
    output.write_trailer()?;

    info!("Transcoding completed!");
    Ok(())
}

/// Batch transcode multiple files
pub fn batch_transcode(
    files: &[(&str, &str)],
    config: &EncoderConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    for (i, (input, output)) in files.iter().enumerate() {
        info!("\n[{}/{}] Processing: {}", i + 1, files.len(), input);
        transcode_to_av1(input, output, config)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_presets() {
        let hq = EncoderConfig::high_quality();
        assert_eq!(hq.crf, 23);

        let fast = EncoderConfig::fast();
        assert_eq!(fast.crf, 35);
    }
}
