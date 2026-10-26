//! The output file: a muxer with an optional picture stream and an
//! optional sound stream, fed BGRA pictures and stereo `f32` samples.

use std::path::Path;

use ff::format::{Pixel, Sample, sample::Type as SampleType};
use ff::software::scaling;
use ff::{ChannelLayout, Dictionary, Packet, Rational, codec, encoder, format, frame};
use ffmpeg_next as ff;
use videode_core::Fps;
use videode_media::Picture;
use videode_media::audio::{CHANNELS, RATE};

use crate::ExportError;
use crate::codecs::{self, Plan, VideoCodec, VideoPlan};

impl From<ff::Error> for ExportError {
    fn from(e: ff::Error) -> Self {
        ExportError::Ffmpeg(e.to_string())
    }
}

struct VideoOut {
    encoder: encoder::video::Encoder,
    index: usize,
    time_base: Rational,
    scaler: scaling::Context,
    source: frame::Video,
    converted: frame::Video,
}

struct AudioOut {
    encoder: encoder::audio::Encoder,
    index: usize,
    time_base: Rational,
    format: Sample,
    /// Samples per frame the encoder wants.
    frame_size: usize,
    /// Samples sent so far.
    sent: i64,
    pending: Vec<f32>,
}

pub(crate) struct Writer {
    output: format::context::Output,
    video: Option<VideoOut>,
    audio: Option<AudioOut>,
    /// The video encoder that opened.
    pub encoder: Option<&'static str>,
}

impl Writer {
    /// Opens `path` for writing. For image sequences `path` holds the
    /// frame number pattern.
    pub fn open(path: &Path, plan: &Plan, fps: Fps) -> Result<Self, ExportError> {
        let mut output = format::output_as(path, plan.muxer)
            .map_err(|e| ExportError::Io(format!("couldn't create {}: {e}", path.display())))?;
        let global_header = output
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);
        let mut chosen = None;
        let video = match &plan.video {
            Some(v) => {
                let (enc, name) = open_video(v, fps, global_header)?;
                chosen = Some(name);
                let codec = encoder::find_by_name(name).expect("opened above");
                let mut stream = output.add_stream(codec)?;
                stream.set_parameters(&enc);
                stream.set_time_base(enc.time_base());
                if v.codec == VideoCodec::H265 && plan.muxer != "matroska" {
                    // Apple players only take H.265 tagged hvc1.
                    unsafe {
                        (*(*stream.as_mut_ptr()).codecpar).codec_tag = u32::from_le_bytes(*b"hvc1");
                    }
                }
                let index = stream.index();
                let scaler = scaling::Context::get(
                    Pixel::BGRA,
                    v.width,
                    v.height,
                    enc.format(),
                    v.width,
                    v.height,
                    scaling::Flags::BICUBIC,
                )?;
                let mut out = VideoOut {
                    time_base: enc.time_base(),
                    encoder: enc,
                    index,
                    scaler,
                    source: frame::Video::new(Pixel::BGRA, v.width, v.height),
                    converted: frame::Video::empty(),
                };
                if v.codec != VideoCodec::Png {
                    rec709(&mut out.scaler);
                }
                Some(out)
            }
            None => None,
        };
        let audio = match plan.audio {
            Some(name) => {
                let enc = open_audio(name, global_header)?;
                let codec = encoder::find_by_name(name).expect("opened above");
                let mut stream = output.add_stream(codec)?;
                stream.set_parameters(&enc);
                stream.set_time_base(Rational(1, RATE as i32));
                let frame_size = match enc.frame_size() {
                    0 => 1024,
                    n => n as usize,
                };
                Some(AudioOut {
                    index: stream.index(),
                    time_base: Rational(1, RATE as i32),
                    format: enc.format(),
                    encoder: enc,
                    frame_size,
                    sent: 0,
                    pending: Vec::new(),
                })
            }
            None => None,
        };
        output.write_header()?;
        Ok(Self {
            output,
            video,
            audio,
            encoder: chosen,
        })
    }

    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Encodes picture number `index` (from 0).
    pub fn video(&mut self, pic: &Picture, index: i64) -> Result<(), ExportError> {
        let Some(v) = &mut self.video else {
            return Ok(());
        };
        let stride = v.source.stride(0);
        let row = pic.width as usize * 4;
        let data = v.source.data_mut(0);
        for (y, src) in pic.bgra.chunks_exact(row).enumerate() {
            data[y * stride..y * stride + row].copy_from_slice(src);
        }
        v.scaler.run(&v.source, &mut v.converted)?;
        v.converted.set_pts(Some(index));
        v.encoder.send_frame(&v.converted)?;
        drain(&mut v.encoder, v.index, v.time_base, &mut self.output)
    }

    /// Queues interleaved stereo samples and encodes whole frames of them.
    pub fn audio(&mut self, samples: &[f32]) -> Result<(), ExportError> {
        let Some(a) = &mut self.audio else {
            return Ok(());
        };
        a.pending.extend_from_slice(samples);
        let block = a.frame_size * CHANNELS;
        while a.pending.len() >= block {
            let rest = a.pending.split_off(block);
            let chunk = std::mem::replace(&mut a.pending, rest);
            send_audio(a, &chunk, &mut self.output)?;
        }
        Ok(())
    }

    /// Flushes the encoders and closes the file.
    pub fn finish(mut self) -> Result<(), ExportError> {
        if let Some(v) = &mut self.video {
            v.encoder.send_eof()?;
            drain(&mut v.encoder, v.index, v.time_base, &mut self.output)?;
        }
        if let Some(a) = &mut self.audio {
            if !a.pending.is_empty() {
                let chunk = std::mem::take(&mut a.pending);
                send_audio(a, &chunk, &mut self.output)?;
            }
            a.encoder.send_eof()?;
            drain(&mut a.encoder, a.index, a.time_base, &mut self.output)?;
        }
        self.output.write_trailer()?;
        Ok(())
    }
}

/// Tries each candidate encoder until one opens.
fn open_video(
    v: &VideoPlan,
    fps: Fps,
    global_header: bool,
) -> Result<(encoder::video::Encoder, &'static str), ExportError> {
    let mut last = None;
    for name in v.codec.candidates(v.hardware, v.quality) {
        match try_video(name, v, fps, global_header) {
            Ok(enc) => return Ok((enc, name)),
            Err(e) => last = Some(format!("{name}: {e}")),
        }
    }
    Err(ExportError::NoEncoder(last.unwrap_or_else(|| {
        "FFmpeg was built without an encoder for this codec".into()
    })))
}

pub(crate) fn try_video(
    name: &'static str,
    v: &VideoPlan,
    fps: Fps,
    global_header: bool,
) -> Result<encoder::video::Encoder, ff::Error> {
    let codec = encoder::find_by_name(name).ok_or(ff::Error::EncoderNotFound)?;
    let supported: Vec<Pixel> = codec
        .video()?
        .formats()
        .map(|f| f.collect())
        .unwrap_or_default();
    let pixel = v
        .codec
        .pixel_formats()
        .iter()
        .copied()
        .find(|p| supported.is_empty() || supported.contains(p))
        .ok_or(ff::Error::InvalidData)?;
    let mut enc = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    enc.set_width(v.width);
    enc.set_height(v.height);
    enc.set_format(pixel);
    enc.set_time_base(Rational(fps.den as i32, fps.num as i32));
    enc.set_frame_rate(Some(Rational(fps.num as i32, fps.den as i32)));
    if !matches!(v.codec, VideoCodec::Png | VideoCodec::ProRes { .. }) {
        enc.set_gop((fps.as_f64() * 2.).round().max(1.) as u32);
    }
    if v.codec != VideoCodec::Png {
        enc.set_colorspace(ff::color::Space::BT709);
        enc.set_color_range(ff::color::Range::MPEG);
        unsafe {
            let ctx = enc.as_mut_ptr();
            (*ctx).color_primaries = ff::ffi::AVColorPrimaries::AVCOL_PRI_BT709;
            (*ctx).color_trc = ff::ffi::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
        }
    }
    if global_header {
        enc.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let (opts, bitrate) = codecs::options(name, v, fps.as_f64());
    if let Some(b) = bitrate {
        enc.set_bit_rate(b);
        if b > 0 {
            enc.set_max_bit_rate(b * 3 / 2);
        }
    }
    let mut dict = Dictionary::new();
    for (k, val) in &opts {
        dict.set(k, val);
    }
    enc.open_with(dict)
}

fn open_audio(name: &str, global_header: bool) -> Result<encoder::audio::Encoder, ExportError> {
    let codec = encoder::find_by_name(name)
        .ok_or_else(|| ExportError::NoEncoder(format!("FFmpeg has no {name} encoder")))?;
    let supported: Vec<Sample> = codec
        .audio()?
        .formats()
        .map(|f| f.collect())
        .unwrap_or_default();
    let format = [
        Sample::F32(SampleType::Planar),
        Sample::F32(SampleType::Packed),
        Sample::I32(SampleType::Packed),
        Sample::I32(SampleType::Planar),
        Sample::I16(SampleType::Packed),
        Sample::I16(SampleType::Planar),
    ]
    .into_iter()
    .find(|f| supported.contains(f))
    .ok_or_else(|| {
        ExportError::NoEncoder(format!("{name} takes no sample format videode makes"))
    })?;
    let mut enc = codec::context::Context::new_with_codec(codec)
        .encoder()
        .audio()?;
    enc.set_rate(RATE as i32);
    enc.set_channel_layout(ChannelLayout::STEREO);
    enc.set_format(format);
    enc.set_time_base(Rational(1, RATE as i32));
    match name {
        "aac" => enc.set_bit_rate(320_000),
        "libopus" => enc.set_bit_rate(192_000),
        _ => {}
    }
    if global_header {
        enc.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    Ok(enc.open_with(Dictionary::new())?)
}

fn send_audio(
    a: &mut AudioOut,
    interleaved: &[f32],
    output: &mut format::context::Output,
) -> Result<(), ExportError> {
    let n = interleaved.len() / CHANNELS;
    let mut f = frame::Audio::new(a.format, n, ChannelLayout::STEREO);
    f.set_rate(RATE);
    let planar = a.format.is_planar();
    let put = |f: &mut frame::Audio, bytes: &dyn Fn(f32) -> Vec<u8>| {
        let size = bytes(0.).len();
        for i in 0..n {
            for c in 0..CHANNELS {
                let s = bytes(interleaved[i * CHANNELS + c]);
                let (plane, at) = if planar {
                    (c, i * size)
                } else {
                    (0, (i * CHANNELS + c) * size)
                };
                f.data_mut(plane)[at..at + size].copy_from_slice(&s);
            }
        }
    };
    match a.format {
        Sample::F32(_) => put(&mut f, &|s| s.to_ne_bytes().to_vec()),
        Sample::I32(_) => put(&mut f, &|s| {
            ((s.clamp(-1., 1.) as f64 * i32::MAX as f64) as i32)
                .to_ne_bytes()
                .to_vec()
        }),
        _ => put(&mut f, &|s| {
            ((s.clamp(-1., 1.) * i16::MAX as f32) as i16)
                .to_ne_bytes()
                .to_vec()
        }),
    }
    f.set_pts(Some(a.sent));
    a.sent += n as i64;
    a.encoder.send_frame(&f)?;
    drain(&mut a.encoder, a.index, a.time_base, output)
}

fn drain(
    enc: &mut encoder::Encoder,
    index: usize,
    time_base: Rational,
    output: &mut format::context::Output,
) -> Result<(), ExportError> {
    let stream_tb = output
        .stream(index)
        .map(|s| s.time_base())
        .unwrap_or(time_base);
    let mut packet = Packet::empty();
    while enc.receive_packet(&mut packet).is_ok() {
        packet.set_stream(index);
        packet.rescale_ts(time_base, stream_tb);
        packet.write_interleaved(output)?;
    }
    Ok(())
}

/// Converts with Rec.709 coefficients into limited-range YUV, matching the
/// tags written on the stream (swscale defaults to Rec.601).
// Bindgen types the SWS_CS constants differently per platform.
#[allow(clippy::unnecessary_cast)]
fn rec709(scaler: &mut scaling::Context) {
    unsafe {
        let table = ff::ffi::sws_getCoefficients(ff::ffi::SWS_CS_ITU709 as i32);
        ff::ffi::sws_setColorspaceDetails(
            scaler.as_mut_ptr(),
            table,
            1,
            table,
            0,
            0,
            1 << 16,
            1 << 16,
        );
    }
}
