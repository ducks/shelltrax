use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use cpal::{
    Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use symphonia::core::{
    audio::{AudioBufferRef, Signal},
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
};

use symphonia::default::{get_codecs, get_probe};

use std::collections::VecDeque;

pub struct Player {
    pub current_path: Option<PathBuf>,
    pub is_playing: bool,
    pub handle: Option<JoinHandle<()>>,
    stream: Option<Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    pub autoplay_trigger: Arc<AtomicBool>,
    pub is_decoder_done: Arc<AtomicBool>,
    pub is_paused: bool,
    pub paused_flag: Arc<AtomicBool>,
}

impl Player {
    pub fn new() -> Self {
        Self {
            current_path: None,
            is_playing: false,
            stream: None,
            handle: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
            autoplay_trigger: Arc::new(AtomicBool::new(false)),
            is_decoder_done: Arc::new(AtomicBool::new(false)),
            is_paused: false,
            paused_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn play(&mut self, path: &Path) -> anyhow::Result<()> {
        self.stop(); // Stop any current playback

        self.autoplay_trigger.store(false, Ordering::SeqCst);
        self.is_decoder_done.store(false, Ordering::SeqCst);

        let file =
            File::open(path).map_err(|e| anyhow::anyhow!("Failed to open {:?}: {}", path, e))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let probed = get_probe()
            .format(
                &Default::default(),
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| anyhow::anyhow!("Unsupported format {:?}: {}", path, e))?;

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow::anyhow!("No supported audio track in {:?}", path))?;

        let mut decoder = get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| anyhow::anyhow!("Unsupported codec in {:?}: {}", path, e))?;

        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

        // Create CPAL output stream
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No audio output device available"))?;

        let config = cpal::StreamConfig {
            channels: channels as u16,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));

        let sample_buf = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let sample_buf_clone = Arc::clone(&sample_buf);

        let autoplay_trigger = Arc::clone(&self.autoplay_trigger);
        let decoder_done = Arc::clone(&self.is_decoder_done);
        let decoder_done_for_thread = Arc::clone(&self.is_decoder_done);
        let paused_flag = Arc::clone(&self.paused_flag);

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    let mut buf = sample_buf_clone.lock().unwrap();

                    if paused_flag.load(Ordering::SeqCst) {
                        for sample in data.iter_mut() {
                            *sample = 0.0;
                        }
                        return;
                    }

                    for sample in data.iter_mut() {
                        *sample = buf.pop_front().unwrap_or(0.0);
                    }

                    if buf.is_empty() && decoder_done.load(Ordering::SeqCst) {
                        autoplay_trigger.store(true, Ordering::SeqCst);
                    }
                },
                move |err| log::error!("CPAL stream error: {err}"),
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to build output stream: {}", e))?;

        stream
            .play()
            .map_err(|e| anyhow::anyhow!("Failed to start playback: {}", e))?;

        self.is_playing = true;
        self.current_path = Some(path.to_path_buf());

        // Spawn decoding thread
        let decode_buffer = Arc::clone(&sample_buf);
        let paused_flag_decoder = Arc::clone(&self.paused_flag);
        let handle = thread::spawn(move || {
            while let Ok(packet) = format.next_packet() {
                let decoded = match decoder.decode(&packet) {
                    Ok(decoded) => decoded,
                    Err(err) => {
                        log::error!("Decode error: {err}");
                        continue;
                    }
                };

                let spec = decoded.spec();
                log::debug!(
                    "Decoded: sample_rate={}, channels={}",
                    spec.rate,
                    spec.channels.count()
                );
                log::debug!(
                    "CPAL: sample_rate={}, channels={}",
                    config.sample_rate.0,
                    config.channels
                );

                let mut samples = Vec::new();

                match &decoded {
                    AudioBufferRef::F32(_) => log::debug!("Decoded buffer format: F32"),
                    AudioBufferRef::S16(_) => log::debug!("Decoded buffer format: S16"),
                    AudioBufferRef::U8(_) => log::debug!("Decoded buffer format: U8"),
                    AudioBufferRef::S24(_) => log::debug!("Decoded buffer format: S24"),
                    AudioBufferRef::F64(_) => log::debug!("Decoded buffer format: F64"),
                    AudioBufferRef::S32(_) => log::debug!("Decoded buffer format: S32"),
                    _ => log::debug!("Decoded buffer format: Unknown/Unsupported"),
                }

                match decoded {
                    AudioBufferRef::F32(buf) => {
                        for frame in 0..buf.frames() {
                            for ch in 0..buf.spec().channels.count() {
                                samples.push(buf.chan(ch)[frame]);
                            }
                        }
                    }
                    AudioBufferRef::S16(buf) => {
                        for frame in 0..buf.frames() {
                            for ch in 0..buf.spec().channels.count() {
                                samples.push(buf.chan(ch)[frame] as f32 / i16::MAX as f32);
                            }
                        }
                    }
                    AudioBufferRef::U8(buf) => {
                        for frame in 0..buf.frames() {
                            for ch in 0..buf.spec().channels.count() {
                                samples.push(buf.chan(ch)[frame] as f32 / u8::MAX as f32);
                            }
                        }
                    }
                    AudioBufferRef::S24(buf) => {
                        for frame in 0..buf.frames() {
                            for ch in 0..buf.spec().channels.count() {
                                let val = buf.chan(ch)[frame];
                                let sample_f32 = val.inner() as f32 / (1 << 23) as f32;
                                samples.push(sample_f32);
                            }
                        }
                    }
                    AudioBufferRef::F64(buf) => {
                        for frame in 0..buf.frames() {
                            for ch in 0..buf.spec().channels.count() {
                                samples.push(buf.chan(ch)[frame] as f32);
                            }
                        }
                    }
                    AudioBufferRef::S32(buf) => {
                        for frame in 0..buf.frames() {
                            for ch in 0..buf.spec().channels.count() {
                                samples.push(buf.chan(ch)[frame] as f32 / i32::MAX as f32);
                            }
                        }
                    }
                    _ => {
                        log::debug!("Unsupported buffer format");
                        continue;
                    }
                }

                decode_buffer.lock().unwrap().extend(samples);

                // Adaptive buffering: only sleep if we have enough samples buffered
                // Target: keep 2-3 seconds of audio buffered to prevent underruns
                let target_buffer_size = (sample_rate as usize) * (channels) * 2;
                let current_size = decode_buffer.lock().unwrap().len();

                // If paused, sleep and wait instead of filling the buffer
                if paused_flag_decoder.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }

                if current_size > target_buffer_size {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }

            // Decoding is finished!
            log::debug!("Finished decoding, setting decoder_done = true");
            decoder_done_for_thread.store(true, Ordering::SeqCst);
        });

        self.handle = Some(handle);
        self.stream = Some(stream);
        self.buffer = buffer;

        Ok(())
    }

    pub fn stop(&mut self) {
        self.stream = None;
        self.is_playing = false;
        self.current_path = None;
        self.buffer.lock().unwrap().clear();
    }

    pub fn is_loaded(&self) -> bool {
        self.current_path.is_some()
    }

    pub fn is_done(&self) -> bool {
        self.buffer.lock().unwrap().is_empty() && self.is_playing
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.is_paused = paused;
        self.paused_flag.store(paused, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_paused_flag_stops_buffer_filling() {
        // Simulate the decoder thread behavior with pause flag
        let buffer: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let paused_flag = Arc::new(AtomicBool::new(false));

        let buffer_clone = Arc::clone(&buffer);
        let paused_clone = Arc::clone(&paused_flag);

        // Spawn a thread that simulates the decoder
        let handle = thread::spawn(move || {
            for i in 0..10 {
                // Check pause flag like the real decoder does
                if paused_clone.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }

                // Simulate adding samples
                buffer_clone.lock().unwrap().push_back(i as f32);
                thread::sleep(Duration::from_millis(10));
            }
        });

        // Let it run for a bit
        thread::sleep(Duration::from_millis(25));

        // Pause it
        paused_flag.store(true, Ordering::SeqCst);
        let size_when_paused = buffer.lock().unwrap().len();

        // Wait while paused - buffer should not grow
        thread::sleep(Duration::from_millis(50));
        let size_while_paused = buffer.lock().unwrap().len();

        // Unpause
        paused_flag.store(false, Ordering::SeqCst);

        // Wait for completion
        handle.join().unwrap();

        let final_size = buffer.lock().unwrap().len();

        // Assert that buffer didn't grow while paused
        assert_eq!(
            size_when_paused, size_while_paused,
            "Buffer should not grow while paused"
        );

        // Assert that buffer continued growing after unpause
        assert!(
            final_size > size_while_paused,
            "Buffer should grow after unpause"
        );
    }

    #[test]
    fn test_set_paused_updates_flag() {
        let mut player = Player::new();

        assert!(!player.is_paused);
        assert!(!player.paused_flag.load(Ordering::SeqCst));

        player.set_paused(true);
        assert!(player.is_paused);
        assert!(player.paused_flag.load(Ordering::SeqCst));

        player.set_paused(false);
        assert!(!player.is_paused);
        assert!(!player.paused_flag.load(Ordering::SeqCst));
    }
}
