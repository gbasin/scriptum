// Semantic commit triggers: lease release, comment resolved, explicit checkpoint.
//
// Watches for trigger events and collects changed files since the last commit.
// When a trigger fires and there are uncommitted changes, produces a semantic
// commit message describing the change context.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use uuid::Uuid;

// ── Trigger events ──────────────────────────────────────────────────

/// Events that can trigger a semantic commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEvent {
    /// An agent released a section lease (finished editing).
    LeaseReleased { agent: String, doc_path: String, section_heading: String },
    /// A comment thread was resolved.
    CommentResolved { agent: String, doc_path: String, thread_id: String },
    /// Explicit checkpoint requested (via `scriptum checkpoint` CLI).
    ExplicitCheckpoint { agent: String, message: Option<String> },
    /// Fallback trigger when there are pending changes and no semantic trigger fired.
    IdleFallback,
}

impl TriggerEvent {
    /// The agent that caused this trigger.
    pub fn agent(&self) -> &str {
        match self {
            TriggerEvent::LeaseReleased { agent, .. } => agent,
            TriggerEvent::CommentResolved { agent, .. } => agent,
            TriggerEvent::ExplicitCheckpoint { agent, .. } => agent,
            TriggerEvent::IdleFallback => "scriptum",
        }
    }

    /// Short label for the trigger type.
    pub fn kind(&self) -> &'static str {
        match self {
            TriggerEvent::LeaseReleased { .. } => "lease_released",
            TriggerEvent::CommentResolved { .. } => "comment_resolved",
            TriggerEvent::ExplicitCheckpoint { .. } => "checkpoint",
            TriggerEvent::IdleFallback => "idle_fallback",
        }
    }
}

// ── Changed file tracking ───────────────────────────────────────────

/// Tracks files changed since the last commit.
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    pub doc_id: Option<Uuid>,
    pub change_type: ChangeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
}

// ── Commit context ──────────────────────────────────────────────────

/// Context for generating a semantic commit message.
#[derive(Debug, Clone)]
pub struct CommitContext {
    pub trigger: TriggerEvent,
    pub changed_files: Vec<ChangedFile>,
    pub agents_involved: Vec<String>,
}

impl CommitContext {
    /// Generate a conventional commit message from the trigger context.
    pub fn generate_message(&self) -> String {
        match &self.trigger {
            TriggerEvent::LeaseReleased { agent, doc_path, section_heading } => {
                let scope = path_scope(doc_path);
                let file_summary = self.file_summary();
                format!(
                    "docs({scope}): update {section_heading}\n\n\
                     Triggered by lease release from {agent}.{file_summary}"
                )
            }
            TriggerEvent::CommentResolved { agent, doc_path, thread_id } => {
                let scope = path_scope(doc_path);
                let file_summary = self.file_summary();
                format!(
                    "docs({scope}): resolve comment thread {thread_id}\n\n\
                     Resolved by {agent}.{file_summary}"
                )
            }
            TriggerEvent::ExplicitCheckpoint { agent, message } => {
                let msg = message.as_deref().unwrap_or("manual checkpoint");
                let file_summary = self.file_summary();
                format!(
                    "chore: {msg}\n\n\
                     Checkpoint by {agent}.{file_summary}"
                )
            }
            TriggerEvent::IdleFallback => {
                let file_summary = self.file_summary();
                format!(
                    "chore: idle fallback checkpoint\n\n\
                     Triggered by 30s idle timeout.{file_summary}"
                )
            }
        }
    }

    fn file_summary(&self) -> String {
        if self.changed_files.is_empty() {
            return String::new();
        }

        let mut parts = Vec::new();
        let added =
            self.changed_files.iter().filter(|f| f.change_type == ChangeType::Added).count();
        let modified =
            self.changed_files.iter().filter(|f| f.change_type == ChangeType::Modified).count();
        let deleted =
            self.changed_files.iter().filter(|f| f.change_type == ChangeType::Deleted).count();

        if added > 0 {
            parts.push(format!("{added} added"));
        }
        if modified > 0 {
            parts.push(format!("{modified} modified"));
        }
        if deleted > 0 {
            parts.push(format!("{deleted} deleted"));
        }

        format!("\n\nFiles: {}", parts.join(", "))
    }
}

/// Extract a short scope from a document path.
fn path_scope(doc_path: &str) -> &str {
    // Use the filename without extension as scope.
    let basename = doc_path.rsplit('/').next().unwrap_or(doc_path);
    basename.strip_suffix(".md").unwrap_or(basename)
}

// ── Trigger collector ───────────────────────────────────────────────

/// Configuration for the trigger collector.
#[derive(Debug, Clone)]
pub struct TriggerConfig {
    /// Minimum time between automatic commits (debounce).
    pub min_commit_interval: Duration,
    /// Inactivity threshold before idle fallback auto-commit can fire.
    pub idle_fallback_timeout: Duration,
    /// Maximum number of trigger events to batch before forcing a commit.
    pub max_batch_size: usize,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            min_commit_interval: Duration::from_secs(30),
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 10,
        }
    }
}

/// Collects trigger events and decides when to produce a commit.
pub struct TriggerCollector {
    config: TriggerConfig,
    pending_triggers: Vec<TriggerEvent>,
    changed_paths: HashSet<String>,
    last_edit_at: Option<Instant>,
    last_commit_at: Option<Instant>,
}

impl TriggerCollector {
    pub fn new(config: TriggerConfig) -> Self {
        Self {
            config,
            pending_triggers: Vec::new(),
            changed_paths: HashSet::new(),
            last_edit_at: None,
            last_commit_at: None,
        }
    }

    /// Record a trigger event.
    pub fn push_trigger(&mut self, event: TriggerEvent) {
        self.pending_triggers.push(event);
    }

    /// Record a file change.
    pub fn mark_changed(&mut self, path: &str) {
        self.mark_changed_at(path, Instant::now());
    }

    /// Record a file change at a specific timestamp.
    pub fn mark_changed_at(&mut self, path: &str, at: Instant) {
        self.changed_paths.insert(path.to_string());
        self.last_edit_at = Some(at);
    }

    /// Check if a commit should be triggered now.
    pub fn should_commit(&self, now: Instant) -> bool {
        if self.pending_triggers.is_empty() {
            return self.should_idle_fallback_commit(now);
        }

        // Explicit checkpoints always commit immediately.
        if self
            .pending_triggers
            .iter()
            .any(|t| matches!(t, TriggerEvent::ExplicitCheckpoint { .. }))
        {
            return true;
        }

        // Batch size exceeded.
        if self.pending_triggers.len() >= self.config.max_batch_size {
            return true;
        }

        // Debounce interval passed.
        self.commit_interval_elapsed(now)
    }

    /// Consume pending triggers and produce a commit context.
    /// Returns None if there's nothing to commit.
    pub fn take_commit_context(
        &mut self,
        changed_files: Vec<ChangedFile>,
    ) -> Option<CommitContext> {
        self.take_commit_context_at(Instant::now(), changed_files)
    }

    /// Consume pending triggers and produce a commit context at a specific timestamp.
    /// Returns None if there's nothing to commit.
    pub fn take_commit_context_at(
        &mut self,
        now: Instant,
        changed_files: Vec<ChangedFile>,
    ) -> Option<CommitContext> {
        if self.pending_triggers.is_empty() && self.changed_paths.is_empty() {
            return None;
        }

        let (trigger, agents_involved) = if self.pending_triggers.is_empty() {
            // Fallback commit: no semantic trigger fired before idle timeout.
            (TriggerEvent::IdleFallback, Vec::new())
        } else {
            // Use the most recent trigger as the primary.
            let trigger = self.pending_triggers.last().cloned().unwrap();
            let agents_involved: Vec<String> = self
                .pending_triggers
                .iter()
                .map(|t| t.agent().to_string())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            (trigger, agents_involved)
        };

        self.pending_triggers.clear();
        self.changed_paths.clear();
        self.last_edit_at = None;
        self.last_commit_at = Some(now);

        Some(CommitContext { trigger, changed_files, agents_involved })
    }

    /// Number of pending trigger events.
    pub fn pending_count(&self) -> usize {
        self.pending_triggers.len()
    }

    /// Number of tracked changed paths.
    pub fn changed_path_count(&self) -> usize {
        self.changed_paths.len()
    }

    fn should_idle_fallback_commit(&self, now: Instant) -> bool {
        if self.changed_paths.is_empty() {
            return false;
        }

        let Some(last_edit) = self.last_edit_at else {
            return false;
        };

        let idle_elapsed = now
            .checked_duration_since(last_edit)
            .is_some_and(|elapsed| elapsed >= self.config.idle_fallback_timeout);
        if !idle_elapsed {
            return false;
        }

        self.commit_interval_elapsed(now)
    }

    fn commit_interval_elapsed(&self, now: Instant) -> bool {
        match self.last_commit_at {
            Some(last) => now
                .checked_duration_since(last)
                .is_some_and(|elapsed| elapsed >= self.config.min_commit_interval),
            None => true, // First commit — no debounce.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_event_agent_and_kind() {
        let lease = TriggerEvent::LeaseReleased {
            agent: "alice".into(),
            doc_path: "docs/api.md".into(),
            section_heading: "## Auth".into(),
        };
        assert_eq!(lease.agent(), "alice");
        assert_eq!(lease.kind(), "lease_released");

        let comment = TriggerEvent::CommentResolved {
            agent: "bob".into(),
            doc_path: "docs/readme.md".into(),
            thread_id: "thread-42".into(),
        };
        assert_eq!(comment.agent(), "bob");
        assert_eq!(comment.kind(), "comment_resolved");

        let checkpoint = TriggerEvent::ExplicitCheckpoint {
            agent: "claude".into(),
            message: Some("wip save".into()),
        };
        assert_eq!(checkpoint.agent(), "claude");
        assert_eq!(checkpoint.kind(), "checkpoint");

        let idle = TriggerEvent::IdleFallback;
        assert_eq!(idle.agent(), "scriptum");
        assert_eq!(idle.kind(), "idle_fallback");
    }

    #[test]
    fn commit_message_for_lease_released() {
        let ctx = CommitContext {
            trigger: TriggerEvent::LeaseReleased {
                agent: "alice".into(),
                doc_path: "docs/api.md".into(),
                section_heading: "## Auth".into(),
            },
            changed_files: vec![ChangedFile {
                path: "docs/api.md".into(),
                doc_id: None,
                change_type: ChangeType::Modified,
            }],
            agents_involved: vec!["alice".into()],
        };

        let msg = ctx.generate_message();
        assert!(msg.starts_with("docs(api): update ## Auth"));
        assert!(msg.contains("lease release from alice"));
        assert!(msg.contains("1 modified"));
    }

    #[test]
    fn commit_message_for_comment_resolved() {
        let ctx = CommitContext {
            trigger: TriggerEvent::CommentResolved {
                agent: "bob".into(),
                doc_path: "docs/readme.md".into(),
                thread_id: "thread-42".into(),
            },
            changed_files: vec![],
            agents_involved: vec!["bob".into()],
        };

        let msg = ctx.generate_message();
        assert!(msg.starts_with("docs(readme): resolve comment thread thread-42"));
        assert!(msg.contains("Resolved by bob"));
        // No file summary when no files changed.
        assert!(!msg.contains("Files:"));
    }

    #[test]
    fn commit_message_for_explicit_checkpoint() {
        let ctx = CommitContext {
            trigger: TriggerEvent::ExplicitCheckpoint {
                agent: "claude".into(),
                message: Some("wip: halfway through refactor".into()),
            },
            changed_files: vec![
                ChangedFile {
                    path: "docs/api.md".into(),
                    doc_id: None,
                    change_type: ChangeType::Modified,
                },
                ChangedFile {
                    path: "docs/new.md".into(),
                    doc_id: None,
                    change_type: ChangeType::Added,
                },
            ],
            agents_involved: vec!["claude".into()],
        };

        let msg = ctx.generate_message();
        assert!(msg.starts_with("chore: wip: halfway through refactor"));
        assert!(msg.contains("Checkpoint by claude"));
        assert!(msg.contains("1 added"));
        assert!(msg.contains("1 modified"));
    }

    #[test]
    fn commit_message_for_checkpoint_without_custom_message() {
        let ctx = CommitContext {
            trigger: TriggerEvent::ExplicitCheckpoint { agent: "alice".into(), message: None },
            changed_files: vec![],
            agents_involved: vec!["alice".into()],
        };

        let msg = ctx.generate_message();
        assert!(msg.starts_with("chore: manual checkpoint"));
    }

    #[test]
    fn commit_message_for_idle_fallback() {
        let ctx = CommitContext {
            trigger: TriggerEvent::IdleFallback,
            changed_files: vec![ChangedFile {
                path: "docs/a.md".into(),
                doc_id: None,
                change_type: ChangeType::Modified,
            }],
            agents_involved: vec![],
        };

        let msg = ctx.generate_message();
        assert!(msg.starts_with("chore: idle fallback checkpoint"));
        assert!(msg.contains("Triggered by 30s idle timeout"));
        assert!(msg.contains("1 modified"));
    }

    #[test]
    fn path_scope_extracts_filename_without_extension() {
        assert_eq!(path_scope("docs/api.md"), "api");
        assert_eq!(path_scope("docs/guides/getting-started.md"), "getting-started");
        assert_eq!(path_scope("readme.md"), "readme");
        assert_eq!(path_scope("notes"), "notes");
    }

    #[test]
    fn file_summary_formats_change_counts() {
        let ctx = CommitContext {
            trigger: TriggerEvent::ExplicitCheckpoint { agent: "a".into(), message: None },
            changed_files: vec![
                ChangedFile { path: "a.md".into(), doc_id: None, change_type: ChangeType::Added },
                ChangedFile {
                    path: "b.md".into(),
                    doc_id: None,
                    change_type: ChangeType::Modified,
                },
                ChangedFile {
                    path: "c.md".into(),
                    doc_id: None,
                    change_type: ChangeType::Modified,
                },
                ChangedFile { path: "d.md".into(), doc_id: None, change_type: ChangeType::Deleted },
            ],
            agents_involved: vec!["a".into()],
        };

        let summary = ctx.file_summary();
        assert!(summary.contains("1 added"));
        assert!(summary.contains("2 modified"));
        assert!(summary.contains("1 deleted"));
    }

    // ── TriggerCollector tests ──────────────────────────────────────

    #[test]
    fn collector_tracks_pending_triggers() {
        let mut collector = TriggerCollector::new(TriggerConfig::default());

        assert_eq!(collector.pending_count(), 0);

        collector.push_trigger(TriggerEvent::LeaseReleased {
            agent: "alice".into(),
            doc_path: "docs/api.md".into(),
            section_heading: "## Auth".into(),
        });

        assert_eq!(collector.pending_count(), 1);
    }

    #[test]
    fn collector_tracks_changed_paths() {
        let mut collector = TriggerCollector::new(TriggerConfig::default());

        collector.mark_changed("docs/api.md");
        collector.mark_changed("docs/readme.md");
        collector.mark_changed("docs/api.md"); // duplicate

        assert_eq!(collector.changed_path_count(), 2);
    }

    #[test]
    fn collector_should_commit_on_explicit_checkpoint() {
        let mut collector = TriggerCollector::new(TriggerConfig {
            min_commit_interval: Duration::from_secs(3600), // long debounce
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 100,
        });

        collector.push_trigger(TriggerEvent::ExplicitCheckpoint {
            agent: "alice".into(),
            message: None,
        });

        assert!(collector.should_commit(Instant::now()));
    }

    #[test]
    fn collector_should_not_commit_without_triggers() {
        let collector = TriggerCollector::new(TriggerConfig::default());
        assert!(!collector.should_commit(Instant::now()));
    }

    #[test]
    fn collector_should_commit_when_batch_full() {
        let mut collector = TriggerCollector::new(TriggerConfig {
            min_commit_interval: Duration::from_secs(3600),
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 3,
        });

        for i in 0..3 {
            collector.push_trigger(TriggerEvent::LeaseReleased {
                agent: format!("agent-{i}"),
                doc_path: "docs/api.md".into(),
                section_heading: "## Auth".into(),
            });
        }

        assert!(collector.should_commit(Instant::now()));
    }

    #[test]
    fn collector_debounces_within_interval() {
        let mut collector = TriggerCollector::new(TriggerConfig {
            min_commit_interval: Duration::from_secs(30),
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 100,
        });

        // First commit.
        collector.push_trigger(TriggerEvent::LeaseReleased {
            agent: "alice".into(),
            doc_path: "docs/api.md".into(),
            section_heading: "## Auth".into(),
        });

        let _ = collector.take_commit_context(vec![]); // Consumes and sets last_commit_at.

        // New trigger within debounce window.
        collector.push_trigger(TriggerEvent::LeaseReleased {
            agent: "bob".into(),
            doc_path: "docs/readme.md".into(),
            section_heading: "## Intro".into(),
        });

        // Should not commit yet (within interval).
        assert!(!collector.should_commit(Instant::now()));
    }

    #[test]
    fn collector_take_commit_context_clears_state() {
        let mut collector = TriggerCollector::new(TriggerConfig::default());

        collector.push_trigger(TriggerEvent::LeaseReleased {
            agent: "alice".into(),
            doc_path: "docs/api.md".into(),
            section_heading: "## Auth".into(),
        });
        collector.mark_changed("docs/api.md");

        let ctx = collector.take_commit_context(vec![]).unwrap();
        assert_eq!(ctx.trigger.agent(), "alice");
        assert_eq!(ctx.agents_involved.len(), 1);

        // State is cleared.
        assert_eq!(collector.pending_count(), 0);
        assert_eq!(collector.changed_path_count(), 0);
    }

    #[test]
    fn collector_take_commit_context_returns_none_when_empty() {
        let mut collector = TriggerCollector::new(TriggerConfig::default());
        assert!(collector.take_commit_context(vec![]).is_none());
    }

    #[test]
    fn collector_deduplicates_agents() {
        let mut collector = TriggerCollector::new(TriggerConfig::default());

        collector.push_trigger(TriggerEvent::LeaseReleased {
            agent: "alice".into(),
            doc_path: "docs/a.md".into(),
            section_heading: "## X".into(),
        });
        collector.push_trigger(TriggerEvent::LeaseReleased {
            agent: "alice".into(),
            doc_path: "docs/b.md".into(),
            section_heading: "## Y".into(),
        });
        collector.push_trigger(TriggerEvent::LeaseReleased {
            agent: "bob".into(),
            doc_path: "docs/c.md".into(),
            section_heading: "## Z".into(),
        });

        let ctx = collector.take_commit_context(vec![]).unwrap();
        // Should have unique agents.
        let mut agents = ctx.agents_involved.clone();
        agents.sort();
        assert_eq!(agents, vec!["alice", "bob"]);
    }

    #[test]
    fn collector_idle_fallback_commits_after_timeout() {
        let mut collector = TriggerCollector::new(TriggerConfig {
            min_commit_interval: Duration::from_secs(30),
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 100,
        });

        let t0 = Instant::now();
        collector.mark_changed_at("docs/api.md", t0);

        assert!(!collector.should_commit(t0 + Duration::from_secs(29)));
        assert!(collector.should_commit(t0 + Duration::from_secs(30)));
    }

    #[test]
    fn collector_idle_fallback_resets_timer_on_each_edit() {
        let mut collector = TriggerCollector::new(TriggerConfig {
            min_commit_interval: Duration::from_secs(30),
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 100,
        });

        let t0 = Instant::now();
        collector.mark_changed_at("docs/api.md", t0);
        collector.mark_changed_at("docs/api.md", t0 + Duration::from_secs(20));

        assert!(!collector.should_commit(t0 + Duration::from_secs(49)));
        assert!(collector.should_commit(t0 + Duration::from_secs(50)));
    }

    #[test]
    fn collector_idle_fallback_emits_context_without_semantic_trigger() {
        let mut collector = TriggerCollector::new(TriggerConfig {
            min_commit_interval: Duration::from_secs(30),
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 100,
        });

        let t0 = Instant::now();
        collector.mark_changed_at("docs/api.md", t0);

        let ctx = collector
            .take_commit_context_at(
                t0 + Duration::from_secs(30),
                vec![ChangedFile {
                    path: "docs/api.md".into(),
                    doc_id: None,
                    change_type: ChangeType::Modified,
                }],
            )
            .expect("idle fallback should produce commit context");

        assert!(matches!(ctx.trigger, TriggerEvent::IdleFallback));
        assert!(ctx.agents_involved.is_empty());
        assert_eq!(collector.pending_count(), 0);
        assert_eq!(collector.changed_path_count(), 0);
    }

    #[test]
    fn collector_idle_fallback_respects_max_one_auto_commit_per_interval() {
        let mut collector = TriggerCollector::new(TriggerConfig {
            min_commit_interval: Duration::from_secs(45),
            idle_fallback_timeout: Duration::from_secs(30),
            max_batch_size: 100,
        });

        let t0 = Instant::now();
        collector.mark_changed_at("docs/a.md", t0);
        assert!(collector.should_commit(t0 + Duration::from_secs(30)));
        let _ = collector.take_commit_context_at(t0 + Duration::from_secs(30), vec![]);

        collector.mark_changed_at("docs/b.md", t0 + Duration::from_secs(31));
        assert!(!collector.should_commit(t0 + Duration::from_secs(61)));
        assert!(collector.should_commit(t0 + Duration::from_secs(75)));
    }
}
