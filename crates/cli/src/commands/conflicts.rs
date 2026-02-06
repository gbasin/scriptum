use clap::Args;

#[derive(Debug, Args)]
pub struct ConflictsArgs {}

pub fn run(_args: ConflictsArgs) -> anyhow::Result<()> {
    anyhow::bail!("'conflicts' command is not implemented yet")
}
