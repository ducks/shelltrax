use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use cpal::{
    Stream, StreamConfig, StreamError,
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
const DECODER_BACKPRESSURE_DELAY: Duration = Duration::from_millis(25);
const BUFFER_TARGET_SECONDS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecoderAction {
    Decode,
    Wait,
    Stop,
}

fn decoder_action(buffered: usize, target: usize, paused: bool, stopped: bool) -> DecoderAction {
    if stopped {
        DecoderAction::Stop
    } else if paused || buffered >= target {
        DecoderAction::Wait
    } else {
        DecoderAction::Decode
    }
}

fn wait_until_decode_ready(
    buffer: &Mutex<VecDeque<f32>>,
    target: usize,
    paused: &AtomicBool,
    stopped: &AtomicBool,
) -> bool {
    loop {
        let action = decoder_action(
            buffer.lock().unwrap().len(),
            target,
            paused.load(Ordering::Relaxed),
            stopped.load(Ordering::Relaxed),
        );
        match action {
            DecoderAction::Decode => return true,
            DecoderAction::Stop => return false,
            DecoderAction::Wait => thread::sleep(DECODER_BACKPRESSURE_DELAY),
        }
    }
}

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
    buffer: Arc<Mutex<VecDeque<f32>>>,
    stream_config: Option<StreamConfig>,
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
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            stream_config: None,
            autoplay_trigger: Arc::new(AtomicBool::new(false)),
            is_decoder_done: Arc::new(AtomicBool::new(false)),
            is_paused: false,
            paused_flag: Arc::new(AtomicBool::new(false)),
            decoder_stop: None,
            stream_failed: Arc::new(AtomicBool::new(false)),
            stream_error: Arc::new(Mutex::new(None)),
        }
    }

    fn build_output_stream(&self, config: &StreamConfig) -> Result<Stream> {
        let mut last_stream_error = None;

        for attempt in 1..=AUDIO_STREAM_ATTEMPTS {
            // Reacquire the default device on every attempt. Audio servers can
            // replace the device while an output is disconnected.
            let host = cpal::default_host();
            let Some(device) = host.default_output_device() else {
                last_stream_error = Some(anyhow!("no audio output device available"));
                if attempt < AUDIO_STREAM_ATTEMPTS {
                    thread::sleep(AUDIO_STREAM_RETRY_DELAY);
                }
                continue;
            };

            let callback_buffer = Arc::clone(&self.buffer);
            let callback_autoplay = Arc::clone(&self.autoplay_trigger);
            let callback_decoder_done = Arc::clone(&self.is_decoder_done);
            let callback_paused = Arc::clone(&self.paused_flag);
            let callback_stream_failed = Arc::clone(&self.stream_failed);
            let callback_stream_error = Arc::clone(&self.stream_error);
            let candidate = device.build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let mut buf = callback_buffer.lock().unwrap();

                    if callback_paused.load(Ordering::SeqCst) {
                        data.fill(0.0);
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
                    Ok(()) => return Ok(candidate),
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

        Err(last_stream_error.unwrap_or_else(|| anyhow!("audio output stream unavailable")))
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
        self.buffer = Arc::new(Mutex::new(VecDeque::new()));
        self.stream_config = Some(config.clone());
        let stream = match self.build_output_stream(&config) {
            Ok(stream) => stream,
            Err(error) => {
                self.stream_config = None;
                return Err(error);
            }
        };

        let sample_buf = Arc::clone(&self.buffer);
        let decoder_done_for_thread = Arc::clone(&self.is_decoder_done);

        self.is_playing = true;
        self.current_path = Some(path.to_path_buf());

        // Spawn decoding thread
        let decode_buffer = Arc::clone(&sample_buf);
        let paused_flag_decoder = Arc::clone(&self.paused_flag);
        let decoder_stop = Arc::new(AtomicBool::new(false));
        let decoder_stop_for_thread = Arc::clone(&decoder_stop);
        let target_buffer_size = sample_rate as usize * channels * BUFFER_TARGET_SECONDS;
        let handle = thread::spawn(move || {
            loop {
                // Decode only when the audio callback has made room. The old
                // fixed 10 ms sleep still let decoding outrun playback and
                // even appended another packet on every paused iteration.
                if !wait_until_decode_ready(
                    &decode_buffer,
                    target_buffer_size,
                    &paused_flag_decoder,
                    &decoder_stop_for_thread,
                ) {
                    break;
                }

                let packet = match format.next_packet() {
                    Ok(packet) => packet,
                    Err(_) => break,
                };
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
            }

            // Decoding is finished!
            log::debug!("Finished decoding, setting decoder_done = true");
            decoder_done_for_thread.store(true, Ordering::SeqCst);
        });

        self.handle = Some(handle);
        self.decoder_stop = Some(decoder_stop);
        self.stream = Some(stream);
        Ok(())
    }

    fn stop_playback_resources(&mut self) {
        if let Some(stop) = self.decoder_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.stream.take();
        if let Some(handle) = self.handle.take()
            && let Err(error) = handle.join()
        {
            log::error!("Decoder thread failed during shutdown: {error:?}");
        }
    }

    pub fn stop(&mut self) {
        self.stop_playback_resources();
        self.is_playing = false;
        self.is_paused = false;
        self.paused_flag.store(false, Ordering::SeqCst);
        self.current_path = None;
        self.stream_config = None;
        self.buffer.lock().unwrap().clear();
    }

    /// Release a failed backend without forgetting which track can be retried.
    ///
    /// ALSA can repeatedly wake CPAL with `POLLERR` after a device disappears.
    /// Merely muting the callback leaves that backend thread spinning, so drop
    /// the stream as soon as the main loop observes the failure. A later resume
    /// rebuilds playback from `current_path` using the new default device.
    pub fn contain_stream_failure(&mut self) {
        self.is_paused = true;
        self.paused_flag.store(true, Ordering::SeqCst);
        self.stop_playback_resources();
        self.is_playing = false;
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

    /// Change playback pause state. Returns true when a failed output required
    /// rebuilding the current track from the beginning.
    pub fn set_paused(&mut self, paused: bool) -> Result<bool> {
        if paused {
            self.is_paused = true;
            self.paused_flag.store(true, Ordering::SeqCst);

            // CPAL's ALSA backend keeps its polling worker alive after
            // Stream::pause(). Some ALSA devices then report immediately with
            // no readable/writable event, making that worker busy-loop at a
            // full CPU core. Dropping only the output stream stops the worker;
            // the decoder and buffered samples remain intact for resume.
            self.stream.take();
            return Ok(false);
        }

        let needs_rebuild = self.stream_failed.load(Ordering::SeqCst)
            || (self.stream.is_none()
                && self.current_path.is_some()
                && self.decoder_stop.is_none());

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
        } else if self.stream.is_none()
            && let Some(config) = self.stream_config.clone()
        {
            self.stream_failed.store(false, Ordering::SeqCst);
            *self.stream_error.lock().unwrap() = None;
            let stream = self
                .build_output_stream(&config)
                .context("failed to recreate paused audio output")?;
            self.stream = Some(stream);
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
    fn decoder_waits_when_paused_or_buffer_is_full() {
        assert_eq!(decoder_action(0, 100, true, false), DecoderAction::Wait);
        assert_eq!(decoder_action(100, 100, false, false), DecoderAction::Wait);
        assert_eq!(decoder_action(101, 100, false, false), DecoderAction::Wait);
    }

    #[test]
    fn decoder_runs_only_below_the_buffer_target() {
        assert_eq!(decoder_action(99, 100, false, false), DecoderAction::Decode);
        assert_eq!(decoder_action(0, 100, false, false), DecoderAction::Decode);
    }

    #[test]
    fn decoder_stop_wins_over_other_states() {
        assert_eq!(decoder_action(0, 100, false, true), DecoderAction::Stop);
        assert_eq!(decoder_action(100, 100, true, true), DecoderAction::Stop);
    }

    #[test]
    fn containing_stream_failure_preserves_track_for_reconnect() {
        let mut player = Player::new();
        let path = PathBuf::from("/music/current.flac");
        player.current_path = Some(path.clone());
        player.is_playing = true;

        let decoder_stop = Arc::new(AtomicBool::new(false));
        let observed_stop = Arc::clone(&decoder_stop);
        player.decoder_stop = Some(decoder_stop);
        player.handle = Some(thread::spawn(move || {
            while !observed_stop.load(Ordering::SeqCst) {
                thread::yield_now();
            }
        }));

        player.contain_stream_failure();

        assert_eq!(player.current_path.as_deref(), Some(path.as_path()));
        assert!(player.is_paused);
        assert!(player.paused_flag.load(Ordering::SeqCst));
        assert!(!player.is_playing);
        assert!(player.decoder_stop.is_none());
        assert!(player.handle.is_none());
        assert!(player.stream.is_none());
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
    fn stop_clears_paused_state() {
        let mut player = Player::new();
        player.set_paused(true).unwrap();

        player.stop();

        assert!(!player.is_paused);
        assert!(!player.paused_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn completion_uses_the_audio_callback_buffer() {
        let mut player = Player::new();
        player.is_playing = true;
        player.buffer.lock().unwrap().push_back(0.5);

        assert!(!player.is_done());
        player.buffer.lock().unwrap().clear();
        assert!(player.is_done());
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
