//! The Speech Model and the text rules behind a C interface, so the Mac app
//! hears and writes exactly what the Linux daemon does. See
//! `include/talkify.h` for the contract.

use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use talkify_core::tidy::{self, TidyOptions};
use talkify_core::vocab::Vocabulary;
use talkify_speech::ahead::Ahead;
use talkify_speech::{Heard, Speech};

pub struct TalkifySpeech {
  speech: Arc<Mutex<Speech>>,
  /// The recording in progress and the Vocabulary it began with.
  session: Mutex<(Ahead, Vocabulary)>,
}

/// # Safety
/// `weights_path` is a NUL-terminated string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_open(weights_path: *const c_char) -> *mut TalkifySpeech {
  let Some(path) = (unsafe { text(weights_path) }) else {
    return std::ptr::null_mut();
  };
  match Speech::open(Path::new(&path)) {
    Ok(speech) => {
      let speech = Arc::new(Mutex::new(speech));
      Box::into_raw(Box::new(TalkifySpeech {
        session: Mutex::new((Ahead::new(speech.clone()), Vocabulary::parse(""))),
        speech,
      }))
    }
    Err(_) => std::ptr::null_mut(),
  }
}

/// # Safety
/// `speech` came from `talkify_open`, `samples` points at `count` floats, and
/// the two strings are NUL-terminated or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_dictate(
  speech: *mut TalkifySpeech,
  samples: *const f32,
  count: usize,
  vocabulary: *const c_char,
  send_phrases: *const c_char,
) -> *mut c_char {
  let Some(speech) = (unsafe { speech.as_ref() }) else {
    return std::ptr::null_mut();
  };
  let samples = if samples.is_null() {
    &[]
  } else {
    unsafe { std::slice::from_raw_parts(samples, count) }
  };
  // Without a words file the seed still turns "pr" into "PR".
  let vocabulary = unsafe { words(vocabulary) };
  let send_phrases = unsafe { text(send_phrases) }.unwrap_or_default();
  let heard = {
    let mut speech = held(&speech.speech);
    speech.stop_when(Arc::default());
    speech.transcribe(samples, &tidy::hints(&vocabulary.hints()))
  };
  json(written(heard, &vocabulary, &send_phrases))
}

/// # Safety
/// `speech` came from `talkify_open` and `vocabulary` is NUL-terminated or
/// null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_begin(speech: *mut TalkifySpeech, vocabulary: *const c_char) {
  let Some(speech) = (unsafe { speech.as_ref() }) else {
    return;
  };
  let vocabulary = unsafe { words(vocabulary) };
  let mut session = held(&speech.session);
  session.0.begin(tidy::hints(&vocabulary.hints()));
  session.1 = vocabulary;
}

/// # Safety
/// `speech` came from `talkify_open` and `samples` points at `count` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_feed(
  speech: *mut TalkifySpeech,
  samples: *const f32,
  count: usize,
) {
  let Some(speech) = (unsafe { speech.as_ref() }) else {
    return;
  };
  if !samples.is_null() {
    held(&speech.session)
      .0
      .feed(unsafe { std::slice::from_raw_parts(samples, count) });
  }
}

/// # Safety
/// `speech` came from `talkify_open` and `send_phrases` is NUL-terminated or
/// null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_finish(
  speech: *mut TalkifySpeech,
  send_phrases: *const c_char,
) -> *mut c_char {
  let Some(speech) = (unsafe { speech.as_ref() }) else {
    return std::ptr::null_mut();
  };
  let send_phrases = unsafe { text(send_phrases) }.unwrap_or_default();
  // Waited for with the session let go: the next recording may begin while
  // this one is still being heard.
  let (heard, vocabulary) = {
    let mut session = held(&speech.session);
    (session.0.finish(), session.1.clone())
  };
  json(written(
    heard.join().unwrap_or_default(),
    &vocabulary,
    &send_phrases,
  ))
}

/// What lands: the Tidy pass, then the Vocabulary.
fn written(heard: Heard, vocabulary: &Vocabulary, send: &str) -> String {
  let tidied = tidy::tidy(&heard.words, &TidyOptions::all(send));
  let text = vocabulary.apply(&tidied.text, true).text;
  serde_json::json!({ "text": text, "raw": heard.text, "send": tidied.send }).to_string()
}

fn json(written: String) -> *mut c_char {
  CString::new(written).map_or(std::ptr::null_mut(), CString::into_raw)
}

fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
  lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Without a words file the seed still turns "pr" into "PR".
unsafe fn words(vocabulary: *const c_char) -> Vocabulary {
  Vocabulary::parse(
    &unsafe { text(vocabulary) }.unwrap_or_else(|| talkify_core::vocab::SEED.into()),
  )
}

/// # Safety
/// `text` came from `talkify_dictate` or `talkify_finish`, once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_free(text: *mut c_char) {
  if !text.is_null() {
    drop(unsafe { CString::from_raw(text) });
  }
}

/// # Safety
/// `speech` came from `talkify_open`, once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_close(speech: *mut TalkifySpeech) {
  if !speech.is_null() {
    drop(unsafe { Box::from_raw(speech) });
  }
}

unsafe fn text(pointer: *const c_char) -> Option<String> {
  if pointer.is_null() {
    return None;
  }
  Some(
    unsafe { CStr::from_ptr(pointer) }
      .to_string_lossy()
      .into_owned(),
  )
}
