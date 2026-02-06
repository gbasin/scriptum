use clap::Args;

#[derive(Debug, Args)]
pub struct BundleArgs {}

pub fn run(_args: BundleArgs) -> anyhow::Result<()> {
    anyhow::bail!("'bundle' command is not implemented yet")
}
