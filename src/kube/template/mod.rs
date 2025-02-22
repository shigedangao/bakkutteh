use k8s_openapi::api::batch::v1::JobTemplateSpec;

pub mod cronjob;
pub mod deployment;

pub trait TemplateSpecOps {
    fn get_template_spec(self) -> Option<JobTemplateSpec>;
}
