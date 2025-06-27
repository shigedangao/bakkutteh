use anyhow::{Result, anyhow};
use colored::{self, Colorize};
use k8s_openapi::{
    NamespaceResourceScope,
    api::batch::v1::{Job, JobSpec, JobTemplateSpec},
    serde::de::DeserializeOwned,
};
use kube::{
    Client, Resource,
    api::{Api, DeleteParams, ListParams, PostParams},
};
use serde_json::json;
use std::fmt::Debug;
use template::TemplateSpecOps;

pub(crate) mod spec;
pub(crate) mod template;

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
    pub async fn get_object<K, N>(&self, name: N) -> Result<K>
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

    /// Delete object
    ///
    /// # Arguments
    ///
    /// * `name` - N
    pub async fn delete_object<N>(&self, name: N) -> Result<()>
    where
        N: AsRef<str>,
    {
        let api: Api<Job> = Api::namespaced(self.client.clone(), self.namespace.as_ref());
        let delete_params = DeleteParams::default();

        api.delete(name.as_ref(), &delete_params)
            .await
            .map_err(|err| anyhow!("Unable to delete the job due to {:?}", err))?
            .map_right(|s| println!("Job deleted with status {s:?}"));

        Ok(())
    }

    /// Get the spec for a targeted kubernetes object
    ///
    /// # Arguments
    ///
    /// * `name` - N
    pub async fn get_spec_for_object<N, K>(&self, name: N) -> Result<JobTemplateSpec>
    where
        N: AsRef<str>,
        K: Resource<Scope = NamespaceResourceScope>,
        K: Resource + Clone + Debug + DeserializeOwned + TemplateSpecOps,
        <K as Resource>::DynamicType: Default,
    {
        let object: K = self.get_object(name.as_ref()).await?;
        object
            .get_template_spec()
            .ok_or_else(|| anyhow!("Unable to get the template spec for {}", name.as_ref()))
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

        println!("\n{yaml}");

        Ok(())
    }
}
