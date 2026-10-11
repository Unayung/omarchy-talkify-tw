//! The Speech Model: Breeze ASR 25, a Whisper tuned for Taiwanese Mandarin
//! with English mixed in, run by whisper.cpp on the GPU when the build has
//! one. Shared by the Linux daemon and the library the Mac app links, so both
//! hear the same words.

use std::path::Path;

use talkify_core::chunk;
use talkify_core::live::{Word, join};
use talkify_core::pieces::{self, Piece};
use whisper_rs::{
  DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
  WhisperContextParameters, WhisperState,
};

pub use whisper_rs::WhisperError;

/// A recording whose loudest sample stays under this (about -66 dB) is
/// silence.
const SILENCE_PEAK: f32 = 0.0005;
/// Speed stops improving past six threads on a 13600KF.
const THREADS: i32 = 6;
/// whisper.cpp refuses less than a second of audio; shorter is padded.
const SHORTEST: usize = 16_800;
/// Mandarin, which is what Breeze was tuned on; English inside it is still
/// written as English.
const LANGUAGE: &str = "zh";
/// The encoder hears in frames of 20 ms and by default works through thirty
/// seconds of them however short the recording, which on a Mac is a second
/// gone before the first word. It is given the recording's length and some
/// to spare, and never less than half its full window: below that the model
/// starts mishearing short English ("hello world" as "the low world").
const SPARE_FRAMES: usize = 128;
const FEWEST_FRAMES: usize = 768;
const FRAME_SAMPLES: usize = 320;
/// Recordings longer than this are decoded in pieces cut at pauses.
const PIECE_SECONDS: usize = 60;

/// What one stretch of a 16 kHz recording was heard to say.
#[derive(Debug, Default)]
pub struct Heard {
  pub text: String,
  /// Timed from the start of the stretch.
  pub words: Vec<Word>,
}

/// The loaded model. One for the process's lifetime: it holds the decoder's
/// buffers on the GPU, and making one per pass would allocate them every
/// time.
pub struct Speech(WhisperState);

impl Speech {
  pub fn open(weights: &Path) -> Result<Self, WhisperError> {
    // whisper.cpp prints its own log to stderr unless it is hooked.
    whisper_rs::install_logging_hooks();
    // Dynamic time warping over the decoder's attention says when each token
    // ended; whisper.cpp's default timestamps drift by a second around pauses.
    // Breeze is a large-v2 underneath and keeps its alignment heads.
    let mut params = WhisperContextParameters::default();
    params.dtw_parameters(DtwParameters {
      mode: DtwMode::ModelPreset {
        model_preset: DtwModelPreset::LargeV2,
      },
      ..Default::default()
    });
    Ok(Self(
      WhisperContext::new_with_params(weights, params)?.create_state()?,
    ))
  }

  /// A whole recording, however long: decoded in pieces cut at pauses, so
  /// the model's memory stays flat.
  pub fn transcribe(&mut self, samples: &[f32], hints: &str) -> Heard {
    let mut whole = Heard::default();
    for range in chunk::split(samples, PIECE_SECONDS, 10) {
      let offset = range.start as f32 / 16_000.0;
      let mut piece = self.decode(&samples[range], hints);
      for word in &mut piece.words {
        word.start += offset;
        word.end += offset;
      }
      whole.words.append(&mut piece.words);
    }
    whole.text = join(whole.words.iter().map(|word| word.text.as_str()));
    whole
  }

  /// One stretch, short enough to decode at once. `hints` are words the
  /// model is told to expect.
  pub fn decode(&mut self, samples: &[f32], hints: &str) -> Heard {
    // Under a quarter second there is no word to find, and the model
    // sometimes invents one.
    if samples.len() < 4_000 {
      return Heard::default();
    }
    // Silence has no words, but the model will still offer "Yeah." or
    // "Thank you." for it.
    if samples.iter().all(|sample| sample.abs() < SILENCE_PEAK) {
      return Heard::default();
    }
    // One hypothesis at a time. A beam of five wrote the same words on every
    // test recording, differing only in capitals and commas, and took half
    // as long again to do it on Metal.
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(LANGUAGE));
    params.set_n_threads(THREADS);
    // Each pass stands alone: text carried over from the one before is how
    // Whisper starts repeating itself.
    params.set_no_context(true);
    params.set_suppress_nst(true);
    // The Vocabulary's words, so names and technical terms are spelled right
    // as they are heard rather than mended afterwards.
    if !hints.is_empty() {
      params.set_initial_prompt(hints);
    }
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    let mut padded;
    let audio = if samples.len() < SHORTEST {
      padded = samples.to_vec();
      padded.resize(SHORTEST, 0.0);
      &padded[..]
    } else {
      samples
    };
    let frames = (audio.len() / FRAME_SAMPLES + SPARE_FRAMES).max(FEWEST_FRAMES);
    // Past the full window whisper.cpp slides it along the recording itself.
    if frames < 1500 {
      params.set_audio_ctx(frames as i32);
    }
    if let Err(error) = self.0.full(params, audio) {
      tracing::error!("transcription failed: {error}");
      return Heard::default();
    }
    // The tokens' bytes are borrowed from a segment that does not outlive its
    // turn of the loop.
    let mut heard: Vec<(Vec<u8>, f32)> = Vec::new();
    for segment in self.0.as_iter() {
      for index in 0..segment.n_tokens() {
        let Some(token) = segment.get_token(index) else {
          continue;
        };
        let Ok(bytes) = token.to_bytes() else {
          continue;
        };
        // "[_BEG_]", "[_TT_150]": the model's own markers, not speech.
        if bytes.starts_with(b"[_") {
          continue;
        }
        heard.push((bytes.to_vec(), token.token_data().t_dtw as f32 / 100.0));
      }
    }
    let heard: Vec<Piece> = heard
      .iter()
      .map(|(bytes, end)| Piece { bytes, end: *end })
      .collect();
    let words = pieces::words(&heard, &talkify_core::level::quiet(samples));
    Heard {
      text: join(words.iter().map(|word| word.text.as_str())),
      words,
    }
  }
}
