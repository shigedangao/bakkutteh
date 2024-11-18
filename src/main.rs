use clap::Parser;
use colored::{self, Colorize};

mod cli;
mod kube;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli::Cli::parse();

    // Initialize the kube handler
    let kube_handler = kube::KubeHandler::new(&cli.namespace).await?;

    // Run the command
    if let Err(err) = cli.run(&kube_handler).await {
        println!(
            "Unable to create job due to error: {}",
            err.to_string().red()
        );
    };

    Ok(())
}
