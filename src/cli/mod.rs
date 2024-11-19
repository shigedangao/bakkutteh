use crate::kube::spec::{ContainerEnv, EnvKind, SpecHandler};
use crate::kube::KubeHandler;
use anyhow::{anyhow, Result};
use clap::Parser;
use colored::Colorize;
use inquire::Text;

#[derive(Parser)]
#[command(
    version = "0.0.1",
    about = "A command to dispatch a kubernetes job from a cronjob spec"
)]
pub struct Cli {
    #[arg(short, long)]
    job_name: String,

    #[arg(short, long)]
    target_name: Option<String>,

    #[arg(short, long, default_value = "false")]
    dry_run: bool,

    #[arg(short, long, default_value = "default")]
    pub namespace: String,

    #[arg(short, long, default_value = "3")]
    pub backoff_limit: i32,
}

impl Cli {
    pub async fn run<S: AsRef<str>>(&self, kube_handler: &mut KubeHandler<S>) -> Result<()> {
        // Get the targeted cronjob
        let job_tmpl_spec = kube_handler.get_cronjob_spec(&self.job_name).await?;

        // Get the environment variable from the job spec
        let Some(mut job_spec) = job_tmpl_spec.spec else {
            return Err(anyhow!("Unable to get the job template spec"));
        };

        let mut envs = job_spec.get_env()?;

        // Show the user the environment variable and let the user confirm the value to output
        self.prompt_user_env(&mut envs);

        // Rebuild the job spec with the updated environment variables
        job_spec.rebuild_env(envs)?;

        let name = match &self.target_name {
            Some(name) => name,
            None => &self.job_name,
        };

        kube_handler
            .build_manual_job(name, job_spec, self.backoff_limit)?
            .apply_manual_job(self.dry_run)
            .await?;

        Ok(())
    }

    fn prompt_user_env(&self, envs: &mut Vec<ContainerEnv>) {
        for container in envs {
            for (name, kind) in &mut container.envs {
                if let EnvKind::Literal(literal) = kind {
                    if let Ok(res) = Text::new(&format!("Env for {}: ", name.bright_cyan()))
                        .with_default(literal)
                        .prompt()
                    {
                        *kind = EnvKind::Literal(res);
                    }
                }
            }
        }
    }
}
