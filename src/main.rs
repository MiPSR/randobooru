mod app;
mod booru;
mod cli;
mod config;
mod db;
mod i18n;
mod pacing;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
	if let Err(err) = app::run().await {
		cli::error(&err.to_string(), "error");
	}
	Ok(())
}
