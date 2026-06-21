use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cairn-node", about = "A Cairn federation node")]
struct Cli {
    #[arg(long, env = "CAIRN_CONN")] conn: String,
    #[arg(long, default_value = "node.key")] key: PathBuf,
    #[command(subcommand)] cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Provision this node: mint a keypair and append the genesis enrollment.
    Init { #[arg(long)] name: String, #[arg(long)] address: String },
    /// Print this node's identity (node_id, pubkey, fingerprint, address).
    Identity,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init { name, address } => {
            let db = cairn_node::db::connect_and_load_schema(&cli.conn).await?;
            let (sk, kid) = cairn_node::keystore::generate_and_seal(&cli.key, None)?;
            let node_id = cairn_node::identity::provision(&db, &sk, &kid, &name, &address).await?;
            println!("provisioned node {node_id}\nfingerprint {}", cairn_event::short_fingerprint(&kid)?);
        }
        Cmd::Identity => {
            let db = cairn_node::db::connect(&cli.conn).await?;
            let id = cairn_node::identity::load_local(&db).await?;
            println!("node_id     {}\npubkey      {}\nfingerprint {}\naddress     {}",
                id.node_id_hex, id.pubkey_hex, id.fingerprint, id.address);
        }
    }
    Ok(())
}
