//! How long the Speech Model takes over a recording:
//! `cargo run --release -p talkify-speech --features metal --example bench WEIGHTS FILE.wav...`
//! Files are 16 kHz mono 16-bit WAV with a plain 44-byte header.

use std::time::Instant;

fn main() {
  let mut args = std::env::args().skip(1);
  let weights = args.next().expect("the model's weights, then recordings");
  let mut speech = talkify_speech::Speech::open(weights.as_ref()).expect("load the model");
  for file in args {
    let bytes = std::fs::read(&file).expect("read the recording");
    let samples: Vec<f32> = bytes[44..]
      .chunks_exact(2)
      .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
      .collect();
    let mut millis = Vec::new();
    let mut text = String::new();
    for _ in 0..3 {
      let started = Instant::now();
      text = speech.transcribe(&samples, "").text;
      millis.push(started.elapsed().as_millis());
    }
    println!(
      "{file} {:.1}s {millis:?} {text}",
      samples.len() as f32 / 16000.0
    );
  }
}
