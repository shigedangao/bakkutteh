use crate::kube::KubeHandler;
use crate::kube::spec::{ContainerEnv, EnvKind, SpecHandler, SpecResources};
use anyhow::{Result, anyhow};
use clap::Parser;
use colored::Colorize;
use inquire::validator::Validation;
use inquire::{Confirm, Select, Text};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use std::fs;
use std::path::PathBuf;

// Constant
const SPLIT_ENV_OPERATOR: &str = "=";
// See definition of the SI here
// @link https://docs.rs/k8s-openapi/latest/k8s_openapi/apimachinery/pkg/api/resource/struct.Quantity.html
const DECIMAL_SI: [&str; 6] = ["Ki", "Mi", "Gi", "Ti", "Pi", "Ei"];
// Used to replace environment variable which already has a quote or single quote
const REPLACE_STR: [char; 2] = ['\"', '\''];

#[derive(Parser)]
#[command(
    version = "0.2.3",
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

    #[arg(
        long,
        help = "Output path of the spec when the user specified to use the --dry-run option"
    )]
    pub dry_run_output_path: Option<String>,
}

impl Cli {
    pub async fn run<S: AsRef<str>>(&self, kube_handler: &mut KubeHandler<S>) -> Result<()> {
        // Check if the targeted name already exist in the cluster
        let target_job_name = format!("{}-manual", self.target_name);
        if kube_handler
            .get_object::<Job, _>(&target_job_name)
            .await
            .is_ok()
        {
            match self.ask_user_prompt(
                "An job with the same name already exist. Do you want to delete this job",
            )? {
                true => kube_handler.delete_object(&target_job_name).await?,
                false => {
                    return Err(anyhow!(
                        "Job with the same name already exist in the cluster"
                    ));
                }
            }
        }

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
            true => {
                kube_handler
                    .get_spec_for_object::<_, Deployment>(name)
                    .await?
            }
            false => kube_handler.get_spec_for_object::<_, CronJob>(name).await?,
        };

        // Get the environment variable from the job spec
        let Some(mut job_spec) = job_tmpl_spec.spec else {
            return Err(anyhow!("Unable to get the job template spec"));
        };

        let mut envs = job_spec.get_env()?;

        // Show the user the environment variable and let the user confirm the value to output
        self.prompt_user_env(&mut envs)?;

        if self.ask_user_prompt("Do you want to add additional env ?")? {
            self.process_prompt_additional_env(&mut envs)?;
        }

        // Rebuild the job spec with the updated environment variables
        job_spec.rebuild_env(&mut envs)?;

        // Upgrade the resources limits if needed
        if self.ask_user_prompt("Do you want to update the resources limits ?")? {
            let user_asked_resources = self.process_resources_prompt(&envs)?;
            job_spec.update_resources(user_asked_resources)?;
        }

        let output = kube_handler
            .build_manual_job(&self.target_name, job_spec, self.backoff_limit)?
            .apply_manual_job()
            .await
            .and_then(|job| kube_handler.display_spec(job))?;

        if let (Some(output_path), Some(contents)) = (&self.dry_run_output_path, output) {
            fs::write(PathBuf::from(output_path), contents)?;
        }

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

    fn ask_user_prompt(&self, msg: &str) -> Result<bool> {
        let res = Confirm::new(msg).with_default(false).prompt()?;

        Ok(res)
    }

    /// Add additional environment variables to the list of existing environment variables present in the envs slice
    ///
    /// # Arguments
    ///
    /// * `envs` - &mut [Containers]
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
            .next_back()
            .ok_or_else(|| anyhow!("Unable to found the targeted container"))?;

        while ask_user_additional_env {
            if let Ok(res) = Text::new("Input the additional env separate with a =")
                .with_validator(|s: &str| {
                    let v = s.split(SPLIT_ENV_OPERATOR).collect::<Vec<_>>();
                    if v.len() != 2 {
                        return Ok(Validation::Invalid(
                            "Environment variable should respect the format: ENV_NAME=VALUE".into(),
                        ));
                    }

                    Ok(Validation::Valid)
                })
                .prompt()
            {
                let properties = res.split(SPLIT_ENV_OPERATOR).collect::<Vec<_>>();
                let (key, value) = (
                    properties
                        .first()
                        .ok_or_else(|| anyhow!("Expect to retrieve the key of the env"))?,
                    properties
                        .last()
                        .ok_or_else(|| anyhow!("Expect to retrieve the value of the env"))?,
                );

                // Push env to the containers envs
                tgt_container.envs.insert(
                    key.to_string(),
                    EnvKind::Literal(value.to_string().replace(REPLACE_STR, "")),
                );

                // Asking to the user whether it wants to add additional env
                if !self.ask_user_prompt("Do you still want to add additional env ?")? {
                    ask_user_additional_env = false;
                }
            };
        }

        Ok(())
    }

    /// Ask desired resources to the user for the targeted container. The envs is only used to get the name list of the containers
    ///
    /// * `envs` - &[ContainerEnv]
    fn process_resources_prompt(&self, envs: &[ContainerEnv]) -> Result<(SpecResources, String)> {
        let containers_name = envs.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        let container = Select::new(
            "Select the container to add the additional environment variable",
            containers_name,
        )
        .prompt()?;

        // Memory
        let memory = Text::new("Set the memory limits")
            .with_validator(|s: &str| match s.parse::<f64>().is_ok() {
                true => Ok(Validation::Valid),
                false => Ok(Validation::Invalid(
                    "Memory should contains only numbers".into(),
                )),
            })
            .prompt()?;
        let memory_format = Select::new("Select a memory format", DECIMAL_SI.to_vec()).prompt()?;

        // Cpu
        let cpu = Text::new("Set the cpu limits")
            .with_validator(|s: &str| match s.parse::<f64>().is_ok() {
                true => Ok(Validation::Valid),
                false => Ok(Validation::Invalid(
                    "CPU should contains only numbers".into(),
                )),
            })
            .prompt()?;

        Ok((
            SpecResources {
                memory: Quantity(format!("{memory}{memory_format}")),
                cpu: Quantity(cpu),
            },
            container,
        ))
    }
}
