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
use std::fmt::Debug;

pub(crate) mod spec;

// Constant
const BATCH_UID_REMOVE: &str = "batch.kubernetes.io/controller-uid";
const UID_REMOVE: &str = "controller-uid";

#[derive(Clone)]
pub struct KubeHandler<S: AsRef<str>> {
    client: Client,
    namespace: S,
    job: Option<Job>,
    dry_run: bool,
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
    pub async fn new(ns: S, dry_run: bool) -> Result<Self> {
        let client = Client::try_default().await?;

        Ok(Self {
            client,
            namespace: ns,
            job: None,
            dry_run,
        })
    }

    /// Get the object for the targeted api
    ///
    /// # Arguments
    ///
    /// * `name` - N
    async fn get_object<K, N>(&self, name: N) -> Result<K>
    where
        K: Resource<Scope = NamespaceResourceScope>,
        K: Resource + Clone + Debug + DeserializeOwned,
        <K as Resource>::DynamicType: Default,
        N: AsRef<str>,
    {
        let api: Api<K> = Api::namespaced(self.client.clone(), self.namespace.as_ref());
        let object = api.get(name.as_ref()).await?;

        Ok(object)
    }

    /// Get a deployment job template spec from a given name
    ///
    /// # Arguments
    ///
    /// * `name` - N
    pub async fn get_deployment_spec<N: AsRef<str>>(&self, name: N) -> Result<JobTemplateSpec> {
        let dep: Deployment = self.get_object(name.as_ref()).await?;

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

        let targeted_cronjob: CronJob = self.get_object(name.as_ref()).await?;

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
        let target_object: Api<K> = Api::namespaced(self.client.clone(), self.namespace.as_ref());

        let lp = ListParams::default();
        let list = target_object.list(&lp).await?;

        let list = list
            .items
            .into_iter()
            .filter_map(|item| item.meta().name.clone())
            .collect::<Vec<_>>();

        Ok(list)
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
    pub async fn apply_manual_job(&self) -> Result<Job> {
        let job_api: Api<Job> = Api::namespaced(self.client.clone(), self.namespace.as_ref());
        let mut pp = PostParams::default();

        if self.dry_run {
            pp.dry_run = true;
        }

        let Some(job) = &self.job else {
            return Err(anyhow!("Unable to create the job as building spec failed"));
        };

        let job = job_api.create(&pp, job).await?;

        Ok(job)
    }

    /// Display the spec in the case if the user asked for a dry run
    ///
    /// # Arguments
    ///
    /// * `job` - Job
    pub fn display_spec(&self, mut job: Job) -> Result<()> {
        if !self.dry_run {
            println!(
                "Job {} created",
                job.metadata
                    .name
                    .unwrap_or_default()
                    .truecolor(7, 174, 237)
                    .bold()
            );

            return Ok(());
        }

        // Remove presence of managed fields from the job
        job.metadata.managed_fields = None;
        // Remove presence of labels containing "controler-uid" in the metadata & template
        if let Some(fields) = job.metadata.labels.as_mut() {
            fields.remove(BATCH_UID_REMOVE);
            fields.remove(UID_REMOVE);
        }

        if let Some(labels) = job
            .spec
            .as_mut()
            .and_then(|spec| spec.template.metadata.as_mut())
            .and_then(|tmpl| tmpl.labels.as_mut())
        {
            labels.remove(UID_REMOVE);
            labels.remove(BATCH_UID_REMOVE);
        }

        job.spec
            .as_mut()
            .and_then(|spec| spec.selector.as_mut())
            .and_then(|selector| selector.match_labels.as_mut())
            .map(|selector| selector.remove(BATCH_UID_REMOVE));

        let yaml = serde_yml::to_string(&job)?;
        println!(
            "\nDry run result for job {}",
            job.metadata.name.unwrap_or_default().bright_purple().bold()
        );

        println!("\n{}", yaml);

        Ok(())
    }
}
