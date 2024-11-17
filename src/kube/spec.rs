use anyhow::{anyhow, Result};
use k8s_openapi::api::{batch::v1::JobSpec, core::v1::EnvVarSource};
use std::collections::HashMap;

pub enum EnvKind {
    Literal(String),
    ConfigMap(EnvVarSource),
}

#[derive(Default)]
pub struct ContainerEnv {
    name: String,
    envs: HashMap<String, EnvKind>,
}

pub trait SpecHandler {
    fn get_env(&self) -> Result<Vec<ContainerEnv>>;
}

impl SpecHandler for JobSpec {
    fn get_env(&self) -> Result<Vec<ContainerEnv>> {
        let pod_spec = self
            .template
            .spec
            .as_ref()
            .ok_or_else(|| anyhow!("Unable to found pod spec on job"))?;

        let mut containers_env = Vec::new();

        for container in pod_spec.containers.iter() {
            let mut cont_env = ContainerEnv {
                name: container.name.to_owned(),
                ..Default::default()
            };

            if let Some(env) = &container.env {
                let envs: HashMap<String, EnvKind> = env
                    .into_iter()
                    .filter_map(|e| {
                        let name = e.name.to_owned();
                        if let Some(literal) = e.value.to_owned() {
                            return Some((name, EnvKind::Literal(literal)));
                        }

                        if let Some(c) = e.value_from.to_owned() {
                            return Some((name, EnvKind::ConfigMap(c)));
                        }

                        None
                    })
                    .collect();

                cont_env.envs = envs;
            }

            containers_env.push(cont_env);
        }

        Ok(containers_env)
    }
}
