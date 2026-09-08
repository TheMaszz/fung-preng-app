// src-tauri/src/decode.rs
//
// Decodes an audio file into raw interleaved f32 PCM samples, using Symphonia.
// This is the shared building block: playback (step 4), BPM/key detection
// (step 5), and time-stretching (step 7) all operate on this same PCM data,
// so getting the format right here matters for everything downstream.

use anyhow::{anyhow, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded audio, ready for playback/analysis/stretching.
/// `samples` is interleaved (e.g. for stereo: L,R,L,R,...).
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
}

impl DecodedAudio {
    /// Duration in seconds — a good sanity check that decoding actually
    /// worked (compare this against the real song length).
    pub fn duration_secs(&self) -> f64 {
        let frames = self.samples.len() / self.channels.max(1);
        frames as f64 / self.sample_rate as f64
    }
}

pub fn decode_to_pcm(path: &Path) -> Result<DecodedAudio> {
    let file = File::open(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).map(String::from);
    decode_from_source(Box::new(file), ext.as_deref())
}

/// Decodes from any Symphonia MediaSource — a local File, or (new) an
/// HttpRangeSource that fetches bytes progressively over the network as
/// Symphonia asks for them, which is what actually lets playback start
/// before the whole track has downloaded.
pub fn decode_from_source(
    source: Box<dyn symphonia::core::io::MediaSource>,
    extension_hint: Option<&str>,
) -> Result<DecodedAudio> {
    let mss = MediaSourceStream::new(source, Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = extension_hint {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no supported audio track found"))?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("track has no sample rate info"))?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2);

    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // End of stream is reported as an IO error — this is the normal
            // way decoding finishes, not a real failure.
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                samples.extend_from_slice(buf.samples());
            }
            // A single bad packet shouldn't kill the whole decode — skip it.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    if samples.is_empty() {
        return Err(anyhow!("decoded zero samples — source may be corrupt or empty"));
    }

    Ok(DecodedAudio {
        samples,
        sample_rate,
        channels,
    })
}