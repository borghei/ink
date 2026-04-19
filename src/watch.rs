use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

/// Cross-platform file watcher backed by `notify`.
///
/// We watch the *parent directory* (non-recursive) instead of the file itself so that
/// editors which save by replacing the inode (vim, IntelliJ, etc.) keep firing events.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<PathBuf>,
}

impl FileWatcher {
    pub fn new(target: &Path) -> Result<Self> {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                ) {
                    for p in event.paths {
                        let _ = tx.send(p);
                    }
                }
            }
        })?;
        let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Drain pending events. Returns true if the watched file was touched.
    pub fn check(&self, target: &Path) -> bool {
        let mut changed = false;
        while let Ok(p) = self.rx.try_recv() {
            if same_file(&p, target) {
                changed = true;
            }
        }
        changed
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::time::{Duration, Instant};

    #[test]
    fn detects_writes_to_watched_file() {
        let dir = std::env::temp_dir().join(format!("ink-watch-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("note.md");
        fs::write(&target, "initial\n").unwrap();

        let watcher = FileWatcher::new(&target).expect("watcher");
        // Drain any startup events.
        std::thread::sleep(Duration::from_millis(100));
        let _ = watcher.check(&target);

        // Modify the file.
        let mut f = fs::OpenOptions::new().append(true).open(&target).unwrap();
        writeln!(f, "more").unwrap();
        f.sync_all().unwrap();
        drop(f);

        // Poll for up to 2 seconds.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut detected = false;
        while Instant::now() < deadline {
            if watcher.check(&target) {
                detected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = fs::remove_dir_all(&dir);
        assert!(detected, "FileWatcher did not report change to watched file");
    }

    #[test]
    fn ignores_other_files_in_same_dir() {
        let dir = std::env::temp_dir().join(format!("ink-watch-test2-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("watched.md");
        let other = dir.join("other.md");
        fs::write(&target, "x\n").unwrap();
        fs::write(&other, "y\n").unwrap();

        let watcher = FileWatcher::new(&target).expect("watcher");
        std::thread::sleep(Duration::from_millis(100));
        let _ = watcher.check(&target);

        // Modify the OTHER file.
        let mut f = fs::OpenOptions::new().append(true).open(&other).unwrap();
        writeln!(f, "z").unwrap();
        f.sync_all().unwrap();
        drop(f);

        std::thread::sleep(Duration::from_millis(300));
        let detected = watcher.check(&target);
        let _ = fs::remove_dir_all(&dir);
        assert!(!detected, "FileWatcher reported a change for a different file");
    }
}
