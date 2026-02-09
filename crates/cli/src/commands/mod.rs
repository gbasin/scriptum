// CLI subcommand dispatch.

use clap::Subcommand;

pub mod agents;
pub mod blame;
pub mod bundle;
pub mod checkpoint;
pub mod claim;
pub mod conflicts;
pub mod diff;
pub mod edit;
pub mod ls;
pub mod peek;
pub mod read;
pub mod search;
pub mod sections;
pub mod setup;
pub mod status;
pub mod tree;
pub mod whoami;

#[derive(Subcommand)]
pub enum Command {
    /// Read a document or section
    Read(read::ReadArgs),
    /// Edit a document or section
    Edit(edit::EditArgs),
    /// Show section tree structure with IDs
    Tree(tree::TreeArgs),
    /// List sections with metadata
    Sections(sections::SectionsArgs),
    /// Full-text search across documents
    Search(search::SearchArgs),
    /// Show pending changes since last commit
    Diff(diff::DiffArgs),
    /// List workspace documents
    Ls(ls::LsArgs),
    /// CRDT-based per-line attribution
    Blame(blame::BlameArgs),
    /// Claim an advisory lease on a section
    Claim(claim::ClaimArgs),
    /// Context bundling with token budget
    Bundle(bundle::BundleArgs),
    /// Trigger an explicit git checkpoint commit
    Checkpoint(checkpoint::CheckpointArgs),
    /// Show agent identity and workspace state
    Whoami(whoami::WhoamiArgs),
    /// Show agent's active sections and overlaps
    Status(status::StatusArgs),
    /// Show section overlap warnings
    Conflicts(conflicts::ConflictsArgs),
    /// List active agents
    Agents(agents::AgentsArgs),
    /// Set up integrations (e.g. Claude hooks)
    Setup(setup::SetupArgs),
    /// Read without registering intent
    Peek(peek::PeekArgs),
}

pub fn run(cmd: Command) -> anyhow::Result<()> {
    match cmd {
        Command::Read(args) => read::run(args),
        Command::Edit(args) => edit::run(args),
        Command::Tree(args) => tree::run(args),
        Command::Sections(args) => sections::run(args),
        Command::Search(args) => search::run(args),
        Command::Diff(args) => diff::run(args),
        Command::Ls(args) => ls::run(args),
        Command::Blame(args) => blame::run(args),
        Command::Claim(args) => claim::run(args),
        Command::Bundle(args) => bundle::run(args),
        Command::Checkpoint(args) => checkpoint::run(args),
        Command::Whoami(args) => whoami::run(args),
        Command::Status(args) => status::run(args),
        Command::Conflicts(args) => conflicts::run(args),
        Command::Agents(args) => agents::run(args),
        Command::Setup(args) => setup::run(args),
        Command::Peek(args) => peek::run(args),
    }
}
