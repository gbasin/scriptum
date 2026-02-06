// File watcher: fsevents/inotify → debounce → hash → diff → Yjs pipeline.
// This module handles the first stage: raw FS event detection and filtering.

pub mod debounce;
pub mod hash;
pub mod pipeline;

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};

/// Raw filesystem event emitted by the watcher.
/// Downstream stages (debounce, hash, diff) consume these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEventKind {
    /// File was created or first detected.
    Create,
    /// File content was modified.
    Modify,
    /// File was deleted.
    Remove,
}

/// A raw filesystem event for a single `.md` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFsEvent {
    pub kind: FsEventKind,
    pub path: PathBuf,
}

/// Capacity for the internal event channel.
const EVENT_CHANNEL_CAPACITY: usize = 512;

/// Watches a workspace directory for `.md` file changes using the OS-native
/// file watcher (fsevents on macOS, inotify on Linux).
///
/// Events are sent to the returned receiver. The watcher runs until dropped
/// or `shutdown()` is called.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    root: PathBuf,
}

impl FileWatcher {
    /// Start watching `root` recursively for `.md` file events.
    ///
    /// Returns the watcher handle and a receiver for raw FS events.
    /// The watcher uses the OS-native backend (fsevents on macOS, inotify on Linux).
    pub fn start(root: &Path) -> Result<(Self, mpsc::Receiver<RawFsEvent>)> {
        let root = root
            .canonicalize()
            .with_context(|| format!("failed to canonicalize watch root: {}", root.display()))?;

        let (tx, rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);

        let root_for_filter = root.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            match res {
                Ok(event) => {
                    if let Some(raw_events) = translate_event(&event, &root_for_filter) {
                        for raw in raw_events {
                            if tx.blocking_send(raw).is_err() {
                                // Receiver dropped — watcher will be cleaned up.
                                debug!("event channel closed, stopping event dispatch");
                                return;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "file watcher error");
                }
            }
        })
        .context("failed to create file watcher")?;

        watcher
            .watch(&root, RecursiveMode::Recursive)
            .with_context(|| format!("failed to watch directory: {}", root.display()))?;

        debug!(path = %root.display(), "file watcher started");

        Ok((Self { _watcher: watcher, root }, rx))
    }

    /// The canonicalized root directory being watched.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Returns true if the path has an `.md` extension (case-insensitive).
fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Returns true if the path is inside the watched root (guards against symlink escapes).
fn is_inside_root(path: &Path, root: &Path) -> bool {
    // Use starts_with on the raw path — caller should canonicalize if needed.
    path.starts_with(root)
}

/// Translate a `notify::Event` into zero or more `RawFsEvent`s.
/// Filters for `.md` files inside the root and maps event kinds.
fn translate_event(event: &Event, root: &Path) -> Option<Vec<RawFsEvent>> {
    let kind = match &event.kind {
        EventKind::Create(_) => FsEventKind::Create,
        EventKind::Modify(modify_kind) => {
            use notify::event::ModifyKind;
            match modify_kind {
                // Data or content change — the important one.
                ModifyKind::Data(_) => FsEventKind::Modify,
                // Name changes (renames) — treat as create for the new path.
                ModifyKind::Name(_) => FsEventKind::Modify,
                // Metadata-only changes (permissions, timestamps) — skip.
                ModifyKind::Metadata(_) => {
                    trace!("skipping metadata-only modify event");
                    return None;
                }
                // Catch-all for other/unknown modify kinds — treat as modify.
                _ => FsEventKind::Modify,
            }
        }
        EventKind::Remove(_) => FsEventKind::Remove,
        // Access, Other, Any — not actionable for file content tracking.
        _ => {
            trace!(kind = ?event.kind, "skipping non-content event");
            return None;
        }
    };

    let events: Vec<RawFsEvent> = event
        .paths
        .iter()
        .filter(|p| is_markdown(p))
        .filter(|p| {
            if is_inside_root(p, root) {
                true
            } else {
                warn!(path = %p.display(), "ignoring event outside watch root (possible symlink escape)");
                false
            }
        })
        .map(|p| RawFsEvent { kind: kind.clone(), path: p.clone() })
        .collect();

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind};
    use std::fs;
    use tempfile::TempDir;
    use tokio::time::{timeout, Duration};

    // ── translate_event unit tests ──────────────────────────────────

    fn make_event(kind: EventKind, paths: Vec<PathBuf>) -> Event {
        Event { kind, paths, attrs: Default::default() }
    }

    #[test]
    fn test_create_md_file() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/workspace/notes/doc.md")],
        );
        let result = translate_event(&event, &root).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, FsEventKind::Create);
        assert_eq!(result[0].path, PathBuf::from("/workspace/notes/doc.md"));
    }

    #[test]
    fn test_modify_data_md_file() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            vec![PathBuf::from("/workspace/doc.md")],
        );
        let result = translate_event(&event, &root).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, FsEventKind::Modify);
    }

    #[test]
    fn test_remove_md_file() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Remove(RemoveKind::File),
            vec![PathBuf::from("/workspace/doc.md")],
        );
        let result = translate_event(&event, &root).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, FsEventKind::Remove);
    }

    #[test]
    fn test_filters_non_md_files() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![
                PathBuf::from("/workspace/doc.md"),
                PathBuf::from("/workspace/image.png"),
                PathBuf::from("/workspace/code.rs"),
            ],
        );
        let result = translate_event(&event, &root).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, PathBuf::from("/workspace/doc.md"));
    }

    #[test]
    fn test_all_non_md_returns_none() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/workspace/image.png")],
        );
        assert!(translate_event(&event, &root).is_none());
    }

    #[test]
    fn test_rejects_outside_root() {
        let root = PathBuf::from("/workspace");
        let event =
            make_event(EventKind::Create(CreateKind::File), vec![PathBuf::from("/etc/evil.md")]);
        assert!(translate_event(&event, &root).is_none());
    }

    #[test]
    fn test_skips_metadata_events() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Permissions)),
            vec![PathBuf::from("/workspace/doc.md")],
        );
        assert!(translate_event(&event, &root).is_none());
    }

    #[test]
    fn test_md_extension_case_insensitive() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Create(CreateKind::File),
            vec![PathBuf::from("/workspace/DOC.MD"), PathBuf::from("/workspace/doc.Md")],
        );
        let result = translate_event(&event, &root).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_multiple_md_paths_in_single_event() {
        let root = PathBuf::from("/workspace");
        let event = make_event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            vec![PathBuf::from("/workspace/a.md"), PathBuf::from("/workspace/b.md")],
        );
        let result = translate_event(&event, &root).unwrap();
        assert_eq!(result.len(), 2);
    }

    // ── is_markdown tests ──────────────────────────────────────────

    #[test]
    fn test_is_markdown_various() {
        assert!(is_markdown(Path::new("doc.md")));
        assert!(is_markdown(Path::new("DOC.MD")));
        assert!(is_markdown(Path::new("path/to/notes.md")));
        assert!(!is_markdown(Path::new("doc.txt")));
        assert!(!is_markdown(Path::new("doc.markdown")));
        assert!(!is_markdown(Path::new("doc")));
        assert!(!is_markdown(Path::new(".md"))); // extension is empty, file stem is .md
    }

    #[test]
    fn test_is_markdown_dotfile_with_md_ext() {
        // .hidden.md should match
        assert!(is_markdown(Path::new(".hidden.md")));
    }

    // ── is_inside_root tests ───────────────────────────────────────

    #[test]
    fn test_inside_root() {
        let root = Path::new("/workspace");
        assert!(is_inside_root(Path::new("/workspace/doc.md"), root));
        assert!(is_inside_root(Path::new("/workspace/sub/doc.md"), root));
        assert!(!is_inside_root(Path::new("/other/doc.md"), root));
        assert!(!is_inside_root(Path::new("/workspaceX/doc.md"), root));
    }

    // ── Integration test: actual filesystem ────────────────────────

    #[tokio::test]
    async fn test_watcher_detects_create() {
        let tmp = TempDir::new().unwrap();
        let (watcher, mut rx) = FileWatcher::start(tmp.path()).unwrap();

        // Small delay for watcher registration to settle
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create a .md file
        let file_path = tmp.path().join("test.md");
        fs::write(&file_path, "# Hello").unwrap();

        // Wait for event (with timeout)
        let event = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for create event")
            .expect("channel closed");

        assert!(matches!(event.kind, FsEventKind::Create | FsEventKind::Modify));
        assert!(event.path.ends_with("test.md"));

        drop(watcher);
    }

    #[tokio::test]
    async fn test_watcher_detects_modify() {
        let tmp = TempDir::new().unwrap();

        // Create file before starting watcher
        let file_path = tmp.path().join("existing.md");
        fs::write(&file_path, "initial").unwrap();

        let (watcher, mut rx) = FileWatcher::start(tmp.path()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Modify the file
        fs::write(&file_path, "updated content").unwrap();

        let event = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for modify event")
            .expect("channel closed");

        assert!(matches!(event.kind, FsEventKind::Modify | FsEventKind::Create));
        assert!(event.path.ends_with("existing.md"));

        drop(watcher);
    }

    #[tokio::test]
    async fn test_watcher_detects_delete() {
        let tmp = TempDir::new().unwrap();

        // Create file before starting watcher
        let file_path = tmp.path().join("to_delete.md");
        fs::write(&file_path, "bye").unwrap();

        let (watcher, mut rx) = FileWatcher::start(tmp.path()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Delete the file
        fs::remove_file(&file_path).unwrap();

        // Drain events until we see a Remove (fsevents may emit synthetic
        // Create/Modify events for pre-existing files on startup).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut found_remove = false;
        while tokio::time::Instant::now() < deadline {
            match timeout(Duration::from_secs(2), rx.recv()).await {
                Ok(Some(event)) if event.kind == FsEventKind::Remove => {
                    assert!(event.path.ends_with("to_delete.md"));
                    found_remove = true;
                    break;
                }
                Ok(Some(_)) => continue, // skip non-remove events
                _ => break,
            }
        }
        assert!(found_remove, "expected a Remove event for to_delete.md");

        drop(watcher);
    }

    #[tokio::test]
    async fn test_watcher_ignores_non_md() {
        let tmp = TempDir::new().unwrap();
        let (watcher, mut rx) = FileWatcher::start(tmp.path()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create a non-.md file — should be filtered
        fs::write(tmp.path().join("ignore.txt"), "not markdown").unwrap();

        // Then create a .md file — should get through
        tokio::time::sleep(Duration::from_millis(50)).await;
        fs::write(tmp.path().join("found.md"), "# Markdown").unwrap();

        let event = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("channel closed");

        // The first event we receive should be for the .md file, not the .txt
        assert!(event.path.ends_with("found.md"));

        drop(watcher);
    }

    #[tokio::test]
    async fn test_watcher_recursive_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("nested").join("deep");
        fs::create_dir_all(&subdir).unwrap();

        let (watcher, mut rx) = FileWatcher::start(tmp.path()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Create .md in a subdirectory
        let file_path = subdir.join("nested.md");
        fs::write(&file_path, "# Nested").unwrap();

        let event = timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for recursive event")
            .expect("channel closed");

        assert!(event.path.ends_with("nested.md"));

        drop(watcher);
    }

    #[test]
    fn test_watcher_rejects_nonexistent_root() {
        let result = FileWatcher::start(Path::new("/nonexistent/path/abc123"));
        assert!(result.is_err());
    }

    #[test]
    fn test_watcher_exposes_root() {
        let tmp = TempDir::new().unwrap();
        let (watcher, _rx) = FileWatcher::start(tmp.path()).unwrap();
        // Canonicalized root should match
        assert_eq!(watcher.root(), tmp.path().canonicalize().unwrap());
    }
}
