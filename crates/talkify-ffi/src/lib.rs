//! The Speech Model and the text rules behind a C interface, so the Mac app
//! hears and writes exactly what the Linux daemon does. See
//! `include/talkify.h` for the contract.

use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::sync::Mutex;

use talkify_core::tidy::{self, TidyOptions};
use talkify_core::vocab::Vocabulary;
use talkify_speech::Speech;

pub struct TalkifySpeech(Mutex<Speech>);

/// # Safety
/// `weights_path` is a NUL-terminated string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn talkify_open(weights_path: *const c_char) -> *mut TalkifySpeech {
  let Some(path) = (unsafe { text(weights_path) }) else {
    return std::ptr::null_mut();
  };
  match Speech::open(Path::new(&path)) {
    Ok(speech) => Box::into_raw(Box::new(TalkifySpeech(Mutex::new(speech)))),
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
  let vocabulary = Vocabulary::parse(&unsafe { text(vocabulary) }.unwrap_or_default());
  let send_phrases = unsafe { text(send_phrases) }.unwrap_or_default();
  let dictated = dictate(
    &mut speech
      .0
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner()),
    samples,
    &vocabulary,
    &send_phrases,
  );
  CString::new(dictated).map_or(std::ptr::null_mut(), CString::into_raw)
}

fn dictate(speech: &mut Speech, samples: &[f32], vocabulary: &Vocabulary, send: &str) -> String {
  let heard = speech.transcribe(samples, &tidy::hints(&vocabulary.hints()));
  let tidied = tidy::tidy(&heard.words, &TidyOptions::all(send));
  let text = vocabulary.apply(&tidied.text, true).text;
  serde_json::json!({ "text": text, "raw": heard.text, "send": tidied.send }).to_string()
}

/// # Safety
/// `text` came from `talkify_dictate`, once.
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
