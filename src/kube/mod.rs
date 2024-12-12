use anyhow::{anyhow, Result};
use colored::{self, Colorize};
use k8s_openapi::api::batch::v1::{CronJob, Job, JobSpec, JobTemplateSpec};
use kube::{
    api::{Api, ListParams, PostParams},
    Client,
};
use serde_json::json;

pub(crate) mod spec;

#[derive(Clone)]
pub struct KubeHandler<S: AsRef<str>> {
    client: Client,
    namespace: S,
    job: Option<Job>,
}

impl<S> KubeHandler<S>
where
    S: AsRef<str>,
{
    /// Create a new instance of the KubeHandler
    ///
    /// # Arguments
    ///
    /// * `ns` - S
    pub async fn new(ns: S) -> Result<Self> {
        let client = Client::try_default().await?;

        Ok(Self {
            client,
            namespace: ns,
            job: None,
        })
    }

    /// Get a cronjob spec for the targeted cronjob name
    ///
    /// # Arguments
    ///
    /// * `name` - N
    pub async fn get_cronjob_spec<N: AsRef<str>>(&self, name: N) -> Result<JobTemplateSpec> {
        println!(
            "Getting cronjob {} from namespace {}",
            name.as_ref().truecolor(7, 174, 237).bold(),
            self.namespace.as_ref().truecolor(133, 59, 255).bold()
        );

        let cronjobs: Api<CronJob> = Api::namespaced(self.client.clone(), self.namespace.as_ref());
        let targeted_cronjob = cronjobs.get(name.as_ref()).await?;

        // Get the spec of the cronjob
        let spec = targeted_cronjob
            .spec
            .map(|j| j.job_template)
            .ok_or_else(|| {
                anyhow!(
                    "Unable to found the cronjob spec for the targeted pod name {:?}",
                    name.as_ref()
                )
            })?;

        Ok(spec)
    }

    /// List cronjob available in the selected namespace
    pub async fn list_cronjob(&self) -> Result<Vec<String>> {
        let cronjobs: Api<CronJob> = Api::namespaced(self.client.clone(), self.namespace.as_ref());

        let lp = ListParams::default();
        let list = cronjobs.list(&lp).await?;

        let cronjob_list = list
            .items
            .into_iter()
            .filter_map(|item| item.metadata.name)
            .collect::<Vec<_>>();

        Ok(cronjob_list)
    }

    /// Build a manual job from the cronjob job spec
    ///
    /// # Arguments
    ///
    /// * `name` - N
    /// * `job_spec` - JobSpec
    /// * `backoff_limit` - BackoffLimit for the job
    pub fn build_manual_job<N: AsRef<str>>(
        &mut self,
        name: N,
        mut job_spec: JobSpec,
        backoff_limit: i32,
    ) -> Result<&Self> {
        let mut job: Job = serde_json::from_value(json!({
            "apiVersion": "batch/v1",
            "kind": "Job",
            "metadata": {
                "name": format!("{}-manual", name.as_ref())
            },
            "spec": {}
        }))?;

        job_spec.backoff_limit = Some(backoff_limit);
        job.spec = Some(job_spec);

        self.job = Some(job);

        Ok(self)
    }

    /// Apply the manual job in K8S
    ///
    /// # Arguments
    ///
    /// * `dry_run` - bool
    pub async fn apply_manual_job(&self, dry_run: bool) -> Result<()> {
        let job_api: Api<Job> = Api::namespaced(self.client.clone(), self.namespace.as_ref());
        let mut pp = PostParams::default();

        if dry_run {
            pp.dry_run = true;
        }

        let Some(job) = &self.job else {
            return Err(anyhow!("Unable to create the job as building spec failed"));
        };

        match job_api.create(&pp, job).await {
            Ok(res) => match dry_run {
                true => {
                    let yaml = serde_yml::to_string(&res)?;
                    println!(
                        "\nDry run result for job {}",
                        res.metadata.name.unwrap_or_default().bright_purple().bold()
                    );

                    println!("\n{}", yaml)
                }
                false => println!(
                    "Job {} created",
                    res.metadata
                        .name
                        .unwrap_or_default()
                        .truecolor(7, 174, 237)
                        .bold()
                ),
            },
            Err(err) => println!(
                "Unable to create job due to error: {}",
                err.to_string().red().bold()
            ),
        };

        Ok(())
    }
}
