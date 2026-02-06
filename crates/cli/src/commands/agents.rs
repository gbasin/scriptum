use clap::Args;

#[derive(Debug, Args)]
pub struct AgentsArgs {}

pub fn run(_args: AgentsArgs) -> anyhow::Result<()> {
    anyhow::bail!("'agents' command is not implemented yet")
}
