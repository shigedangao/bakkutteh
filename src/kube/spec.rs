use anyhow::{anyhow, Result};
use k8s_openapi::api::{batch::v1::JobSpec, core::v1::EnvVarSource};
use std::{collections::HashMap, ops::Deref};

pub enum EnvKind {
    Literal(String),
    ConfigMap(Box<EnvVarSource>),
}

#[derive(Default)]
pub struct ContainerEnv {
    pub name: String,
    pub envs: HashMap<String, EnvKind>,
}

pub trait SpecHandler {
    fn get_env(&self) -> Result<Vec<ContainerEnv>>;
    fn rebuild_env(&mut self, envs: Vec<ContainerEnv>) -> Result<()>;
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
                    .iter()
                    .filter_map(|e| {
                        let name = e.name.to_owned();
                        if let Some(literal) = e.value.to_owned() {
                            return Some((name, EnvKind::Literal(literal)));
                        }

                        if let Some(c) = e.value_from.to_owned() {
                            return Some((name, EnvKind::ConfigMap(Box::new(c))));
                        }

                        None
                    })
                    .collect();

                cont_env.envs = envs;

                containers_env.push(cont_env);
            }
        }

        Ok(containers_env)
    }

    fn rebuild_env(&mut self, envs: Vec<ContainerEnv>) -> Result<()> {
        // If no env is to be found then there's no need to rebuild the environment variable
        if envs.is_empty() {
            return Ok(());
        }

        let pod_spec = self
            .template
            .spec
            .as_mut()
            .ok_or_else(|| anyhow!("Unable to found pod spec on job"))?;

        for (idx, container) in pod_spec.containers.iter_mut().enumerate() {
            let updated_env =
                match envs
                    .get(idx)
                    .and_then(|cont| match cont.name == container.name {
                        true => Some(cont),
                        false => None,
                    }) {
                    Some(updated_env) => updated_env,
                    None => {
                        return Err(anyhow!(
                            "Unable to get the environment variable for the container {:?}",
                            container.name
                        ));
                    }
                };

            if let Some(container_envs) = container.env.as_mut() {
                for container_env in container_envs {
                    if let Some(value) = updated_env.envs.get(&container_env.name) {
                        match value {
                            EnvKind::Literal(value) => container_env.value = Some(value.clone()),
                            EnvKind::ConfigMap(value) => {
                                container_env.value_from = Some(value.deref().clone())
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
