use super::TemplateSpecOps;
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::batch::v1::JobTemplateSpec;

impl TemplateSpecOps for CronJob {
    fn get_template_spec(&self) -> Option<JobTemplateSpec> {
        self.spec.as_ref().map(|j| j.job_template.clone())
    }
}
