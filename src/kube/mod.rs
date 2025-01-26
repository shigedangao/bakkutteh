use std::fmt::Debug;

use anyhow::{anyhow, Result};
use colored::{self, Colorize};
use k8s_openapi::{
    api::{
        apps::v1::Deployment,
        batch::v1::{CronJob, Job, JobSpec, JobTemplateSpec},
    },
    serde::de::DeserializeOwned,
    NamespaceResourceScope,
};
use kube::{
    api::{Api, ListParams, PostParams},
    Client, Resource,
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

    /// Get a deployment job template spec from a given name
    ///
    /// # Arguments
    ///
    /// * `name` - N
    pub async fn get_deployment_spec<N: AsRef<str>>(&self, name: N) -> Result<JobTemplateSpec> {
        let deps_api: Api<Deployment> =
            Api::namespaced(self.client.clone(), self.namespace.as_ref());
        let dep = deps_api.get(name.as_ref()).await?;

        let spec = dep
            .spec
            .map(|mut dep| {
                // Update the spec restart policy
                if let Some(spec) = dep.template.spec.as_mut() {
                    spec.restart_policy = Some("Never".to_string());
                }

                JobTemplateSpec {
                    metadata: dep.template.metadata.clone(),
                    spec: Some(JobSpec {
                        template: dep.template,
                        ..Default::default()
                    }),
                }
            })
            .ok_or_else(|| {
                anyhow!(
                    "Unable to found the pod spec for the targeted deployment {:?}",
                    name.as_ref()
                )
            })?;

        Ok(spec)
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

    /// List the existing resources on the cluster
    pub async fn list<K>(&self) -> Result<Vec<String>>
    where
        K: Resource<Scope = NamespaceResourceScope>,
        K: Resource + Clone + Debug + DeserializeOwned,
        <K as Resource>::DynamicType: Default,
    {
        let cronjobs: Api<K> = Api::namespaced(self.client.clone(), self.namespace.as_ref());

        let lp = ListParams::default();
        let list = cronjobs.list(&lp).await?;

        let cronjob_list = list
            .items
            .into_iter()
            .filter_map(|item| item.meta().name.clone())
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
