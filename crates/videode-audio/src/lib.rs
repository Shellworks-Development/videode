//! Plays the sequence's sound. A mixer renders the timeline's audio clips;
//! the output engine sends them to the default device on its own thread,
//! and the number of samples the device has taken is the playback clock.

pub mod mixer;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use videode_media::audio::CHANNELS;

pub use mixer::{ClipSound, Mix, Mixer, TrackSound, db_to_gain};

/// Samples rendered per mixer block, per channel.
const BLOCK: usize = 1024;
/// How much sound is rendered ahead of the device.
const AHEAD: Duration = Duration::from_millis(200);

/// State shared with the device callback.
struct Shared {
    /// Bumped on every play and stop; a callback from an older stream
    /// neither advances the clock nor feeds the meters.
    generation: AtomicU64,
    /// Frames the device has taken since the current play started.
    played: AtomicU64,
    /// Loudest sample per channel since the meters were last read (f32 bits;
    /// non-negative floats order the same as their bits).
    peaks: [AtomicU32; 2],
}

enum Command {
    Play { at: f64, mix: Mix, generation: u64 },
    Update(Mix),
    Stop,
}

pub struct AudioEngine {
    commands: Sender<Command>,
    shared: Arc<Shared>,
    rate: u32,
    /// Sequence time at which the current play started.
    origin: Option<f64>,
}

impl AudioEngine {
    /// Opens the default output device. Fails if there is none.
    pub fn start() -> Result<Self, String> {
        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            played: AtomicU64::new(0),
            peaks: [AtomicU32::new(0), AtomicU32::new(0)],
        });
        let (commands, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread_shared = shared.clone();
        std::thread::Builder::new()
            .name("videode-audio".into())
            .spawn(move || {
                let device = match cpal::default_host().default_output_device() {
                    Some(d) => d,
                    None => {
                        ready_tx.send(Err("no audio output device".into())).ok();
                        return;
                    }
                };
                let config = match device.default_output_config() {
                    Ok(c) => c,
                    Err(e) => {
                        ready_tx.send(Err(e.to_string())).ok();
                        return;
                    }
                };
                ready_tx.send(Ok(config.sample_rate())).ok();
                run(device, config, rx, thread_shared);
            })
            .map_err(|e| e.to_string())?;
        let rate = ready_rx
            .recv()
            .map_err(|_| "the audio thread stopped".to_string())??;
        Ok(Self {
            commands,
            shared,
            rate,
            origin: None,
        })
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Starts playing the sequence from `at` seconds.
    pub fn play(&mut self, at: f64, mix: Mix) {
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared.played.store(0, Ordering::SeqCst);
        self.origin = Some(at);
        self.commands
            .send(Command::Play {
                at,
                mix,
                generation,
            })
            .ok();
    }

    /// Applies edits made while playing.
    pub fn update(&self, mix: Mix) {
        self.commands.send(Command::Update(mix)).ok();
    }

    pub fn stop(&mut self) {
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
        self.origin = None;
        self.commands.send(Command::Stop).ok();
    }

    pub fn is_playing(&self) -> bool {
        self.origin.is_some()
    }

    /// The sequence time the listener is hearing now, while playing. Holds
    /// at the start point until the device begins taking samples.
    pub fn position(&self) -> Option<f64> {
        let origin = self.origin?;
        let played = self.shared.played.load(Ordering::SeqCst);
        Some(origin + played as f64 / self.rate as f64)
    }

    /// Peak level per channel (0–1) since the last call.
    pub fn take_peaks(&self) -> [f32; 2] {
        [0, 1].map(|c| f32::from_bits(self.shared.peaks[c].swap(0, Ordering::Relaxed)))
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.commands.send(Command::Stop).ok();
    }
}

struct Playing {
    _stream: cpal::Stream,
    ring: rtrb::Producer<f32>,
    /// Sequence time of the next block to render.
    next: f64,
}

/// The audio thread: owns the mixer and the device stream.
fn run(
    device: cpal::Device,
    config: cpal::SupportedStreamConfig,
    commands: Receiver<Command>,
    shared: Arc<Shared>,
) {
    let rate = config.sample_rate();
    let mut mixer = Mixer::new(rate);
    let mut playing: Option<Playing> = None;
    let mut block = vec![0f32; BLOCK * CHANNELS];
    loop {
        let command = if playing.is_some() {
            match commands.recv_timeout(Duration::from_millis(5)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match commands.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        match command {
            Some(Command::Play {
                at,
                mix,
                generation,
            }) => {
                playing = None;
                mixer.set_mix(mix);
                let capacity = (AHEAD.as_secs_f64() * rate as f64) as usize * CHANNELS;
                let (mut ring, consumer) =
                    rtrb::RingBuffer::new(capacity.max(BLOCK * CHANNELS * 2));
                // Prefill so the device never starts on an empty buffer.
                let mut next = at;
                while ring.slots() >= block.len() {
                    mixer.render(next, &mut block);
                    push(&mut ring, &block);
                    next += BLOCK as f64 / rate as f64;
                }
                match open_stream(&device, &config, consumer, shared.clone(), generation) {
                    Ok(stream) => {
                        playing = Some(Playing {
                            _stream: stream,
                            ring,
                            next,
                        })
                    }
                    Err(e) => eprintln!("videode: couldn't start audio: {e}"),
                }
            }
            Some(Command::Update(mix)) => mixer.set_mix(mix),
            Some(Command::Stop) => playing = None,
            None => {}
        }
        if let Some(p) = &mut playing {
            while p.ring.slots() >= block.len() {
                mixer.render(p.next, &mut block);
                push(&mut p.ring, &block);
                p.next += BLOCK as f64 / rate as f64;
            }
        }
    }
}

fn push(ring: &mut rtrb::Producer<f32>, samples: &[f32]) {
    if let Ok(chunk) = ring.write_chunk_uninit(samples.len()) {
        chunk.fill_from_iter(samples.iter().copied());
    }
}

fn open_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    consumer: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
    generation: u64,
) -> Result<cpal::Stream, String> {
    let format = config.sample_format();
    let config: cpal::StreamConfig = (*config).into();
    match format {
        SampleFormat::F32 => build::<f32>(device, config, consumer, shared, generation),
        SampleFormat::I16 => build::<i16>(device, config, consumer, shared, generation),
        SampleFormat::I32 => build::<i32>(device, config, consumer, shared, generation),
        SampleFormat::U16 => build::<u16>(device, config, consumer, shared, generation),
        SampleFormat::F64 => build::<f64>(device, config, consumer, shared, generation),
        other => Err(format!("unsupported sample format {other}")),
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut consumer: rtrb::Consumer<f32>,
    shared: Arc<Shared>,
    generation: u64,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let current = shared.generation.load(Ordering::Relaxed) == generation;
                let mut peak = [0f32; 2];
                let frames = data.len() / channels.max(1);
                for frame in data.chunks_mut(channels.max(1)) {
                    let l = consumer.pop().unwrap_or(0.);
                    let r = consumer.pop().unwrap_or(0.);
                    let (l, r) = if current { (l, r) } else { (0., 0.) };
                    peak[0] = peak[0].max(l.abs());
                    peak[1] = peak[1].max(r.abs());
                    match frame.len() {
                        1 => frame[0] = T::from_sample((l + r) * 0.5),
                        _ => {
                            frame[0] = T::from_sample(l);
                            frame[1] = T::from_sample(r);
                            for s in &mut frame[2..] {
                                *s = T::from_sample(0.);
                            }
                        }
                    }
                }
                if current {
                    shared.played.fetch_add(frames as u64, Ordering::SeqCst);
                    for (c, p) in peak.iter().enumerate() {
                        shared.peaks[c].fetch_max(p.min(4.).to_bits(), Ordering::Relaxed);
                    }
                }
            },
            |e| eprintln!("videode: audio stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}
