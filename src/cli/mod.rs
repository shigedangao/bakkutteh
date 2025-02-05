use crate::kube::spec::{ContainerEnv, EnvKind, SpecHandler};
use crate::kube::KubeHandler;
use anyhow::{anyhow, Result};
use clap::Parser;
use colored::Colorize;
use inquire::{Confirm, Select, Text};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::batch::v1::CronJob;

// Constant
const SPLIT_ENV_OPERATOR: &str = "=";

#[derive(Parser)]
#[command(
    version = "0.1.6",
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
    pub dry_run: bool,

    #[arg(short, long, default_value = "default")]
    pub namespace: String,

    #[arg(short, long, default_value = "3")]
    pub backoff_limit: i32,

    #[arg(
        long,
        default_value = "false",
        help = "Enable the option to use a deployment spec to create a manual job"
    )]
    pub deployment: bool,
}

impl Cli {
    pub async fn run<S: AsRef<str>>(&self, kube_handler: &mut KubeHandler<S>) -> Result<()> {
        let name = match &self.job_name {
            Some(name) => name.to_owned(),
            None => {
                let list = match self.deployment {
                    true => kube_handler.list::<Deployment>().await?,
                    false => kube_handler.list::<CronJob>().await?,
                };

                self.prompt_user_list_selection(list)?
            }
        };

        let job_tmpl_spec = match self.deployment {
            true => kube_handler.get_deployment_spec(name).await?,
            false => kube_handler.get_cronjob_spec(name).await?,
        };

        // Get the environment variable from the job spec
        let Some(mut job_spec) = job_tmpl_spec.spec else {
            return Err(anyhow!("Unable to get the job template spec"));
        };

        let mut envs = job_spec.get_env()?;

        // Show the user the environment variable and let the user confirm the value to output
        self.prompt_user_env(&mut envs)?;

        if self.ask_user_additional_env("Do you want to add additional env ?")? {
            self.process_prompt_additional_env(&mut envs)?;
        }

        // Rebuild the job spec with the updated environment variables
        job_spec.rebuild_env(&mut envs)?;

        kube_handler
            .build_manual_job(&self.target_name, job_spec, self.backoff_limit)?
            .apply_manual_job()
            .await
            .and_then(|job| kube_handler.display_spec(job))?;

        Ok(())
    }

    fn prompt_user_env(&self, envs: &mut Vec<ContainerEnv>) -> Result<()> {
        for container in envs {
            for (name, kind) in &mut container.envs {
                if let EnvKind::Literal(literal) = kind {
                    match Text::new(&format!("Env for {}: ", name.bright_cyan()))
                        .with_default(literal)
                        .prompt()
                    {
                        Ok(res) => *kind = EnvKind::Literal(res),
                        Err(err) => return Err(anyhow!("Operation canceled: {:?}", err)),
                    }
                }
            }
        }

        Ok(())
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

    fn ask_user_additional_env(&self, msg: &str) -> Result<bool> {
        let adds_env_prompt = Confirm::new(msg).with_default(false).prompt()?;

        Ok(adds_env_prompt)
    }

    fn process_prompt_additional_env(&self, envs: &mut [ContainerEnv]) -> Result<()> {
        let mut ask_user_additional_env = true;

        // Select the container which will be used to add the additional environment variables
        let containers_name = envs.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        let answer = Select::new(
            "Select the container to add the additional environment variable",
            containers_name,
        )
        .prompt()?;

        let tgt_container = envs
            .iter_mut()
            .filter(|c| c.name == answer)
            .last()
            .ok_or_else(|| anyhow!("Unable to found the targeted container"))?;

        while ask_user_additional_env {
            if let Ok(res) = Text::new("Input the additional env separate with a =").prompt() {
                let properties = res.split(SPLIT_ENV_OPERATOR).collect::<Vec<_>>();
                if properties.len() != 2 {
                    return Err(anyhow!("Expect to have an environment variable formatted like specified: ENV_NAME=VALUE"));
                }

                let (key, value) = (
                    properties.first().expect("Expect key to be defined"),
                    properties.last().expect("Expect value to be defined"),
                );

                // Push env to the containers envs
                tgt_container
                    .envs
                    .insert(key.to_string(), EnvKind::Literal(value.to_string()));

                // Asking to the user whether it wants to add additional env
                if !self.ask_user_additional_env("Do you still want to add additional env ?")? {
                    ask_user_additional_env = false;
                }
            };
        }

        Ok(())
    }
}
