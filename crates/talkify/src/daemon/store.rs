//! The daemon's files: the config, the vocabulary, Insights' aggregates,
//! and saved dictations. Every write goes to a temporary file first and is
//! renamed into place, so a crash never leaves half a file.

use std::path::Path;
use std::time::SystemTime;

use anyhow::Context;
use talkify_core::config::Config;
use talkify_core::insights::UsageDocument;
use talkify_core::vocab::Vocabulary;

use crate::paths;

fn write_atomically(path: &Path, contents: &str) -> anyhow::Result<()> {
  std::fs::create_dir_all(path.parent().context("path has a parent")?)?;
  let partial = path.with_extension("partial");
  std::fs::write(&partial, contents)?;
  std::fs::rename(&partial, path)?;
  Ok(())
}

pub fn load_config() -> Config {
  let path = paths::config_file();
  match std::fs::read_to_string(&path) {
    Ok(text) => Config::from_toml(&text).unwrap_or_else(|error| {
      tracing::error!("{}: {error}; using defaults", path.display());
      Config::default()
    }),
    Err(_) => {
      let config = Config::default();
      if let Err(error) = save_config(&config) {
        tracing::warn!("could not write {}: {error:#}", path.display());
      }
      config
    }
  }
}

pub fn save_config(config: &Config) -> anyhow::Result<()> {
  write_atomically(&paths::config_file(), &config.to_toml())
}

/// The English a Taiwanese engineer says inside Mandarin every day. The
/// Speech Model writes English in lower case, so these are what turn "pr"
/// into "PR"; they are also its hints. Words whose sound sits next to a
/// common one ("Rust" and "rest") are left for the user to add.
const SEED_VOCABULARY: &str = "\
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

pub struct VocabFile {
  pub vocabulary: Vocabulary,
  pub hits: Vec<u32>,
  modified: Option<SystemTime>,
}

impl VocabFile {
  pub fn load() -> Self {
    let path = paths::words_file();
    if !path.exists()
      && let Err(error) = write_atomically(&path, SEED_VOCABULARY)
    {
      tracing::warn!("could not seed {}: {error:#}", path.display());
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let vocabulary = Vocabulary::parse(&text);
    Self {
      hits: vec![0; vocabulary.entries.len()],
      vocabulary,
      modified: modified(&path),
    }
  }

  /// Reloads when the file changed on disk (edited by hand), keeping hit
  /// counts for lines that survived. True when it reloaded.
  pub fn refresh(&mut self) -> bool {
    if modified(&paths::words_file()) == self.modified {
      return false;
    }
    let old: Vec<(String, u32)> = self
      .vocabulary
      .entries
      .iter()
      .map(|entry| entry.line.clone())
      .zip(self.hits.iter().copied())
      .collect();
    *self = Self::load();
    for (entry, hits) in self.vocabulary.entries.iter().zip(self.hits.iter_mut()) {
      if let Some((_, count)) = old.iter().find(|(line, _)| *line == entry.line) {
        *hits = *count;
      }
    }
    true
  }

  pub fn record_hits(&mut self, indices: &[usize]) {
    for index in indices {
      if let Some(count) = self.hits.get_mut(*index) {
        *count += 1;
      }
    }
  }

  pub fn add(&mut self, line: &str) -> anyhow::Result<()> {
    let line = line.trim();
    anyhow::ensure!(!line.is_empty(), "empty line");
    let path = paths::words_file();
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.lines().any(|existing| existing.trim() == line) {
      return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
      text.push('\n');
    }
    text.push_str(line);
    text.push('\n');
    write_atomically(&path, &text)?;
    self.refresh();
    Ok(())
  }

  pub fn remove(&mut self, line: &str) -> anyhow::Result<()> {
    let path = paths::words_file();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let kept: Vec<&str> = text
      .lines()
      .filter(|existing| existing.trim() != line.trim())
      .collect();
    write_atomically(&path, &(kept.join("\n") + "\n"))?;
    self.refresh();
    Ok(())
  }
}

fn modified(path: &Path) -> Option<SystemTime> {
  std::fs::metadata(path)
    .and_then(|meta| meta.modified())
    .ok()
}

pub fn load_usage() -> UsageDocument {
  std::fs::read_to_string(paths::usage_file())
    .ok()
    .and_then(|text| serde_json::from_str(&text).ok())
    .unwrap_or_default()
}

pub fn save_usage(usage: &UsageDocument) -> anyhow::Result<()> {
  write_atomically(&paths::usage_file(), &serde_json::to_string(usage)?)
}

/// Appends a dictation to today's history file, when saving is on.
pub fn save_dictation(app: &str, text: &str) {
  let now = chrono::Local::now();
  let path = paths::history_dir().join(format!("{}.txt", now.format("%Y-%m-%d")));
  let line = format!(
    "{}\t{}\t{}\n",
    now.format("%H:%M:%S"),
    if app.is_empty() { "-" } else { app },
    text.replace('\n', " ")
  );
  let result = std::fs::create_dir_all(paths::history_dir()).and_then(|()| {
    use std::io::Write;
    std::fs::OpenOptions::new()
      .create(true)
      .append(true)
      .open(&path)?
      .write_all(line.as_bytes())
  });
  if let Err(error) = result {
    tracing::warn!("could not save to {}: {error}", path.display());
  }
}
