use clap::Args;

#[derive(Debug, Args)]
pub struct EditArgs {}

pub fn run(_args: EditArgs) -> anyhow::Result<()> {
    anyhow::bail!("'edit' command is not implemented yet")
}
