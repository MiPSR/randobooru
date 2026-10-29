mod app;
mod booru;
mod cli;
mod config;
mod db;
mod i18n;
mod pacing;
mod settings;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
	if let Err(err) = app::run().await {
		cli::error(&format!("{err:#}"), "FATAL");
		return Err(anyhow::anyhow!("randobooru stopped; see the error above"));
	}
	Ok(())
}
