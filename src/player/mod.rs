use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use cpal::{
    Stream, StreamError,
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

use anyhow::{Context, Result, anyhow};

const AUDIO_STREAM_ATTEMPTS: usize = 3;
const AUDIO_STREAM_RETRY_DELAY: Duration = Duration::from_millis(150);

fn record_stream_failure(
    failed: &AtomicBool,
    error_slot: &Mutex<Option<String>>,
    message: String,
) -> bool {
    if failed.swap(true, Ordering::SeqCst) {
        return false;
    }
    *error_slot.lock().unwrap() = Some(message);
    true
}

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
    decoder_stop: Option<Arc<AtomicBool>>,
    stream_failed: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
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
            decoder_stop: None,
            stream_failed: Arc::new(AtomicBool::new(false)),
            stream_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn play(&mut self, path: &Path) -> Result<()> {
        self.stop(); // Stop any current playback

        self.autoplay_trigger.store(false, Ordering::SeqCst);
        self.is_decoder_done.store(false, Ordering::SeqCst);
        self.stream_failed.store(false, Ordering::SeqCst);
        *self.stream_error.lock().unwrap() = None;

        let file = File::open(path)
            .with_context(|| format!("failed to open audio file {}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let probed = get_probe()
            .format(
                &Default::default(),
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .with_context(|| format!("unsupported audio format in {}", path.display()))?;

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("no supported audio track found in {}", path.display()))?;

        let mut decoder = get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .with_context(|| format!("unsupported audio codec in {}", path.display()))?;

        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

        let config = cpal::StreamConfig {
            channels: channels as u16,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));

        let sample_buf = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let decoder_done_for_thread = Arc::clone(&self.is_decoder_done);
        let mut stream = None;
        let mut last_stream_error = None;

        for attempt in 1..=AUDIO_STREAM_ATTEMPTS {
            // Reacquire the default device on every attempt. Audio servers can
            // replace the device while a stream is paused or an output is
            // disconnected.
            let host = cpal::default_host();
            let Some(device) = host.default_output_device() else {
                last_stream_error = Some(anyhow!("no audio output device available"));
                if attempt < AUDIO_STREAM_ATTEMPTS {
                    thread::sleep(AUDIO_STREAM_RETRY_DELAY);
                }
                continue;
            };

            let callback_buffer = Arc::clone(&sample_buf);
            let callback_autoplay = Arc::clone(&self.autoplay_trigger);
            let callback_decoder_done = Arc::clone(&self.is_decoder_done);
            let callback_paused = Arc::clone(&self.paused_flag);
            let callback_stream_failed = Arc::clone(&self.stream_failed);
            let callback_stream_error = Arc::clone(&self.stream_error);
            let candidate = device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    let mut buf = callback_buffer.lock().unwrap();

                    if callback_paused.load(Ordering::SeqCst) {
                        for sample in data.iter_mut() {
                            *sample = 0.0;
                        }
                        return;
                    }

                    for sample in data.iter_mut() {
                        *sample = buf.pop_front().unwrap_or(0.0);
                    }

                    if buf.is_empty() && callback_decoder_done.load(Ordering::SeqCst) {
                        callback_autoplay.store(true, Ordering::SeqCst);
                    }
                },
                move |err| {
                    if matches!(err, StreamError::BufferUnderrun) {
                        log::debug!("CPAL buffer underrun");
                        return;
                    }

                    // CPAL may report a broken backend repeatedly. Preserve
                    // the first useful error for the UI instead of turning an
                    // audio failure into a CPU and disk exhaustion incident.
                    let message = err.to_string();
                    if record_stream_failure(
                        &callback_stream_failed,
                        &callback_stream_error,
                        message.clone(),
                    ) {
                        log::error!("CPAL stream failed: {message}");
                    }
                },
                None,
            );

            match candidate {
                Ok(candidate) => match candidate.play() {
                    Ok(()) => {
                        stream = Some(candidate);
                        break;
                    }
                    Err(error) => {
                        last_stream_error =
                            Some(anyhow!("failed to start audio output stream: {error}"));
                    }
                },
                Err(error) => {
                    last_stream_error =
                        Some(anyhow!("failed to build audio output stream: {error}"));
                }
            }

            if attempt < AUDIO_STREAM_ATTEMPTS {
                log::warn!(
                    "Audio output attempt {attempt}/{AUDIO_STREAM_ATTEMPTS} failed; retrying"
                );
                thread::sleep(AUDIO_STREAM_RETRY_DELAY);
            }
        }

        let stream = stream.ok_or_else(|| {
            last_stream_error.unwrap_or_else(|| anyhow!("audio output stream unavailable"))
        })?;

        self.is_playing = true;
        self.current_path = Some(path.to_path_buf());

        // Spawn decoding thread
        let decode_buffer = Arc::clone(&sample_buf);
        let paused_flag_decoder = Arc::clone(&self.paused_flag);
        let decoder_stop = Arc::new(AtomicBool::new(false));
        let decoder_stop_for_thread = Arc::clone(&decoder_stop);
        let handle = thread::spawn(move || {
            while let Ok(packet) = format.next_packet() {
                if decoder_stop_for_thread.load(Ordering::SeqCst) {
                    break;
                }
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
                    config.sample_rate,
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
        self.decoder_stop = Some(decoder_stop);
        self.stream = Some(stream);
        self.buffer = buffer;
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(stop) = self.decoder_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.stream.take();
        if let Some(handle) = self.handle.take()
            && let Err(error) = handle.join()
        {
            log::error!("Decoder thread failed during shutdown: {error:?}");
        }
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

    pub fn take_stream_error(&mut self) -> Option<String> {
        self.stream_error.lock().unwrap().take()
    }

    /// Change playback pause state. Returns true when a failed resume required
    /// rebuilding the current track's output stream from the beginning.
    pub fn set_paused(&mut self, paused: bool) -> Result<bool> {
        // Pause the OS-level CPAL stream too, not just the flag. Without this
        // the stream stays alive and keeps feeding silence to the device, so
        // the audio sink reports RUNNING / uncorked even while paused — which
        // blocks power management (e.g. screen-idle inhibitors that key off
        // cork state). Pausing the stream lets the device go idle. The
        // paused_flag still gates the callback/decoder for a clean resume.
        if paused {
            if let Some(stream) = &self.stream {
                stream
                    .pause()
                    .context("failed to pause audio output stream")?;
            }
            self.is_paused = true;
            self.paused_flag.store(true, Ordering::SeqCst);
            return Ok(false);
        }

        let resume_failed = self.stream.as_ref().is_some_and(|stream| {
            stream
                .play()
                .inspect_err(|error| log::warn!("Failed to resume CPAL stream: {error}"))
                .is_err()
        });
        let needs_rebuild = self.stream_failed.load(Ordering::SeqCst)
            || resume_failed
            || (self.stream.is_none() && self.current_path.is_some());

        if needs_rebuild {
            let path = self
                .current_path
                .clone()
                .ok_or_else(|| anyhow!("cannot rebuild audio output without a current track"))?;
            if let Err(error) = self.play(&path) {
                // Keep enough state to let a later resume retry after the
                // system audio service or physical device returns.
                self.current_path = Some(path);
                self.is_paused = true;
                self.paused_flag.store(true, Ordering::SeqCst);
                return Err(error).context("failed to rebuild audio output after resume failure");
            }
        }

        self.is_paused = false;
        self.paused_flag.store(false, Ordering::SeqCst);
        Ok(needs_rebuild)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
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
    fn repeated_stream_failures_preserve_only_the_first_error() {
        let failed = AtomicBool::new(false);
        let error = Mutex::new(None);

        assert!(record_stream_failure(&failed, &error, "first".into()));
        assert!(!record_stream_failure(&failed, &error, "second".into()));
        assert_eq!(error.lock().unwrap().as_deref(), Some("first"));
    }

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

        assert!(!player.set_paused(true).unwrap());
        assert!(player.is_paused);
        assert!(player.paused_flag.load(Ordering::SeqCst));

        assert!(!player.set_paused(false).unwrap());
        assert!(!player.is_paused);
        assert!(!player.paused_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn missing_audio_file_returns_error_without_loading_player() {
        let mut player = Player::new();
        let path = Path::new("/definitely/not/a/shelltrax-track.mp3");

        let error = player.play(path).unwrap_err();

        assert!(error.to_string().contains("failed to open audio file"));
        assert!(!player.is_loaded());
        assert!(!player.is_playing);
    }
}
