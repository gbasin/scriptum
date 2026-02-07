// `scriptum setup claude` — install Claude Code hooks into .claude/settings.json.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[command(subcommand)]
    pub target: SetupTarget,
}

#[derive(Debug, Subcommand)]
pub enum SetupTarget {
    /// Install Claude Code hooks for Scriptum integration
    Claude(ClaudeArgs),
}

#[derive(Debug, Args)]
pub struct ClaudeArgs {
    /// Remove previously installed hooks
    #[arg(long)]
    pub remove: bool,
}

pub fn run(args: SetupArgs) -> anyhow::Result<()> {
    match args.target {
        SetupTarget::Claude(claude_args) => {
            let project_root = std::env::current_dir()?;
            if claude_args.remove {
                remove_claude_hooks(&project_root)
            } else {
                install_claude_hooks(&project_root)
            }
        }
    }
}

// ── Public API (for testing) ────────────────────────────────────────

pub fn install_claude_hooks(project_root: &Path) -> anyhow::Result<()> {
    let claude_dir = project_root.join(".claude");
    let hooks_dir = claude_dir.join("hooks").join("scriptum");
    fs::create_dir_all(&hooks_dir)?;

    // Generate hook scripts.
    for (name, content) in hook_scripts() {
        let script_path = hooks_dir.join(name);
        fs::write(&script_path, content)?;
        #[cfg(unix)]
        set_executable(&script_path)?;
    }

    // Merge into .claude/settings.json.
    let settings_path = claude_dir.join("settings.json");
    let mut settings = read_settings(&settings_path)?;
    merge_hooks(&mut settings);
    write_settings(&settings_path, &settings)?;

    eprintln!("Scriptum hooks installed into {}", settings_path.display());
    eprintln!("Hook scripts written to {}", hooks_dir.display());
    Ok(())
}

pub fn remove_claude_hooks(project_root: &Path) -> anyhow::Result<()> {
    let claude_dir = project_root.join(".claude");
    let hooks_dir = claude_dir.join("hooks").join("scriptum");

    // Remove scripts directory.
    if hooks_dir.exists() {
        fs::remove_dir_all(&hooks_dir)?;
        eprintln!("Removed {}", hooks_dir.display());
    }

    // Remove hooks from settings.json.
    let settings_path = claude_dir.join("settings.json");
    if settings_path.exists() {
        let mut settings = read_settings(&settings_path)?;
        strip_hooks(&mut settings);
        write_settings(&settings_path, &settings)?;
        eprintln!("Removed Scriptum hooks from {}", settings_path.display());
    }

    Ok(())
}

// ── Settings JSON ───────────────────────────────────────────────────

/// Minimal representation of .claude/settings.json.
/// We preserve unknown keys via `extra`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ClaudeSettings {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    hooks: BTreeMap<String, Vec<HookGroup>>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct HookGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    matcher: Option<String>,
    hooks: Vec<HookEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct HookEntry {
    #[serde(rename = "type")]
    hook_type: String,
    command: String,
}

fn read_settings(path: &Path) -> anyhow::Result<ClaudeSettings> {
    if !path.exists() {
        return Ok(ClaudeSettings::default());
    }
    let content = fs::read_to_string(path)?;
    let settings: ClaudeSettings = serde_json::from_str(&content)?;
    Ok(settings)
}

fn write_settings(path: &Path, settings: &ClaudeSettings) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(settings)?;
    fs::write(path, content + "\n")?;
    Ok(())
}

// ── Hook definitions ────────────────────────────────────────────────

fn scriptum_hooks() -> Vec<(&'static str, Option<&'static str>, &'static str)> {
    // (event_name, matcher, command)
    vec![
        ("SessionStart", None, "scriptum:session-start"),
        ("PreCompact", None, "scriptum:pre-compact"),
        ("PreToolUse", Some("Write|Edit"), "scriptum:pre-tool-use"),
        ("PostToolUse", Some("Write|Edit"), "scriptum:post-tool-use"),
        ("Stop", None, "scriptum:stop"),
    ]
}

fn hook_command(hooks_dir_relative: &str, script_name: &str) -> String {
    format!("{hooks_dir_relative}/{script_name}")
}

fn merge_hooks(settings: &mut ClaudeSettings) {
    let hooks_rel = ".claude/hooks/scriptum";

    for (event, matcher, script_tag) in scriptum_hooks() {
        let script_name = script_tag.strip_prefix("scriptum:").unwrap();
        let command = hook_command(hooks_rel, &format!("{script_name}.sh"));
        let entry = HookEntry { hook_type: "command".to_string(), command };
        let group = HookGroup { matcher: matcher.map(|s| s.to_string()), hooks: vec![entry] };

        let groups = settings.hooks.entry(event.to_string()).or_default();

        // Remove any existing scriptum hook for this event.
        groups.retain(|g| !is_scriptum_group(g));
        groups.push(group);
    }
}

fn strip_hooks(settings: &mut ClaudeSettings) {
    for groups in settings.hooks.values_mut() {
        groups.retain(|g| !is_scriptum_group(g));
    }
    // Remove empty event keys.
    settings.hooks.retain(|_, groups| !groups.is_empty());
}

fn is_scriptum_group(group: &HookGroup) -> bool {
    group.hooks.iter().any(|h| h.command.contains("hooks/scriptum/"))
}

// ── Hook script generation ──────────────────────────────────────────

fn hook_scripts() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "session-start.sh",
            r#"#!/usr/bin/env bash
# Scriptum SessionStart hook — inject workspace context into Claude session.
set -euo pipefail

echo "=== Scriptum Agent State ==="
scriptum whoami 2>/dev/null || echo "(agent identity unavailable)"
echo ""
echo "=== Scriptum Workspace Status ==="
scriptum status 2>/dev/null || echo "(scriptum daemon not running)"
echo ""
echo "=== Scriptum Overlap Warnings ==="
scriptum conflicts 2>/dev/null || true
echo ""
echo "=== Scriptum CLI Quick Reference ==="
echo "  scriptum read <doc>           Read document or section"
echo "  scriptum edit <doc>           Edit document or section"
echo "  scriptum tree <doc>           Show section tree"
echo "  scriptum ls                   List workspace documents"
echo "  scriptum status               Show agent state and overlaps"
echo "  scriptum conflicts            Show section overlap warnings"
echo "  scriptum claim <section>      Claim advisory lease"
echo "  scriptum blame <doc>          CRDT-based attribution"
echo "  scriptum bundle <doc>         Context bundling for agents"
echo "  scriptum agents               List active agents"
"#,
        ),
        (
            "pre-compact.sh",
            r#"#!/usr/bin/env bash
# Scriptum PreCompact hook — preserve context across /compact.
set -euo pipefail

echo "=== Scriptum Context Snapshot (preserve after /compact) ==="
echo "Keep this block so Scriptum state survives compaction."
echo ""
echo "=== Scriptum Agent State ==="
scriptum whoami 2>/dev/null || echo "(agent identity unavailable)"
echo ""
echo "=== Scriptum Workspace Status ==="
scriptum status 2>/dev/null || echo "(status unavailable)"
echo ""
echo "=== Scriptum Active Overlaps ==="
scriptum conflicts 2>/dev/null || true
echo ""
echo "=== Scriptum CLI Quick Reference ==="
echo "  scriptum read <doc>           Read document or section"
echo "  scriptum edit <doc>           Edit document or section"
echo "  scriptum status               Show agent state and overlaps"
echo "  scriptum conflicts            Show section overlap warnings"
echo "  scriptum claim <section>      Claim advisory lease"
"#,
        ),
        (
            "pre-tool-use.sh",
            r#"#!/usr/bin/env bash
# Scriptum PreToolUse hook — warn about .md file edits.
# Triggered on Write|Edit tools. Warns about section overlaps
# and suggests using scriptum edit for attribution.
set -euo pipefail

# Parse the tool input to check if target is a .md file.
# Claude Code passes tool input as JSON via stdin.
INPUT=$(cat)
FILE=$(echo "$INPUT" | grep -oP '"file_path"\s*:\s*"[^"]*\.md"' || true)

if [ -n "$FILE" ]; then
    echo "⚠ Scriptum: Direct .md edit detected."
    echo "  Prefer \`scriptum edit\` for CRDT attribution and section-level sync."
    echo ""
    echo "Section overlap check:"
    scriptum conflicts 2>/dev/null || echo "  (overlap check unavailable — daemon not running)"
fi
"#,
        ),
        (
            "post-tool-use.sh",
            r#"#!/usr/bin/env bash
# Scriptum PostToolUse hook — confirm file sync after .md edits.
# Triggered on Write|Edit tools. Confirms file watcher will sync
# the change and reports any active section conflicts.
set -euo pipefail

# Parse tool input for .md file path.
INPUT=$(cat)
FILE=$(echo "$INPUT" | grep -oP '"file_path"\s*:\s*"[^"]*\.md"' || true)

if [ -n "$FILE" ]; then
    echo "Scriptum: File watcher will sync this change to CRDT."
    echo "  Run \`scriptum status\` to verify sync completed."
    CONFLICTS=$(scriptum conflicts 2>/dev/null || true)
    if [ -n "$CONFLICTS" ]; then
        echo ""
        echo "Active conflicts:"
        echo "$CONFLICTS"
    fi
fi
"#,
        ),
        (
            "stop.sh",
            r#"#!/usr/bin/env bash
# Scriptum Stop hook — check for unsynced changes at session end.
# Warns if there are pending changes that haven't been synced to CRDT.
set -euo pipefail

echo "=== Scriptum Session End ==="
echo ""

# Check for pending/unsynced changes.
STATUS=$(scriptum status 2>/dev/null || echo "")
if [ -n "$STATUS" ]; then
    echo "$STATUS"
    if echo "$STATUS" | grep -qi "pending\|unsynced\|dirty"; then
        echo ""
        echo "⚠ Warning: You may have unsynced changes."
        echo "  Ensure the daemon is running and changes are persisted."
    fi
else
    echo "(Scriptum status unavailable — daemon not running)"
fi

echo ""
echo "Overlap check:"
scriptum conflicts 2>/dev/null || echo "  (no conflicts or daemon not running)"
"#,
        ),
    ]
}

// ── Platform helpers ────────────────────────────────────────────────

#[cfg(unix)]
fn set_executable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_temp_project() -> TempDir {
        tempfile::tempdir().expect("should create temp dir")
    }

    fn hook_script_text(script_name: &str) -> &'static str {
        hook_scripts()
            .into_iter()
            .find_map(|(name, script)| (name == script_name).then_some(script))
            .expect("hook script should exist")
    }

    #[test]
    fn session_start_script_injects_agent_state_workspace_status_and_cli_reference() {
        let script = hook_script_text("session-start.sh");
        assert!(script.contains("=== Scriptum Agent State ==="));
        assert!(script.contains("scriptum whoami"));
        assert!(script.contains("=== Scriptum Workspace Status ==="));
        assert!(script.contains("scriptum status"));
        assert!(script.contains("=== Scriptum Overlap Warnings ==="));
        assert!(script.contains("scriptum conflicts"));
        assert!(script.contains("=== Scriptum CLI Quick Reference ==="));
    }

    #[test]
    fn pre_compact_script_preserves_context_for_post_compaction() {
        let script = hook_script_text("pre-compact.sh");
        assert!(script.contains("preserve after /compact"));
        assert!(script.contains("state survives compaction"));
        assert!(script.contains("=== Scriptum Agent State ==="));
        assert!(script.contains("scriptum whoami"));
        assert!(script.contains("=== Scriptum Workspace Status ==="));
        assert!(script.contains("scriptum status"));
        assert!(script.contains("=== Scriptum Active Overlaps ==="));
        assert!(script.contains("scriptum conflicts"));
        assert!(script.contains("=== Scriptum CLI Quick Reference ==="));
    }

    #[test]
    fn install_creates_settings_and_scripts() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        install_claude_hooks(root).expect("install should succeed");

        // Settings file should exist.
        let settings_path = root.join(".claude/settings.json");
        assert!(settings_path.exists(), "settings.json should be created");

        // All 5 scripts should exist.
        let hooks_dir = root.join(".claude/hooks/scriptum");
        for name in
            ["session-start.sh", "pre-compact.sh", "pre-tool-use.sh", "post-tool-use.sh", "stop.sh"]
        {
            let script = hooks_dir.join(name);
            assert!(script.exists(), "{name} should exist");

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&script).unwrap().permissions().mode();
                assert_eq!(mode & 0o111, 0o111, "{name} should be executable");
            }
        }
    }

    #[test]
    fn install_populates_settings_with_all_hooks() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        install_claude_hooks(root).unwrap();

        let settings: ClaudeSettings =
            serde_json::from_str(&fs::read_to_string(root.join(".claude/settings.json")).unwrap())
                .unwrap();

        assert!(settings.hooks.contains_key("SessionStart"));
        assert!(settings.hooks.contains_key("PreCompact"));
        assert!(settings.hooks.contains_key("PreToolUse"));
        assert!(settings.hooks.contains_key("PostToolUse"));
        assert!(settings.hooks.contains_key("Stop"));

        // PreToolUse should have a matcher.
        let pre = &settings.hooks["PreToolUse"][0];
        assert_eq!(pre.matcher.as_deref(), Some("Write|Edit"));
    }

    #[test]
    fn install_preserves_existing_settings() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        // Write existing settings.
        let claude_dir = root.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("settings.json"), r#"{"permissions":{"allow":["Bash(git:*)"]}}"#)
            .unwrap();

        install_claude_hooks(root).unwrap();

        let settings: ClaudeSettings =
            serde_json::from_str(&fs::read_to_string(claude_dir.join("settings.json")).unwrap())
                .unwrap();

        // Hooks should be added.
        assert!(settings.hooks.contains_key("SessionStart"));

        // Existing permissions should be preserved.
        assert!(settings.extra.contains_key("permissions"));
    }

    #[test]
    fn install_is_idempotent() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        install_claude_hooks(root).unwrap();
        install_claude_hooks(root).unwrap();

        let settings: ClaudeSettings =
            serde_json::from_str(&fs::read_to_string(root.join(".claude/settings.json")).unwrap())
                .unwrap();

        // Should have exactly one hook group per event.
        assert_eq!(settings.hooks["SessionStart"].len(), 1);
        assert_eq!(settings.hooks["PreToolUse"].len(), 1);
    }

    #[test]
    fn install_preserves_non_scriptum_hooks() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        // Write settings with an existing non-scriptum hook.
        let claude_dir = root.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"some-other-hook.sh"}]}]}}"#,
        )
        .unwrap();

        install_claude_hooks(root).unwrap();

        let settings: ClaudeSettings =
            serde_json::from_str(&fs::read_to_string(claude_dir.join("settings.json")).unwrap())
                .unwrap();

        // Should have both the existing hook and the new one.
        assert_eq!(settings.hooks["PreToolUse"].len(), 2);
    }

    #[test]
    fn remove_strips_hooks_and_deletes_scripts() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        install_claude_hooks(root).unwrap();
        remove_claude_hooks(root).unwrap();

        // Scripts directory should be removed.
        let hooks_dir = root.join(".claude/hooks/scriptum");
        assert!(!hooks_dir.exists(), "hooks dir should be removed");

        // Settings should have no hooks.
        let settings: ClaudeSettings =
            serde_json::from_str(&fs::read_to_string(root.join(".claude/settings.json")).unwrap())
                .unwrap();
        assert!(settings.hooks.is_empty(), "hooks should be empty after removal");
    }

    #[test]
    fn remove_preserves_non_scriptum_hooks() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        // Write settings with a non-scriptum hook + scriptum hooks.
        let claude_dir = root.join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(
            claude_dir.join("settings.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"other.sh"}]},{"matcher":"Write|Edit","hooks":[{"type":"command","command":".claude/hooks/scriptum/pre-tool-use.sh"}]}]}}"#,
        )
        .unwrap();

        remove_claude_hooks(root).unwrap();

        let settings: ClaudeSettings =
            serde_json::from_str(&fs::read_to_string(claude_dir.join("settings.json")).unwrap())
                .unwrap();

        assert_eq!(settings.hooks["PreToolUse"].len(), 1);
        assert_eq!(settings.hooks["PreToolUse"][0].hooks[0].command, "other.sh");
    }

    #[test]
    fn remove_on_clean_project_is_noop() {
        let tmp = setup_temp_project();
        let root = tmp.path();

        // Should not fail even if nothing is installed.
        remove_claude_hooks(root).unwrap();
    }

    #[test]
    fn script_content_starts_with_shebang() {
        for (name, content) in hook_scripts() {
            assert!(content.starts_with("#!/usr/bin/env bash"), "{name} should have bash shebang");
        }
    }

    #[test]
    fn settings_round_trip_preserves_structure() {
        let tmp = setup_temp_project();
        let root = tmp.path();
        let path = root.join("test-settings.json");

        let mut settings = ClaudeSettings::default();
        merge_hooks(&mut settings);
        write_settings(&path, &settings).unwrap();

        let loaded = read_settings(&path).unwrap();
        assert_eq!(settings.hooks.len(), loaded.hooks.len());

        for (event, groups) in &settings.hooks {
            assert_eq!(groups, &loaded.hooks[event]);
        }
    }

    #[test]
    fn pre_tool_use_script_warns_about_overlaps_and_suggests_scriptum_edit() {
        let script = hook_script_text("pre-tool-use.sh");
        assert!(script.contains("file_path"), "should parse file_path from JSON");
        assert!(script.contains(".md"), "should detect .md files");
        assert!(script.contains("scriptum edit"), "should suggest scriptum edit for attribution");
        assert!(script.contains("scriptum conflicts"), "should check for section overlaps");
        assert!(script.contains("CRDT attribution"), "should mention CRDT attribution benefit");
    }

    #[test]
    fn post_tool_use_script_confirms_sync_and_reports_conflicts() {
        let script = hook_script_text("post-tool-use.sh");
        assert!(script.contains("file_path"), "should parse file_path from JSON");
        assert!(script.contains(".md"), "should detect .md files");
        assert!(script.contains("File watcher will sync"), "should confirm file watcher sync");
        assert!(script.contains("scriptum status"), "should suggest verifying sync status");
        assert!(script.contains("scriptum conflicts"), "should check for conflicts");
    }

    #[test]
    fn stop_script_warns_about_unsynced_changes() {
        let script = hook_script_text("stop.sh");
        assert!(script.contains("Session End"), "should show session end banner");
        assert!(script.contains("scriptum status"), "should check status for pending changes");
        assert!(script.contains("unsynced"), "should warn about unsynced changes");
        assert!(script.contains("scriptum conflicts"), "should check for overlaps at session end");
    }

    #[test]
    fn hook_commands_point_to_correct_paths() {
        let mut settings = ClaudeSettings::default();
        merge_hooks(&mut settings);

        let session_start = &settings.hooks["SessionStart"][0];
        assert_eq!(session_start.hooks[0].command, ".claude/hooks/scriptum/session-start.sh");

        let pre_tool = &settings.hooks["PreToolUse"][0];
        assert_eq!(pre_tool.hooks[0].command, ".claude/hooks/scriptum/pre-tool-use.sh");
    }
}
