//! The Speech Model: Breeze ASR 25, a Whisper tuned for Taiwanese Mandarin
//! with English mixed in, run by whisper.cpp on the GPU when the build has
//! one. It is loaded once and kept warm for the daemon's lifetime, so a
//! session never waits on a load.
//!
//! One thread owns the model. Live drafts are best-effort: when several are
//! queued only the newest runs, and a final pass always runs.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use talkify_core::level::trailing_silence;
use talkify_core::live::Word;
use talkify_core::protocol::EngineStatus;
use talkify_speech::{Heard, Speech};

use super::Msg;
use super::models::Model;

/// Past this, a live draft decodes only the most recent stretch.
const LIVE_WINDOW_SECONDS: usize = 30;
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
  let mut loaded: Option<Speech> = None;
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
          let mut pass = Pass::from(speech.decode(&samples[window..], &hints));
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
          let mut whole = Pass::from(speech.transcribe(&samples, &hints));
          whole.seconds = samples.len() as f32 / 16_000.0;
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

fn load(model: &'static Model, events: &Sender<Msg>) -> Option<Speech> {
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
  match Speech::open(&model.weights()) {
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
  let mut speech = Speech::open(&model.weights())
    .map_err(|error| anyhow::anyhow!("load {}: {error}", model.id))?;
  Ok(speech.transcribe(samples, "").text)
}

impl From<Heard> for Pass {
  fn from(heard: Heard) -> Self {
    Self {
      text: heard.text,
      words: heard.words,
      ..Self::default()
    }
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
