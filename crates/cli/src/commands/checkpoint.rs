// `scriptum checkpoint` — trigger an explicit git checkpoint commit.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

const DEFAULT_CHECKPOINT_MESSAGE: &str = "chore: manual checkpoint";

#[derive(Debug, Args)]
pub struct CheckpointArgs {
    /// Optional commit message for this checkpoint.
    #[arg(long)]
    message: Option<String>,

    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointResult {
    pub job_id: String,
}

pub fn run(args: CheckpointArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let message = resolve_checkpoint_message(args.message)?;

    let rt = tokio::runtime::Handle::try_current()
        .map(|h| h.block_on(call_checkpoint(message.clone())))
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build")
                .block_on(call_checkpoint(message))
        });

    match rt {
        Ok(result) => {
            output::print_output(format, &result, format_human)?;
            Ok(())
        }
        Err(error) => {
            output::print_error(format, "RPC_ERROR", &format!("{error:#}"));
            Err(error)
        }
    }
}

fn resolve_checkpoint_message(message: Option<String>) -> anyhow::Result<String> {
    match message {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                anyhow::bail!("--message must not be empty");
            }
            Ok(trimmed.to_string())
        }
        None => Ok(DEFAULT_CHECKPOINT_MESSAGE.to_string()),
    }
}

async fn call_checkpoint(message: String) -> anyhow::Result<CheckpointResult> {
    let client = DaemonClient::default();
    client
        .call(
            rpc_methods::GIT_SYNC,
            json!({
                "action": {
                    "commit": {
                        "message": message,
                    }
                }
            }),
        )
        .await
}

fn format_human(result: &CheckpointResult) -> String {
    format!("Checkpoint queued (job: {})", result.job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> CheckpointResult {
        CheckpointResult { job_id: "be7f8e6e-b3a0-4f1e-8bd3-4b40dfc78d67".to_string() }
    }

    #[test]
    fn resolve_checkpoint_message_defaults_when_missing() {
        let message = resolve_checkpoint_message(None).expect("default message should resolve");
        assert_eq!(message, DEFAULT_CHECKPOINT_MESSAGE);
    }

    #[test]
    fn resolve_checkpoint_message_rejects_empty_custom_message() {
        let error = resolve_checkpoint_message(Some("   ".to_string()))
            .expect_err("empty message should be rejected");
        assert!(error.to_string().contains("must not be empty"));
    }

    #[test]
    fn resolve_checkpoint_message_trims_custom_message() {
        let message = resolve_checkpoint_message(Some("  docs: checkpoint now  ".to_string()))
            .expect("custom message should resolve");
        assert_eq!(message, "docs: checkpoint now");
    }

    #[test]
    fn human_format_shows_job_id() {
        let output = format_human(&sample_result());
        assert!(output.contains("Checkpoint queued"));
        assert!(output.contains("be7f8e6e-b3a0-4f1e-8bd3-4b40dfc78d67"));
    }

    #[test]
    fn json_format_roundtrips() {
        let result = sample_result();
        let mut buf = Vec::new();
        output::write_output(&mut buf, OutputFormat::Json, &result, format_human)
            .expect("json output should serialize");

        let parsed: CheckpointResult =
            serde_json::from_slice(&buf).expect("json output should deserialize");
        assert_eq!(parsed.job_id, result.job_id);
    }
}
