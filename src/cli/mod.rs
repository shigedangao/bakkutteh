use crate::kube::spec::SpecHandler;
use crate::kube::KubeHandler;
use anyhow::{anyhow, Result};
use clap::Parser;

#[derive(Parser)]
#[command(
    version = "0.0.1",
    about = "A command to dispatch a kubernetes job from a cronjob spec"
)]
pub struct Cli {
    #[arg(short, long)]
    job_name: String,

    #[arg(short, long, default_value = "default")]
    pub namespace: String,
}

impl Cli {
    pub async fn run<S: AsRef<str>>(&self, kube_handler: &KubeHandler<S>) -> Result<()> {
        // Get the targeted cronjob
        let job_tmpl_spec = kube_handler.get_cronjob_spec(&self.job_name).await?;

        // Get the environment variable from the job spec
        let Some(job_spec) = job_tmpl_spec.spec else {
            return Err(anyhow!("Unable to get the job template spec"));
        };

        let envs = job_spec.get_env()?;

        Ok(())
    }
}
