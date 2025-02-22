use super::TemplateSpecOps;
use k8s_openapi::api::batch::v1::JobTemplateSpec;
use k8s_openapi::api::{apps::v1::Deployment, batch::v1::JobSpec};

impl TemplateSpecOps for Deployment {
    fn get_template_spec(self) -> Option<JobTemplateSpec> {
        self.spec.map(|mut dep| {
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
    }
}
