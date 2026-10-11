//! The Speech Model's tokens into words.
//!
//! Whisper emits pieces of text, not words: " re" + "view", or one Chinese
//! character split across two tokens mid-byte. English words start where a
//! piece opens with a space. Chinese has no spaces, so a run of it is cut
//! where the speaker paused, which is what lets "送出" said after a pause be
//! found as a word of its own. Where Chinese meets Latin letters or digits a
//! word always ends, so "請幫我review" lands as "請幫我 review" however the
//! model spaced it.
//!
//! The model only says when each piece ends, and only roughly. The pauses
//! are read from the recording itself: a quiet stretch between the ends of
//! two pieces is a pause between them, and the words on either side are
//! timed to its edges. Everything downstream that asks "was this said after
//! a pause" (spoken punctuation, the send phrase, live insertion) reads
//! those timings.

use crate::live::{Word, is_wide};

/// A word starts after silence at least this long. The same pause the Tidy
/// pass asks for beside spoken punctuation.
const PAUSE: f32 = 0.15;

/// One token and when the Speech Model heard it end, in seconds.
pub struct Piece<'a> {
  pub bytes: &'a [u8],
  pub end: f32,
}

/// `quiet` is the recording's quiet stretches (`level::quiet`).
pub fn words(pieces: &[Piece], quiet: &[(f32, f32)]) -> Vec<Word> {
  let mut words: Vec<Word> = Vec::new();
  let mut pending: Vec<u8> = Vec::new();
  let mut heard_until: f32 = 0.0;
  let mut start;
  let mut since = 0.0;
  let mut spaced = true;
  for piece in pieces {
    if pending.is_empty() {
      since = heard_until;
    }
    heard_until = heard_until.max(piece.end);
    pending.extend_from_slice(piece.bytes);
    // A character cut across tokens is whole once the rest arrives, and only
    // then is it timed: the model stamps the first half anywhere.
    let Ok(text) = std::str::from_utf8(&pending) else {
      continue;
    };
    start = since;
    // The widest quiet stretch that is over before this piece is, and was
    // not before the last one: the pause this piece was said after. The
    // model's ends run a little late, so the word before is timed to where
    // the quiet really began.
    let pause = quiet
      .iter()
      .filter(|&&(_, to)| to > since && to <= piece.end)
      .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)));
    if let Some(&(from, to)) = pause {
      start = to;
      if let Some(word) = words.last_mut() {
        word.end = word.end.min(from).max(word.start);
      }
    }
    let mut paused = words.last().is_some_and(|word| start - word.end >= PAUSE);
    for c in text.chars() {
      if c.is_whitespace() {
        spaced = true;
        continue;
      }
      match words.last_mut() {
        Some(word) if !spaced && !opens_a_word(&word.text, c, paused) => {
          word.text.push(c);
          // Punctuation makes no sound, and the model places it after the
          // pause that follows the word: the word keeps its own end.
          if c.is_alphanumeric() {
            word.end = heard_until;
          }
        }
        _ => words.push(Word::new(&c.to_string(), start, heard_until)),
      }
      spaced = false;
      paused = false;
    }
    pending.clear();
  }
  words
}

/// Punctuation never opens a word; it belongs to the one before it. A
/// letter does after a pause, or where Chinese meets anything else.
fn opens_a_word(before: &str, c: char, paused: bool) -> bool {
  if !c.is_alphanumeric() {
    return false;
  }
  match before.chars().rev().find(|c| c.is_alphanumeric()) {
    Some(last) => paused || is_wide(last) != is_wide(c),
    None => false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::live::join;

  /// Pieces 0.1 s long, back to back; "|" in the spec is a 0.4 s pause and
  /// "_" a space inside a piece.
  fn heard(spec: &[&str]) -> Vec<Word> {
    let texts: Vec<String> = spec.iter().map(|piece| piece.replace('_', " ")).collect();
    let mut at = 0.0;
    let mut pieces = Vec::new();
    let mut quiet = Vec::new();
    for text in &texts {
      if text == "|" {
        quiet.push((at, at + 0.4));
        at += 0.4;
        continue;
      }
      at += 0.1;
      pieces.push(Piece {
        bytes: text.as_bytes(),
        end: at,
      });
    }
    words(&pieces, &quiet)
  }

  fn texts(spec: &[&str]) -> Vec<String> {
    heard(spec).into_iter().map(|word| word.text).collect()
  }

  #[test]
  fn english_words_start_at_a_leading_space() {
    assert_eq!(
      texts(&["_Run", "_the", "_te", "sts", "."]),
      ["Run", "the", "tests."]
    );
  }

  #[test]
  fn chinese_runs_together_until_a_pause() {
    assert_eq!(texts(&["今天", "天氣", "很好"]), ["今天天氣很好"]);
    assert_eq!(texts(&["跑", "測試", "|", "送出"]), ["跑測試", "送出"]);
  }

  #[test]
  fn chinese_and_latin_part_ways_however_the_model_spaced_them() {
    let words = texts(&["請", "幫我", "review", "這個", "_PR", "。"]);
    assert_eq!(words, ["請幫我", "review", "這個", "PR。"]);
    assert_eq!(
      join(words.iter().map(String::as_str)),
      "請幫我 review 這個 PR。"
    );
    assert_eq!(
      texts(&["有", "3", "個", "bug", "，", "好"]),
      ["有", "3", "個", "bug，", "好"]
    );
  }

  #[test]
  fn words_are_timed_to_the_edges_of_the_pause_between_them() {
    let words = heard(&["_Run", "_tests", "|", "_send", "_it"]);
    let tenths = |seconds: f32| (seconds * 10.0).round() as i32;
    let timings: Vec<(i32, i32)> = words
      .iter()
      .map(|word| (tenths(word.start), tenths(word.end)))
      .collect();
    assert_eq!(timings, [(0, 1), (1, 2), (6, 7), (7, 8)]);
  }

  #[test]
  fn a_pause_parts_words_the_model_ran_together() {
    // The model heard "much" end late, inside the pause, placed the period
    // after it and gave "Send" no space.
    let pieces = [
      Piece {
        bytes: " much".as_bytes(),
        end: 1.24,
      },
      Piece {
        bytes: ".".as_bytes(),
        end: 2.06,
      },
      Piece {
        bytes: "S".as_bytes(),
        end: 2.14,
      },
      Piece {
        bytes: "end".as_bytes(),
        end: 2.38,
      },
    ];
    let words = words(&pieces, &[(1.18, 1.99)]);
    assert_eq!(words.len(), 2);
    assert_eq!((words[0].text.as_str(), words[0].end), ("much.", 1.18));
    assert_eq!((words[1].text.as_str(), words[1].start), ("Send", 2.06));
    // "much" itself started before the quiet did.
    assert_eq!(words[0].start, 0.0);
  }

  #[test]
  fn a_split_character_is_timed_by_its_second_half() {
    // "逗" in two tokens, the first stamped inside the pause before it.
    let bytes = "逗號".as_bytes();
    let pieces = [
      Piece {
        bytes: "很好".as_bytes(),
        end: 2.24,
      },
      Piece {
        bytes: &bytes[..2],
        end: 2.28,
      },
      Piece {
        bytes: &bytes[2..3],
        end: 3.72,
      },
      Piece {
        bytes: &bytes[3..],
        end: 4.08,
      },
    ];
    let words = words(&pieces, &[(1.82, 3.58)]);
    let texts: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
    assert_eq!(texts, ["很好", "逗號"]);
    assert_eq!((words[0].end, words[1].start), (1.82, 3.58));
  }

  #[test]
  fn a_character_split_across_tokens_is_put_back_together() {
    let bytes = "好的".as_bytes();
    let pieces = [
      Piece {
        bytes: &bytes[..2],
        end: 0.1,
      },
      Piece {
        bytes: &bytes[2..],
        end: 0.2,
      },
    ];
    let words = words(&pieces, &[]);
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].text, "好的");
    assert_eq!((words[0].start, words[0].end), (0.0, 0.2));
  }
}
