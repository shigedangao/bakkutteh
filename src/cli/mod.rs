use crate::kube::spec::{ContainerEnv, EnvKind, SpecHandler};
use crate::kube::KubeHandler;
use anyhow::{anyhow, Result};
use clap::Parser;
use colored::Colorize;
use inquire::{Select, Text};

#[derive(Parser)]
#[command(
    version = "0.1.0",
    about = "A command to dispatch a kubernetes job from a cronjob spec"
)]
pub struct Cli {
    #[arg(
        short,
        long,
        help = "The cronjob name that will be used as the source of the job"
    )]
    job_name: Option<String>,

    #[arg(short, long, help = "The name of the job that will be create")]
    target_name: String,

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
        let job_tmpl_spec = match &self.job_name {
            Some(name) => kube_handler.get_cronjob_spec(name).await?,
            None => {
                let name = kube_handler
                    .list_cronjob()
                    .await
                    .map(|list| self.prompt_user_list_selection(list))??;

                kube_handler.get_cronjob_spec(name).await?
            }
        };

        // Get the environment variable from the job spec
        let Some(mut job_spec) = job_tmpl_spec.spec else {
            return Err(anyhow!("Unable to get the job template spec"));
        };

        let mut envs = job_spec.get_env()?;

        // Show the user the environment variable and let the user confirm the value to output
        self.prompt_user_env(&mut envs);

        // Rebuild the job spec with the updated environment variables
        job_spec.rebuild_env(envs)?;

        kube_handler
            .build_manual_job(&self.target_name, job_spec, self.backoff_limit)?
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

    fn prompt_user_list_selection(&self, list: Vec<String>) -> Result<String> {
        let selected = Select::new(
            "Select the cronjob that you want to use as a base of the job",
            list,
        )
        .prompt()
        .map_err(|_| anyhow!("An error occurred. Please try again"))?;

        Ok(selected)
    }
}
