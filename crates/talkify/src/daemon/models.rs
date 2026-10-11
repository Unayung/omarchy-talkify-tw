//! The Speech Model on disk: which files make it up, whether they are all
//! there, and fetching the missing ones.
//!
//! Hugging Face downloads of a gigabyte are known to stall partway,
//! so curl resumes (`-C -`), gives up on a stalled connection after 30
//! seconds, and the whole thing retries.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, bail};

use crate::paths;

pub struct ModelFile {
  pub name: &'static str,
  pub size: u64,
}

pub struct Model {
  pub id: &'static str,
  pub source: &'static str,
  pub files: &'static [ModelFile],
}

pub const BREEZE_ASR_25: Model = Model {
  id: "breeze-asr-25-q5_k",
  source: "https://huggingface.co/alan314159/Breeze-ASR-25-whispercpp/resolve/main",
  files: &[ModelFile {
    name: "ggml-model-q5_k.bin",
    size: 1_080_732_108,
  }],
};

pub fn find(id: &str) -> Option<&'static Model> {
  [&BREEZE_ASR_25].into_iter().find(|model| model.id == id)
}

impl Model {
  pub fn dir(&self) -> PathBuf {
    paths::models_dir().join(self.id)
  }

  /// The weights: a whisper.cpp model is one file.
  pub fn weights(&self) -> PathBuf {
    self.dir().join(self.files[0].name)
  }

  pub fn total_size(&self) -> u64 {
    self.files.iter().map(|file| file.size).sum()
  }

  fn on_disk(&self) -> u64 {
    self
      .files
      .iter()
      .map(|file| std::fs::metadata(self.dir().join(file.name)).map_or(0, |m| m.len()))
      .sum()
  }

  pub fn is_complete(&self) -> bool {
    self
      .files
      .iter()
      .all(|file| std::fs::metadata(self.dir().join(file.name)).is_ok_and(|m| m.len() == file.size))
  }

  /// Fetches every missing or partial file, reporting progress 0–1 about
  /// once a second.
  pub fn download(&self, progress: impl Fn(f32) + Sync) -> anyhow::Result<()> {
    std::fs::create_dir_all(self.dir()).context("create the model folder")?;
    let total = self.total_size() as f32;
    for file in self.files {
      let path = self.dir().join(file.name);
      for attempt in 1..=5 {
        match std::fs::metadata(&path).map(|m| m.len()) {
          Ok(size) if size == file.size => break,
          // Longer than the real file: corrupt, and resuming cannot fix it.
          Ok(size) if size > file.size => std::fs::remove_file(&path)?,
          _ => {}
        }
        let done = AtomicBool::new(false);
        let curl = std::thread::scope(|scope| {
          scope.spawn(|| {
            while !done.load(Ordering::Relaxed) {
              progress(self.on_disk() as f32 / total);
              std::thread::sleep(Duration::from_secs(1));
            }
          });
          let status = Command::new("curl")
            .args(["--location", "--fail", "--silent", "--show-error"])
            .args([
              "--continue-at",
              "-",
              "--speed-limit",
              "20000",
              "--speed-time",
              "30",
            ])
            .arg("--output")
            .arg(&path)
            .arg(format!("{}/{}", self.source, file.name))
            .status();
          done.store(true, Ordering::Relaxed);
          status
        });
        match curl {
          Ok(status) if status.success() => {}
          Ok(status) => tracing::warn!("{} attempt {attempt}: curl {status}", file.name),
          Err(error) => bail!("could not run curl: {error}"),
        }
      }
      let size = std::fs::metadata(&path).map_or(0, |m| m.len());
      if size != file.size {
        bail!("{} is {size} bytes, expected {}", file.name, file.size);
      }
    }
    progress(1.0);
    Ok(())
  }
}
