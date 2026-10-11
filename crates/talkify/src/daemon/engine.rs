//! The Speech Model: Breeze ASR 25, a Whisper tuned for Taiwanese Mandarin
//! with English mixed in, run by whisper.cpp on the GPU when the build has
//! one. It is loaded once and kept warm for the daemon's lifetime, so a
//! session never waits on a load.
//!
//! One thread owns the model. Live drafts are best-effort: when several are
//! queued only the newest runs, and a final pass always runs.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use talkify_core::chunk;
use talkify_core::level::trailing_silence;
use talkify_core::live::{Word, gap};
use talkify_core::pieces::{self, Piece};
use talkify_core::protocol::EngineStatus;

use super::Msg;
use super::models::Model;
use whisper_rs::{
  FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

/// Past this, a live draft decodes only the most recent stretch.
const LIVE_WINDOW_SECONDS: usize = 30;
/// Final passes longer than this are decoded in pieces cut at pauses.
const PIECE_SECONDS: usize = 60;
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

/// `offset` is where `samples` start in the session's recording, in
/// samples. Live insertion decodes only the part after the last commit, and
/// a result for an offset that has since moved is stale.
pub enum Job {
  Load,
  Live {
    id: u64,
    offset: usize,
    samples: Vec<f32>,
    hints: String,
  },
  Final {
    id: u64,
    offset: usize,
    samples: Vec<f32>,
    hints: String,
  },
}

/// One decode of a stretch of the recording.
#[derive(Debug, Default)]
pub struct Pass {
  pub text: String,
  /// Timed from the start of the decoded samples.
  pub words: Vec<Word>,
  pub seconds: f32,
  pub trailing_silence: f32,
}

#[derive(Clone)]
pub struct Engine {
  jobs: Sender<Job>,
}

impl Engine {
  pub fn spawn(model: &'static Model, events: Sender<Msg>) -> Self {
    let (jobs, rx) = crossbeam_channel::unbounded();
    std::thread::Builder::new()
      .name("engine".into())
      .spawn(move || run(model, rx, events))
      .expect("spawn engine thread");
    let engine = Self { jobs };
    engine.send(Job::Load);
    engine
  }

  pub fn send(&self, job: Job) {
    let _ = self.jobs.send(job);
  }
}

fn status(events: &Sender<Msg>, status: EngineStatus, detail: impl Into<String>, progress: f32) {
  let _ = events.send(Msg::EngineStatus {
    status,
    detail: detail.into(),
    progress,
  });
}

fn run(model: &'static Model, jobs: Receiver<Job>, events: Sender<Msg>) {
  let mut loaded: Option<WhisperState> = None;
  while let Ok(first) = jobs.recv() {
    for job in newest_live_only(first, &jobs) {
      match job {
        Job::Load => {
          if loaded.is_none() {
            loaded = load(model, &events);
          }
        }
        Job::Live {
          id,
          offset,
          samples,
          hints,
        } => {
          let Some(speech) = loaded.as_mut() else {
            continue;
          };
          let window = samples.len().saturating_sub(LIVE_WINDOW_SECONDS * 16_000);
          let started = Instant::now();
          let mut pass = decode(speech, &samples[window..], &hints);
          shift(&mut pass.words, window);
          if window > 0 {
            pass.text = format!("…{}", pass.text);
          }
          pass.seconds = samples.len() as f32 / 16_000.0;
          pass.trailing_silence = trailing_silence(&samples);
          send_pass(&events, id, false, offset, pass, started.elapsed());
        }
        Job::Final {
          id,
          offset,
          samples,
          hints,
        } => {
          let Some(speech) = loaded.as_mut() else {
            send_pass(&events, id, true, offset, Pass::default(), Duration::ZERO);
            continue;
          };
          let started = Instant::now();
          let mut whole = Pass {
            seconds: samples.len() as f32 / 16_000.0,
            ..Pass::default()
          };
          for range in chunk::split(&samples, PIECE_SECONDS, 10) {
            let start = range.start;
            let mut piece = decode(speech, &samples[range], &hints);
            shift(&mut piece.words, start);
            if !piece.text.is_empty() {
              whole.text.push_str(gap(&whole.text, &piece.text));
              whole.text.push_str(&piece.text);
            }
            whole.words.append(&mut piece.words);
          }
          send_pass(&events, id, true, offset, whole, started.elapsed());
        }
      }
    }
  }
}

fn shift(words: &mut [Word], samples: usize) {
  let seconds = samples as f32 / 16_000.0;
  for word in words {
    word.start += seconds;
    word.end += seconds;
  }
}

/// Everything queued, with all but the newest live draft dropped. A live
/// draft is stale the moment a newer one exists.
fn newest_live_only(first: Job, jobs: &Receiver<Job>) -> Vec<Job> {
  let mut queued: Vec<Job> = std::iter::once(first).chain(jobs.try_iter()).collect();
  let newest_live = queued
    .iter()
    .rposition(|job| matches!(job, Job::Live { .. }));
  let mut index = 0;
  queued.retain(|job| {
    let keep = !matches!(job, Job::Live { .. }) || Some(index) == newest_live;
    index += 1;
    keep
  });
  queued
}

fn load(model: &'static Model, events: &Sender<Msg>) -> Option<WhisperState> {
  if !model.is_complete() {
    status(events, EngineStatus::Downloading, model.id, 0.0);
    let progress_events = events.clone();
    let downloaded = model.download(|fraction| {
      status(
        &progress_events,
        EngineStatus::Downloading,
        model.id,
        fraction,
      );
    });
    if let Err(error) = downloaded {
      tracing::error!("model download failed: {error:#}");
      status(
        events,
        EngineStatus::Failed,
        format!("Download failed: {error:#}"),
        0.0,
      );
      return None;
    }
  }
  status(events, EngineStatus::Loading, model.id, 0.0);
  let started = Instant::now();
  match open(model) {
    Ok(speech) => {
      tracing::info!("{} loaded in {:?}", model.id, started.elapsed());
      status(events, EngineStatus::Ready, model.id, 1.0);
      Some(speech)
    }
    Err(error) => {
      tracing::error!("model load failed: {error}");
      status(
        events,
        EngineStatus::Failed,
        format!("Could not load {}: {error}", model.id),
        0.0,
      );
      None
    }
  }
}

/// Loads the model and decodes one recording, outside the daemon: what
/// `talkify transcribe` runs.
pub fn transcribe_once(model: &Model, samples: &[f32]) -> anyhow::Result<String> {
  let mut speech = open(model).map_err(|error| anyhow::anyhow!("load {}: {error}", model.id))?;
  let pieces: Vec<String> = chunk::split(samples, PIECE_SECONDS, 10)
    .into_iter()
    .map(|range| decode(&mut speech, &samples[range], "").text)
    .collect();
  Ok(talkify_core::live::join(pieces.iter().map(String::as_str)))
}

/// One state for the daemon's lifetime: it holds the decoder's buffers on
/// the GPU, and making one per pass would allocate them every time.
fn open(model: &Model) -> Result<WhisperState, whisper_rs::WhisperError> {
  // whisper.cpp prints its own log to stderr unless it is hooked.
  whisper_rs::install_logging_hooks();
  // Dynamic time warping over the decoder's attention says when each token
  // ended; whisper.cpp's default timestamps drift by a second around pauses.
  // Breeze is a large-v2 underneath and keeps its alignment heads.
  let mut params = WhisperContextParameters::default();
  params.dtw_parameters(whisper_rs::DtwParameters {
    mode: whisper_rs::DtwMode::ModelPreset {
      model_preset: whisper_rs::DtwModelPreset::LargeV2,
    },
    ..Default::default()
  });
  WhisperContext::new_with_params(model.weights(), params)?.create_state()
}

fn decode(speech: &mut WhisperState, samples: &[f32], hints: &str) -> Pass {
  // Under a quarter second there is no word to find, and the model
  // sometimes invents one.
  if samples.len() < 4_000 {
    return Pass::default();
  }
  // Silence has no words, but the model will still offer "Yeah." or
  // "Thank you." for it.
  if samples.iter().all(|sample| sample.abs() < SILENCE_PEAK) {
    return Pass::default();
  }
  let mut params = FullParams::new(SamplingStrategy::BeamSearch {
    beam_size: 5,
    patience: -1.0,
  });
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
  if let Err(error) = speech.full(params, audio) {
    tracing::error!("transcription failed: {error}");
    return Pass::default();
  }
  // The tokens' bytes are borrowed from a segment that does not outlive its
  // turn of the loop.
  let mut heard: Vec<(Vec<u8>, f32)> = Vec::new();
  for segment in speech.as_iter() {
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
      let data = token.token_data();
      heard.push((bytes.to_vec(), data.t_dtw as f32 / 100.0));
    }
  }
  let heard: Vec<Piece> = heard
    .iter()
    .map(|(bytes, end)| Piece { bytes, end: *end })
    .collect();
  let words = pieces::words(&heard, &talkify_core::level::quiet(samples));
  Pass {
    text: talkify_core::live::join(words.iter().map(|word| word.text.as_str())),
    words,
    ..Pass::default()
  }
}

fn send_pass(
  events: &Sender<Msg>,
  id: u64,
  is_final: bool,
  offset: usize,
  pass: Pass,
  took: Duration,
) {
  tracing::debug!(
    "{} pass for session {id} took {took:?}: {:?}",
    if is_final { "final" } else { "live" },
    pass.text
  );
  tracing::trace!("{:?}", pass.words);
  let _ = events.send(Msg::Transcribed {
    id,
    is_final,
    offset,
    pass,
  });
}
