use crate::cli::ui::SpinnerWrapper;
use crate::kube::KubeHandler;
use crate::kube::spec::{ContainerEnv, EnvKind, SpecHandler, SpecResources};
use anyhow::{Result, anyhow};
use clap::Parser;
use colored::Colorize;
use inquire::validator::Validation;
use jiff::Span;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use std::fs;
use std::path::PathBuf;

pub mod ui;

// Constant
const SPLIT_ENV_OPERATOR: &str = "=";
// See definition of the SI here
// @link https://docs.rs/k8s-openapi/latest/k8s_openapi/apimachinery/pkg/api/resource/struct.Quantity.html
const DECIMAL_SI: [&str; 6] = ["Ki", "Mi", "Gi", "Ti", "Pi", "Ei"];
// CPU definition is either None (no format) or m (millis)
const CPU: [&str; 2] = ["None", "m"];
// Used to replace environment variable which already has a quote or single quote
const REPLACE_STR: [char; 2] = ['\"', '\''];
// Color code for the Clack purple theme on colorized side.
pub(crate) const COLOR: (u8, u8, u8) = (180, 140, 247);

#[derive(Parser)]
#[command(
    version = "0.2.8",
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
    target_name: Option<String>,

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

    #[arg(long, help = "Wait for the job to complete before exiting")]
    pub wait: Option<Span>,
}

impl Cli {
    pub async fn run<S: AsRef<str>>(&self, kube_handler: &mut KubeHandler<S>) -> Result<()> {
        if self.dry_run && self.wait.is_some() {
            return Err(anyhow!("Cannot use --wait with --dry-run"));
        }

        let name = match &self.job_name {
            Some(name) => name.to_owned(),
            None => {
                // Show a spinner while getting the list of jobs
                let mut spinner = SpinnerWrapper::new("Getting list of jobs...");

                let list = match self.deployment {
                    true => kube_handler.list::<Deployment>().await?,
                    false => kube_handler.list::<CronJob>().await?,
                };

                // Stop the spinner after getting the list
                spinner.stop();

                ui::select(
                    "Select the cronjob that you want to use as a base of the job".to_string(),
                    list,
                )?
            }
        };

        // Check if the targeted name already exist in the cluster
        let target_job_name = match &self.target_name {
            Some(name) => format!("{}-manual", name),
            None => {
                println!("Will use the name of the target job to create the job");
                format!("{}-manual", name)
            }
        };

        if kube_handler
            .get_object::<Job, _>(&target_job_name)
            .await
            .is_ok()
        {
            match ui::confirm(
                "An job with the same name already exist. Do you want to delete this job",
                false,
            )? {
                true => kube_handler.delete_object(&target_job_name).await?,
                false => {
                    return Err(anyhow!(
                        "Job with the same name already exist in the cluster"
                    ));
                }
            }
        }

        // Get the job details and stop the spinner if it exists
        let mut object_spinner = SpinnerWrapper::new("Getting object details...");

        let job_tmpl_spec = match self.deployment {
            true => {
                kube_handler
                    .get_spec_for_object::<_, Deployment>(name)
                    .await?
            }
            false => kube_handler.get_spec_for_object::<_, CronJob>(name).await?,
        };

        // Stop the spinner after getting the job details
        object_spinner.stop();

        // Get the environment variable from the job spec
        let Some(mut job_spec) = job_tmpl_spec.spec else {
            return Err(anyhow!("Unable to get the job template spec"));
        };

        let mut envs = job_spec.get_env()?;

        // Show the user the environment variable and let the user confirm the value to output
        self.prompt_user_env(&mut envs)?;

        if ui::confirm("Do you want to add additional env ?", false)? {
            self.process_prompt_additional_env(&mut envs)?;
        }

        // Rebuild the job spec with the updated environment variables
        job_spec.rebuild_env(&mut envs)?;

        // Upgrade the resources limits if needed
        if ui::confirm("Do you want to update the resources limits ?", false)? {
            let user_asked_resources = self.process_resources_prompt(&envs)?;
            job_spec.update_resources(user_asked_resources)?;
        }

        // Apply the job spec and display the output
        let mut apply_spinner = match self.dry_run {
            true => SpinnerWrapper::new("Running a dry-run job..."),
            false => SpinnerWrapper::new("Applying job..."),
        };

        let job = kube_handler
            .build_manual_job(&target_job_name, job_spec, self.backoff_limit)?
            .apply_manual_job()
            .await?;

        let output = kube_handler
            .wait_for_job(job, self.wait)
            .await
            .and_then(|job| {
                // stop the spinner before displaying the output
                apply_spinner.stop();

                kube_handler.display_spec(job)
            })
            .inspect_err(|_| {
                // stop the spinner before returning an error
                apply_spinner.stop();
            })?;

        if let (Some(output_path), Some(contents)) = (&self.dry_run_output_path, output) {
            fs::write(PathBuf::from(output_path), contents)?;
        }

        Ok(())
    }

    // Prompt the user to add additional environment variables to the containers
    fn prompt_user_env(&self, envs: &mut Vec<ContainerEnv>) -> Result<()> {
        for container in envs {
            for (name, kind) in &mut container.envs {
                if let EnvKind::Literal(literal) = kind {
                    let new_value = ui::text(
                        &format!("Env for {}: ", name.truecolor(COLOR.0, COLOR.1, COLOR.2)),
                        Some(literal),
                    )?;
                    *kind = EnvKind::Literal(new_value);
                }
            }
        }

        Ok(())
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
        let answer = ui::select(
            "Select the container to add the additional environment variable".to_string(),
            containers_name,
        )?;

        let tgt_container = envs
            .iter_mut()
            .rfind(|c| c.name == answer)
            .ok_or_else(|| anyhow!("Unable to found the targeted container"))?;

        while ask_user_additional_env {
            if let Ok(res) =
                ui::text_with_validator("Input the additional env separate with a =", |s: &str| {
                    let v = s.split(SPLIT_ENV_OPERATOR).collect::<Vec<_>>();
                    match v.len() != 2 {
                        true => Ok(Validation::Invalid(
                            "Environment variable should respect the format: ENV_NAME=VALUE".into(),
                        )),
                        false => Ok(Validation::Valid),
                    }
                })
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
                if !ui::confirm("Do you still want to add additional env ?", false)? {
                    ask_user_additional_env = false;
                }
            };
        }

        Ok(())
    }

    /// Ask desired resources to the user for the targeted container. The envs is only used to get the name list of the containers
    ///
    /// * `envs` - &[ContainerEnv]
    fn process_resources_prompt(&self, envs: &[ContainerEnv]) -> Result<SpecResources> {
        let containers_name = envs.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        let container = ui::select(
            "Select the container to add the additional environment variable".to_string(),
            containers_name,
        )?;

        // Memory
        let memory = ui::text_with_validator("Set the memory limits", |s: &str| {
            match s.parse::<f64>().is_ok() {
                true => Ok(Validation::Valid),
                false => Ok(Validation::Invalid(
                    "Memory should contains only numbers".into(),
                )),
            }
        })?;
        let memory_format = ui::select("Select a memory format", DECIMAL_SI.to_vec())?;

        // Cpu
        let cpu =
            ui::text_with_validator("Set the cpu limits", |s: &str| match s.parse::<f64>() {
                Ok(v) => {
                    if v < 0.001 {
                        return Ok(Validation::Invalid(
                            "CPU should be greater >= to 0.001".into(),
                        ));
                    }

                    Ok(Validation::Valid)
                }
                Err(_) => Ok(Validation::Invalid("CPU should contains numbers".into())),
            })?;

        let cpu_format =
            ui::select("Select a cpu format", CPU.to_vec()).map(|format| match format {
                "None" => "",
                _ => format,
            })?;

        Ok(SpecResources {
            memory: Quantity(format!("{memory}{memory_format}")),
            cpu: Quantity(format!("{cpu}{cpu_format}")),
            container_name: container,
        })
    }
}
