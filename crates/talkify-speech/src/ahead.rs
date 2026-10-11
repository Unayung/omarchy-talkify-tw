//! Hearing ahead of the release.
//!
//! A speaker stops talking a moment before letting go of the trigger. Once
//! the recording has gone quiet the Speech Model starts on it, and if nothing
//! more is said that pass is the dictation: the wait after the release is
//! shorter by however long the speaker lingered. Speech after the quiet
//! means the guess was early; it is stopped and the next quiet tries again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use talkify_core::level;

use crate::{Heard, Speech};

/// Quiet this long and the speaker may be done. Shorter and every breath
/// starts a pass; longer and there is little of the wait left to save.
const SETTLED: usize = 4_000;
/// Less sound than this is a click, not a word.
const WORTH_HEARING: usize = 4_800;

pub struct Ahead {
  speech: Arc<Mutex<Speech>>,
  hints: String,
  samples: Vec<f32>,
  guess: Option<Guess>,
}

/// A pass over the recording as it stood when it went quiet.
struct Guess {
  upto: usize,
  stop: Arc<AtomicBool>,
  heard: JoinHandle<Heard>,
}

impl Ahead {
  pub fn new(speech: Arc<Mutex<Speech>>) -> Self {
    Self {
      speech,
      hints: String::new(),
      samples: Vec::new(),
      guess: None,
    }
  }

  /// A new recording; `hints` are the words the model is told to expect.
  pub fn begin(&mut self, hints: String) {
    self.forget();
    self.samples.clear();
    self.hints = hints;
  }

  /// More of the recording, 16 kHz mono, as it arrives.
  pub fn feed(&mut self, samples: &[f32]) {
    self.samples.extend_from_slice(samples);
    let quiet = (level::trailing_silence(&self.samples) * 16_000.0) as usize;
    let sound_until = self.samples.len() - quiet;
    if self
      .guess
      .as_ref()
      .is_some_and(|guess| sound_until > guess.upto)
    {
      self.forget();
    }
    if self.guess.is_none() && quiet >= SETTLED && sound_until >= WORTH_HEARING {
      let stop = Arc::new(AtomicBool::new(false));
      self.guess = Some(Guess {
        upto: self.samples.len(),
        stop: stop.clone(),
        heard: hear(
          self.speech.clone(),
          self.samples.clone(),
          self.hints.clone(),
          stop,
        ),
      });
    }
  }

  /// The recording is over: what was said, from the guess when it still
  /// holds and from a pass over all of it otherwise. Handed back still
  /// running, so the caller waits for it without holding up the next
  /// recording.
  pub fn finish(&mut self) -> JoinHandle<Heard> {
    let samples = std::mem::take(&mut self.samples);
    match self.guess.take() {
      Some(guess) => guess.heard,
      None => hear(
        self.speech.clone(),
        samples,
        std::mem::take(&mut self.hints),
        Arc::default(),
      ),
    }
  }

  fn forget(&mut self) {
    if let Some(guess) = self.guess.take() {
      guess.stop.store(true, Ordering::Relaxed);
    }
  }
}

/// One pass on a thread of its own. A stopped pass that was still waiting
/// for the model gives up as soon as it gets it.
fn hear(
  speech: Arc<Mutex<Speech>>,
  samples: Vec<f32>,
  hints: String,
  stop: Arc<AtomicBool>,
) -> JoinHandle<Heard> {
  std::thread::spawn(move || {
    let mut speech = speech
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner());
    speech.stop_when(stop);
    let heard = speech.transcribe(&samples, &hints);
    // Whoever uses the model next is not this pass's to stop.
    speech.stop_when(Arc::default());
    heard
  })
}
