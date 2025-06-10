use clap::{Args, Parser};
use library::cli::{Cli, MalachiteChainSpecParser};
use library::node::MalachiteNode;
use reth::builder::NodeHandle;

/// No Additional arguments
#[derive(Debug, Clone, Copy, Default, Args)]
#[non_exhaustive]
struct NoArgs;

fn main() -> eyre::Result<()> {
    reth_cli_util::sigsegv_handler::install();

    Cli::<MalachiteChainSpecParser, NoArgs>::parse().run(|builder, _: NoArgs| async move {
        let node = MalachiteNode::new();
        let NodeHandle {
            node: _,
            node_exit_future,
        } = builder.node(node).launch().await?;

        node_exit_future.await
    })?;
    Ok(())
}

