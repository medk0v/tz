use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use tz_backend::{Config, bootstrap::create_lite_admin};

#[tokio::main]
async fn main() -> Result<()> {
    let mut config = None;
    let mut email = None;
    let mut credentials = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--config" => {
                config = Some(PathBuf::from(
                    args.next().context("missing --config value")?,
                ));
            }
            "--email" => email = Some(args.next().context("missing --email value")?),
            "--credentials" => {
                credentials = Some(PathBuf::from(
                    args.next().context("missing --credentials value")?,
                ));
            }
            "--help" | "-h" => {
                println!(
                    "Usage: bootstrap-admin --config <path> --email <email> --credentials <absolute-new-file>"
                );
                return Ok(());
            }
            _ => bail!("unknown argument: {argument}"),
        }
    }
    let config = Config::from_file(config.context("--config is required")?)?;
    let credentials = credentials.context("--credentials is required")?;
    let created = create_lite_admin(
        &config,
        &email.context("--email is required")?,
        &credentials,
    )
    .await?;
    println!(
        "{}",
        if created {
            "Lite administrator created; credentials saved to the requested owner-only file"
        } else {
            "Lite administrator already initialized; credentials unchanged"
        }
    );
    Ok(())
}
