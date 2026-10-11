//! The vocabulary: one plain-text file of words the Speech Model should get
//! right, corrections, and snippets.
//!
//! ```text
//! # comments
//! Hyprland                 a word to prefer, matched by sound and spelling
//! omachi -> Omarchy        a correction: heard, then written
//! my email => me@x.dev     a snippet: say the trigger, get the expansion
//! ```
//!
//! It runs after decoding, over windows of one to three words. Corrections
//! and snippets match exactly (whole words, any case, longest first).
//! Preferred words also match by sound (Double Metaphone) with a bounded
//! edit distance, and only from the same first letter, so "talker" never
//! becomes "Walker".
//!
//! Chinese has no spaces to make words of, so a correction or snippet heard
//! in Chinese ("歐瑪奇 -> Omarchy") matches wherever it appears in a run.

use std::ops::Range;

use rphonetic::DoubleMetaphone;

use crate::live::{gap, is_wide};

/// Longest window, in words, an entry can match.
const MAX_WINDOW: usize = 3;
/// How long the hints may grow. Whisper's prompt holds 224 tokens.
const HINT_CHARS: usize = 400;

/// The English a Taiwanese engineer says inside Mandarin every day. The
/// Speech Model writes English in lower case, so these are what turn "pr"
/// into "PR"; they are also its hints. Words whose sound sits next to a
/// common one ("Rust" and "rest") are left for the user to add.
pub const SEED: &str = "\
# Talkify 詞庫，一行一筆：
#   Hyprland                 偏好的寫法（大小寫照這裡，也會提示給語音模型）
#   歐瑪奇 -> Omarchy         修正：聽到左邊，寫成右邊
#   我的信箱 => me@x.dev      片語：說左邊，貼上右邊
# 選取改好的字再執行 `talkify teach`，會自動把修正加到這裡。
Omarchy
Hyprland
Quickshell
Walker
Claude Code
GitHub
Docker
Kubernetes
PostgreSQL
TypeScript
JavaScript
Python
PR
API
CI
JSON
YAML
SSH
AWS
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
  Word,
  Replacement,
  Snippet,
}

impl EntryKind {
  pub fn as_str(self) -> &'static str {
    match self {
      EntryKind::Word => "word",
      EntryKind::Replacement => "replacement",
      EntryKind::Snippet => "snippet",
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
  pub line: String,
  pub kind: EntryKind,
  /// What is matched, as normalized words.
  pub from: Vec<String>,
  /// What lands.
  pub to: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vocabulary {
  pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
  pub text: String,
  /// Indices into `entries` of every entry that changed something.
  pub hits: Vec<usize>,
  /// Byte ranges in the input that were rewritten.
  pub spans: Vec<Range<usize>>,
  /// What each span became, in the same order.
  pub replacements: Vec<String>,
}

pub fn normalized(word: &str) -> String {
  word
    .chars()
    .filter(|c| c.is_alphanumeric())
    .flat_map(char::to_lowercase)
    .collect()
}

fn parse_line(line: &str) -> Option<Entry> {
  let trimmed = line.trim();
  if trimmed.is_empty() || trimmed.starts_with('#') {
    return None;
  }
  let (kind, from, to) = if let Some((from, to)) = trimmed.split_once("=>") {
    (EntryKind::Snippet, from, to.trim().replace("\\n", "\n"))
  } else if let Some((from, to)) = trimmed.split_once("->").or_else(|| trimmed.split_once('→')) {
    (EntryKind::Replacement, from, to.trim().to_string())
  } else {
    (EntryKind::Word, trimmed, trimmed.to_string())
  };
  let from: Vec<String> = from
    .split_whitespace()
    .map(normalized)
    .filter(|word| !word.is_empty())
    .collect();
  if from.is_empty() || from.len() > MAX_WINDOW || to.is_empty() {
    return None;
  }
  Some(Entry {
    line: trimmed.to_string(),
    kind,
    from,
    to,
  })
}

/// A word of the text: its byte range, and the punctuation around its core.
#[derive(Debug)]
struct Token {
  range: Range<usize>,
  core: Range<usize>,
}

fn tokens(text: &str) -> Vec<Token> {
  let mut tokens = Vec::new();
  let mut start = None;
  for (index, ch) in text
    .char_indices()
    .chain(std::iter::once((text.len(), ' ')))
  {
    match (start, ch.is_whitespace()) {
      (None, false) => start = Some(index),
      (Some(begin), true) => {
        let word = &text[begin..index];
        let lead = word.len()
          - word
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .len();
        let trail = word.len() - word.trim_end_matches(|c: char| !c.is_alphanumeric()).len();
        let core = if lead + trail >= word.len() {
          begin..begin
        } else {
          begin + lead..index - trail
        };
        tokens.push(Token {
          range: begin..index,
          core,
        });
        start = None;
      }
      _ => {}
    }
  }
  tokens
}

pub fn levenshtein(a: &str, b: &str) -> usize {
  let a: Vec<char> = a.chars().collect();
  let b: Vec<char> = b.chars().collect();
  let mut row: Vec<usize> = (0..=b.len()).collect();
  for (i, ca) in a.iter().enumerate() {
    let mut previous = row[0];
    row[0] = i + 1;
    for (j, cb) in b.iter().enumerate() {
      let current = row[j + 1];
      row[j + 1] = (current + 1)
        .min(row[j] + 1)
        .min(previous + usize::from(ca != cb));
      previous = current;
    }
  }
  row[b.len()]
}

fn ratio(a: &str, b: &str) -> f32 {
  let longest = a.chars().count().max(b.chars().count()).max(1);
  levenshtein(a, b) as f32 / longest as f32
}

struct Sounds(DoubleMetaphone);

impl Sounds {
  fn new() -> Self {
    Self(DoubleMetaphone::new(Some(8)))
  }

  fn keys(&self, word: &str) -> (String, String) {
    let result = self.0.double_metaphone(word);
    (result.primary(), result.alternate())
  }

  /// Whether `heard` sounds and looks enough like `wanted` (both normalized
  /// and joined) to be that word misheard.
  fn resembles(&self, heard: &str, wanted: &str) -> bool {
    // Double Metaphone is for Latin letters, and panics on Chinese.
    if !heard.is_ascii() || !wanted.is_ascii() {
      return false;
    }
    if heard.chars().count() < 4 || heard.chars().next() != wanted.chars().next() {
      return false;
    }
    let (heard_primary, heard_alternate) = self.keys(heard);
    let (wanted_primary, wanted_alternate) = self.keys(wanted);
    let same_sound = [&heard_primary, &heard_alternate]
      .iter()
      .any(|key| !key.is_empty() && (**key == wanted_primary || **key == wanted_alternate));
    let close_sound = levenshtein(&heard_primary, &wanted_primary) <= 1;
    let spelling = ratio(heard, wanted);
    (same_sound && spelling <= 0.45) || (close_sound && spelling <= 0.3)
  }
}

impl Vocabulary {
  pub fn parse(text: &str) -> Self {
    Self {
      entries: text.lines().filter_map(parse_line).collect(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.entries.is_empty()
  }

  /// What the Speech Model is told to expect: the words to prefer and what
  /// corrections write, each once. Whisper reads only the end of a long
  /// prompt, so the list stops at `HINT_CHARS`.
  pub fn hints(&self) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for entry in &self.entries {
      if entry.kind != EntryKind::Snippet && !seen.contains(&entry.to.as_str()) {
        seen.push(&entry.to);
      }
    }
    let mut hints = String::new();
    for word in seen {
      if hints.chars().count() + word.chars().count() > HINT_CHARS {
        break;
      }
      if !hints.is_empty() {
        hints.push_str(", ");
      }
      hints.push_str(word);
    }
    hints
  }

  pub fn apply(&self, text: &str, fuzzy: bool) -> Applied {
    let tokens = tokens(text);
    let sounds = Sounds::new();
    let mut out = String::with_capacity(text.len());
    let mut hits = Vec::new();
    let mut spans = Vec::new();
    let mut replacements = Vec::new();
    let mut copied = 0;
    let mut index = 0;
    let wide: Vec<(usize, String)> = self
      .entries
      .iter()
      .enumerate()
      .filter(|(_, entry)| entry.kind != EntryKind::Word)
      .map(|(at, entry)| (at, entry.from.concat()))
      .filter(|(_, from)| from.chars().any(is_wide))
      .collect();
    while index < tokens.len() {
      let found = (1..=MAX_WINDOW.min(tokens.len() - index))
        .rev()
        .find_map(|width| {
          self
            .matching(&tokens[index..index + width], text, &sounds, fuzzy)
            .map(|entry| (width, entry))
        });
      let Some((width, entry_index)) = found else {
        let core = tokens[index].core.clone();
        let mut at = core.start;
        while at < core.end {
          let rest = &text[at..core.end];
          let hit = wide
            .iter()
            .filter(|(_, from)| rest.starts_with(from.as_str()))
            .max_by_key(|(_, from)| from.len());
          let Some((entry_index, from)) = hit else {
            at += rest.chars().next().map_or(1, char::len_utf8);
            continue;
          };
          let end = at + from.len();
          let replacement = &self.entries[*entry_index].to;
          out.push_str(&text[copied..at]);
          // English written into a run of Chinese is set off by spaces, as
          // the Speech Model's own English is.
          out.push_str(gap(&out, replacement));
          out.push_str(replacement);
          out.push_str(gap(replacement, &text[end..core.end]));
          copied = end;
          hits.push(*entry_index);
          spans.push(at..end);
          replacements.push(replacement.clone());
          at = end;
        }
        index += 1;
        continue;
      };
      let window = &tokens[index..index + width];
      let first = &window[0];
      let last = &window[width - 1];
      let original = &text[first.core.start..last.core.end];
      let entry = &self.entries[entry_index];
      let replacement = match entry.kind {
        EntryKind::Snippet => entry.to.clone(),
        _ => matched_case(original, &entry.to),
      };
      if replacement != original {
        out.push_str(&text[copied..first.core.start]);
        out.push_str(&replacement);
        copied = last.core.end;
        hits.push(entry_index);
        spans.push(first.core.start..last.core.end);
        replacements.push(replacement.clone());
      }
      index += width;
    }
    out.push_str(&text[copied..]);
    hits.sort_unstable();
    hits.dedup();
    Applied {
      text: out,
      hits,
      spans,
      replacements,
    }
  }

  fn matching(&self, window: &[Token], text: &str, sounds: &Sounds, fuzzy: bool) -> Option<usize> {
    // Punctuation inside a window ("five. Omarchy") means two phrases.
    if window[..window.len() - 1]
      .iter()
      .any(|token| token.core.end != token.range.end)
    {
      return None;
    }
    let words: Vec<String> = window
      .iter()
      .map(|token| normalized(&text[token.core.clone()]))
      .collect();
    if words.iter().any(String::is_empty) {
      return None;
    }
    if let Some(exact) = self.entries.iter().position(|entry| entry.from == words) {
      return Some(exact);
    }
    let joined = words.concat();
    self.entries.iter().position(|entry| {
      entry.kind == EntryKind::Word && {
        let wanted = entry.from.concat();
        joined == wanted || (fuzzy && sounds.resembles(&joined, &wanted))
      }
    })
  }

  /// Teach: `selection` is the right spelling of something in `last`. Finds
  /// the stretch of `last` that was misheard and returns the line to add:
  /// `heard -> written`, or the selection alone as a word to prefer when
  /// nothing in `last` resembles it. None when `last` already has it right.
  pub fn teach(last: &str, selection: &str) -> Option<String> {
    let written = selection.split_whitespace().collect::<Vec<_>>().join(" ");
    let wanted: Vec<String> = written.split_whitespace().map(normalized).collect();
    if wanted.is_empty() || wanted.len() > MAX_WINDOW {
      return None;
    }
    let wanted_joined = wanted.concat();
    let tokens = tokens(last);
    let words: Vec<String> = tokens
      .iter()
      .map(|token| normalized(&last[token.core.clone()]))
      .collect();
    if words
      .windows(wanted.len())
      .any(|window| window == wanted.as_slice())
    {
      return None;
    }
    let sounds = Sounds::new();
    let mut best: Option<(f32, Range<usize>)> = None;
    for width in 1..=MAX_WINDOW.min(words.len()) {
      for start in 0..=words.len() - width {
        let joined = words[start..start + width].concat();
        if joined.is_empty() {
          continue;
        }
        let mut score = ratio(&joined, &wanted_joined);
        if sounds.resembles(&joined, &wanted_joined) {
          score -= 0.2;
        }
        if best
          .as_ref()
          .is_none_or(|(best_score, _)| score < *best_score)
        {
          best = Some((score, start..start + width));
        }
      }
    }
    match best {
      Some((score, range)) if score <= 0.5 => {
        let heard = words[range].join(" ");
        Some(format!("{heard} -> {written}"))
      }
      _ => Some(written),
    }
  }
}

/// The entry's spelling, capitalised when the heard words started a
/// sentence with a capital the entry does not have.
fn matched_case(original: &str, to: &str) -> String {
  let starts_upper = original.chars().next().is_some_and(char::is_uppercase);
  let to_starts_lower = to.chars().next().is_some_and(char::is_lowercase);
  if starts_upper && to_starts_lower {
    let mut chars = to.chars();
    return chars
      .next()
      .map(|c| c.to_uppercase().collect::<String>())
      .unwrap_or_default()
      + chars.as_str();
  }
  to.to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn vocab(lines: &str) -> Vocabulary {
    Vocabulary::parse(lines)
  }

  #[test]
  fn the_three_line_forms_and_comments_parse() {
    let v = vocab(
      "# mine\nHyprland\nomachi -> Omarchy\nclaude code → Claude Code\nmy email => me@x.dev\n\n",
    );
    let kinds: Vec<EntryKind> = v.entries.iter().map(|entry| entry.kind).collect();
    assert_eq!(
      kinds,
      vec![
        EntryKind::Word,
        EntryKind::Replacement,
        EntryKind::Replacement,
        EntryKind::Snippet
      ]
    );
    assert_eq!(v.entries[2].from, vec!["claude", "code"]);
    assert_eq!(v.entries[3].to, "me@x.dev");
  }

  #[test]
  fn replacements_match_whole_words_in_any_case_and_keep_punctuation() {
    let v = vocab("omachi -> Omarchy");
    assert_eq!(
      v.apply("I love Omachi, really.", false).text,
      "I love Omarchy, really."
    );
    assert_eq!(v.apply("omachines stay", false).text, "omachines stay");
  }

  #[test]
  fn chinese_corrections_match_inside_a_run() {
    let v = vocab("歐瑪奇 -> Omarchy\n我的信箱 => me@x.dev\n庫伯 -> K8s");
    let applied = v.apply("我在用歐瑪奇的時候，庫伯掛了。", false);
    assert_eq!(applied.text, "我在用 Omarchy 的時候，K8s 掛了。");
    assert_eq!(applied.hits, vec![0, 2]);
    assert_eq!(v.apply("寄到我的信箱", false).text, "寄到 me@x.dev");
    assert_eq!(v.apply("歐瑪奇 很好", false).text, "Omarchy 很好");
  }

  #[test]
  fn sound_matching_leaves_chinese_alone() {
    let v = vocab("Kubernetes\n簡報");
    assert_eq!(
      v.apply("我剛剛發現 kubernetes 的 pod 一直 crash", true)
        .text,
      "我剛剛發現 Kubernetes 的 pod 一直 crash"
    );
    assert_eq!(
      Vocabulary::teach("記得先把報告準備好", "簡報").as_deref(),
      Some("簡報")
    );
  }

  #[test]
  fn hints_list_what_should_be_written_once_and_leave_snippets_out() {
    let v = vocab("Hyprland\nomachi -> Omarchy\nOmarchy\nmy email => me@x.dev\n歐瑪奇 -> Omarchy");
    assert_eq!(v.hints(), "Hyprland, Omarchy");
  }

  #[test]
  fn the_longest_match_wins() {
    let v = vocab("code -> CODE\nclaude code -> Claude Code");
    assert_eq!(
      v.apply("open claude code now", false).text,
      "open Claude Code now"
    );
  }

  #[test]
  fn snippets_expand_verbatim() {
    let v = vocab("sign off => Best,\\nTornike");
    let applied = v.apply("Thanks again. Sign off", false);
    assert_eq!(applied.text, "Thanks again. Best,\nTornike");
    assert_eq!(applied.hits, vec![0]);
  }

  #[test]
  fn preferred_words_fix_their_spelling_and_match_by_sound() {
    let v = vocab("Hyprland\nOmarchy\nQuickshell");
    assert_eq!(v.apply("i use hyprland", true).text, "i use Hyprland");
    assert_eq!(
      v.apply("running hyper land today", true).text,
      "running Hyprland today"
    );
    assert_eq!(v.apply("the omachi menu", true).text, "the Omarchy menu");
    assert_eq!(v.apply("quick shell bar", true).text, "Quickshell bar");
    // Without fuzzy matching only the spelling is fixed.
    assert_eq!(v.apply("the omachi menu", false).text, "the omachi menu");
  }

  #[test]
  fn sound_alikes_from_another_first_letter_or_too_far_are_left_alone() {
    let v = vocab("Walker\nOmarchy");
    assert_eq!(v.apply("a good talker", true).text, "a good talker");
    assert_eq!(v.apply("an anarchy", true).text, "an anarchy");
    assert_eq!(v.apply("my walk", true).text, "my walk");
  }

  #[test]
  fn a_sentence_start_keeps_its_capital() {
    let v = vocab("gh -> github cli");
    assert_eq!(
      v.apply("Gh is installed", false).text,
      "Github cli is installed"
    );
  }

  #[test]
  fn windows_never_span_a_sentence_break() {
    let v = vocab("five omarchy -> X");
    assert_eq!(
      v.apply("at five. Omarchy rocks", false).text,
      "at five. Omarchy rocks"
    );
  }

  #[test]
  fn hits_and_spans_report_what_changed() {
    let v = vocab("Hyprland\nomachi -> Omarchy");
    let applied = v.apply("omachi on Hyprland and hyprland", true);
    assert_eq!(applied.text, "Omarchy on Hyprland and Hyprland");
    assert_eq!(applied.hits, vec![0, 1]);
    assert_eq!(applied.spans, vec![0..6, 23..31]);
    assert_eq!(applied.replacements, vec!["Omarchy", "Hyprland"]);
  }

  #[test]
  fn teach_finds_the_misheard_stretch() {
    assert_eq!(
      Vocabulary::teach("I switched to omachi last week.", "Omarchy").as_deref(),
      Some("omachi -> Omarchy")
    );
    assert_eq!(
      Vocabulary::teach("open hyper land settings", "Hyprland").as_deref(),
      Some("hyper land -> Hyprland")
    );
  }

  #[test]
  fn teach_prefers_a_new_word_when_nothing_resembles_it_and_skips_what_is_right() {
    assert_eq!(
      Vocabulary::teach("the meeting is at noon", "Zellij").as_deref(),
      Some("Zellij")
    );
    assert_eq!(Vocabulary::teach("I use Omarchy daily", "Omarchy"), None);
  }
}
