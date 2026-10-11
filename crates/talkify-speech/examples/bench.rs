//! How long the Speech Model takes over a recording, and how long the wait
//! after the release is when it hears ahead:
//! `cargo run --release -p talkify-speech --features metal --example bench WEIGHTS FILE.wav...`
//! Files are 16 kHz mono 16-bit WAV with a plain 44-byte header.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use talkify_core::level;
use talkify_speech::Speech;
use talkify_speech::ahead::Ahead;

fn main() {
  let mut args = std::env::args().skip(1);
  let weights = args.next().expect("the model's weights, then recordings");
  let speech = Speech::open(weights.as_ref()).expect("load the model");
  let speech = Arc::new(Mutex::new(speech));
  let mut ahead = Ahead::new(speech.clone());
  for file in args {
    let bytes = std::fs::read(&file).expect("read the recording");
    let mut samples: Vec<f32> = bytes[44..]
      .chunks_exact(2)
      .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
      .collect();
    let quiet = (level::trailing_silence(&samples) * 16_000.0) as usize;
    samples.truncate(samples.len() - quiet);
    let mut millis = Vec::new();
    let mut text = String::new();
    for _ in 0..3 {
      let started = Instant::now();
      text = speech.lock().unwrap().transcribe(&samples, "").text;
      millis.push(started.elapsed().as_millis());
    }
    println!(
      "{file} {:.1}s {millis:?} {text}",
      samples.len() as f32 / 16000.0
    );
    // The trigger held for a while after the last word, fed as a microphone
    // would feed it.
    for linger in [0, 300, 600] {
      let mut held = samples.clone();
      held.resize(samples.len() + linger * 16, 0.0);
      ahead.begin(String::new());
      for chunk in held.chunks(1_600) {
        ahead.feed(chunk);
        std::thread::sleep(Duration::from_millis(100));
      }
      let released = Instant::now();
      let heard = ahead.finish().join().unwrap().text;
      let same = if heard == text { "same" } else { &heard };
      println!(
        "  held {linger} ms longer: {:?} ms, {same}",
        released.elapsed().as_millis()
      );
    }
  }
}
