use clap::Args;

#[derive(Debug, Args)]
pub struct WhoamiArgs {}

pub fn run(_args: WhoamiArgs) -> anyhow::Result<()> {
    anyhow::bail!("'whoami' command is not implemented yet")
}
