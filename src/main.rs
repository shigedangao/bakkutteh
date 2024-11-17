use clap::Parser;

mod cli;
mod kube;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli::Cli::parse();

    // Initialize the kube handler
    let kube_handler = kube::KubeHandler::new(&cli.namespace).await?;

    // Run the command
    cli.run(&kube_handler).await?;

    Ok(())
}
