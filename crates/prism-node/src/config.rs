use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug)]
#[command(name = "prism-node", about = "Prism P2P node")]
pub struct Cli {
    /// Path to config TOML file
    #[arg(short, long, default_value = "config.toml")]
    pub config: std::path::PathBuf,

    /// Override listen address (e.g. /ip4/0.0.0.0/udp/4001/quic-v1)
    #[arg(long)]
    pub listen: Option<String>,

    /// Override capacity class (A|B|C|edge)
    #[arg(long)]
    pub capacity_class: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Config {
    pub listen_addr: Option<String>,
    pub key_path: Option<std::path::PathBuf>,
    pub bootstrap_peers: Vec<String>,
    pub region: Option<String>,
    pub capacity_class: Option<String>,
}

impl Config {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read config {}: {}", path.display(), e))?;
        toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("config parse error in {}: {}", path.display(), e))
    }

    pub fn merge_cli(&mut self, cli: &Cli) {
        if let Some(addr) = &cli.listen {
            self.listen_addr = Some(addr.clone());
        }
        if let Some(class) = &cli.capacity_class {
            self.capacity_class = Some(class.clone());
        }
    }
}
