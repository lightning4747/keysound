use rodio::source::Source;
use rodio::static_buffer::StaticSamplesBuffer;
use rodio::DeviceSinkBuilder;
use std::fs::File;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub type ChannelCount = NonZeroU16;
pub type SampleRate = NonZeroU32;

#[derive(Clone, Copy)]
pub struct SoundData {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub samples: &'static [f32],
}

pub fn load_sound(path: &Path, volume: f32, max_ms: Option<u64>) -> Result<SoundData, String> {
    let file = File::open(path)
        .map_err(|e| format!("Cannot open sound file '{}': {}", path.display(), e))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("Failed to read metadata for '{}': {}", path.display(), e))?;

    let ext_str = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut builder = rodio::Decoder::builder()
        .with_data(file)
        .with_byte_len(meta.len());

    if !ext_str.is_empty() {
        builder = builder.with_hint(&ext_str);
    }

    let is_mp4 = ext_str == "mp4" || ext_str == "m4a";

    let decoder = match builder.build() {
        Ok(d) => d,
        Err(e) => {
            if is_mp4 {
                return Err(format!(
                    "Failed to decode audio track from '{}': {}\nTip: ffmpeg -i \"{}\" -vn out.mp3",
                    path.display(),
                    e,
                    path.display()
                ));
            } else {
                return Err(format!(
                    "Failed to decode audio file '{}': {}",
                    path.display(),
                    e
                ));
            }
        }
    };

    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();

    let max_samples = max_ms.map(|ms| {
        let max_frames = (sample_rate.get() as u64 * ms) / 1000;
        (max_frames * channels.get() as u64) as usize
    });

    let mut samples = Vec::new();
    for sample in decoder {
        samples.push(sample * volume);
        if let Some(limit) = max_samples {
            if samples.len() >= limit {
                break;
            }
        }
    }

    if samples.is_empty() {
        if is_mp4 {
            return Err(format!(
                "No audio samples decoded from '{}'.\nTip: ffmpeg -i \"{}\" -vn out.mp3",
                path.display(),
                path.display()
            ));
        } else {
            return Err(format!("No audio samples decoded from '{}'.", path.display()));
        }
    }

    let static_samples: &'static [f32] = Box::leak(samples.into_boxed_slice());

    Ok(SoundData {
        channels,
        sample_rate,
        samples: static_samples,
    })
}

pub fn validate_sound(path: &Path) -> Result<(), String> {
    let file = File::open(path)
        .map_err(|e| format!("Cannot open sound file '{}': {}", path.display(), e))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("Failed to read metadata for '{}': {}", path.display(), e))?;

    let ext_str = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let mut builder = rodio::Decoder::builder()
        .with_data(file)
        .with_byte_len(meta.len());

    if !ext_str.is_empty() {
        builder = builder.with_hint(&ext_str);
    }

    let is_mp4 = ext_str == "mp4" || ext_str == "m4a";

    let mut decoder = match builder.build() {
        Ok(d) => d,
        Err(e) => {
            if is_mp4 {
                return Err(format!(
                    "Failed to decode audio track from '{}': {}\nTip: ffmpeg -i \"{}\" -vn out.mp3",
                    path.display(),
                    e,
                    path.display()
                ));
            } else {
                return Err(format!(
                    "Failed to decode audio file '{}': {}",
                    path.display(),
                    e
                ));
            }
        }
    };

    if decoder.next().is_none() {
        if is_mp4 {
            return Err(format!(
                "No audio samples found in '{}'.\nTip: ffmpeg -i \"{}\" -vn out.mp3",
                path.display(),
                path.display()
            ));
        } else {
            return Err(format!("No audio samples found in '{}'.", path.display()));
        }
    }

    Ok(())
}

pub struct CancellableSource {
    inner: StaticSamplesBuffer,
    generation: Arc<AtomicU64>,
    my_gen: u64,
    channel_idx: usize,
    num_channels: usize,
}

impl Source for CancellableSource {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)
    }
}

impl Iterator for CancellableSource {
    type Item = rodio::Sample;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // Cut out immediately if a newer keystroke has occurred
        if self.channel_idx == 0 && self.generation.load(Ordering::Relaxed) != self.my_gen {
            return None;
        }

        match self.inner.next() {
            Some(sample) => {
                self.channel_idx = (self.channel_idx + 1) % self.num_channels;
                Some(sample)
            }
            None => None,
        }
    }
}

pub struct AudioManager {
    _sink: rodio::MixerDeviceSink,
    mixer: rodio::mixer::Mixer,
    sound: SoundData,
    generation: Arc<AtomicU64>,
    overlap: bool,
}

impl AudioManager {
    pub fn new(sound: SoundData, overlap: bool) -> Result<Self, String> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Failed to open default audio sink: {e}"))?;
        sink.log_on_drop(false);
        let mixer = sink.mixer().clone();
        Ok(Self {
            _sink: sink,
            mixer,
            sound,
            generation: Arc::new(AtomicU64::new(0)),
            overlap,
        })
    }

    #[inline]
    pub fn play(&self) {
        let source =
            StaticSamplesBuffer::new(self.sound.channels, self.sound.sample_rate, self.sound.samples);
        if self.overlap {
            self.mixer.add(source);
        } else {
            let my_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
            let cancellable = CancellableSource {
                inner: source,
                generation: Arc::clone(&self.generation),
                my_gen,
                channel_idx: 0,
                num_channels: self.sound.channels.get() as usize,
            };
            self.mixer.add(cancellable);
        }
    }
}
