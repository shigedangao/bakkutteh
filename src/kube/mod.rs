use anyhow::{anyhow, Result};
use k8s_openapi::api::batch::v1::{CronJob, Job, JobSpec, JobTemplateSpec};
use kube::{
    api::{Api, PostParams},
    Client,
};
use serde_json::json;

pub(crate) mod spec;

#[derive(Clone)]
pub struct KubeHandler<S: AsRef<str>> {
    client: Client,
    namespace: S,
}

impl<S> KubeHandler<S>
where
    S: AsRef<str>,
{
    pub async fn new(ns: S) -> Result<Self> {
        let client = Client::try_default().await?;

        Ok(Self {
            client,
            namespace: ns,
        })
    }

    pub async fn get_cronjob_spec<N: AsRef<str>>(&self, name: N) -> Result<JobTemplateSpec> {
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

    pub async fn build_manual_job<N: AsRef<str>>(
        &self,
        name: N,
        job_spec: JobSpec,
        backoff_limit: usize,
    ) -> Result<()> {
        let job_api: Api<Job> = Api::namespaced(self.client.clone(), self.namespace.as_ref());

        let mut job: Job = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Job",
            "metadata": {
                "name": format!("{}-manual", name.as_ref())
            },
            "spec": {},
            "backoffLimit": backoff_limit
        }))?;

        job.spec = Some(job_spec);

        let pp = PostParams::default();
        if let Ok(res) = job_api.create(&pp, &job).await {
            println!("Job {:?} created", res.metadata.name)
        };

        Ok(())
    }
}
