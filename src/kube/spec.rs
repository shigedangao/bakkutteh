use anyhow::{anyhow, Result};
use k8s_openapi::api::{
    batch::v1::JobSpec,
    core::v1::{EnvVar, EnvVarSource},
};
use std::{collections::BTreeMap, ops::Deref};

#[derive(Debug, PartialEq, Clone)]
pub enum EnvKind {
    Literal(String),
    ConfigMap(Box<EnvVarSource>),
}

#[derive(Default, Debug)]
pub struct ContainerEnv {
    pub name: String,
    pub envs: BTreeMap<String, EnvKind>,
}

pub trait SpecHandler {
    /// Extract the environment variables of the container (secrets are avoid)
    fn get_env(&self) -> Result<Vec<ContainerEnv>>;
    /// Rebuild the environment variable based on the one provided by the user
    ///
    /// # Arguments
    ///
    /// * `envs` - Vec<ContainerEnv>
    fn rebuild_env(&mut self, envs: &mut Vec<ContainerEnv>) -> Result<()>;
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
                let envs: BTreeMap<String, EnvKind> = env
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

    fn rebuild_env(&mut self, envs: &mut Vec<ContainerEnv>) -> Result<()> {
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
                    .get_mut(idx)
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
                for container_env in container_envs.iter_mut() {
                    if let Some(value) = updated_env.envs.get(&container_env.name) {
                        match value {
                            EnvKind::Literal(value) => container_env.value = Some(value.clone()),
                            EnvKind::ConfigMap(value) => {
                                container_env.value_from = Some(value.deref().clone())
                            }
                        }

                        // Drain the key from the map
                        updated_env.envs.remove(&container_env.name);
                    }
                }

                // Add additional environment variables to the container if there are still some existing keys
                if !updated_env.envs.is_empty() {
                    for (key, value) in &updated_env.envs {
                        if let EnvKind::Literal(value) = value {
                            container_envs.push(EnvVar {
                                name: key.to_owned(),
                                value: Some(value.to_owned()),
                                value_from: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SpecHandler;
    use crate::kube::spec::EnvKind;
    use k8s_openapi::api::{
        batch::v1::JobSpec,
        core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec},
    };

    #[test]
    fn expect_to_process_env() {
        let job_spec = JobSpec {
            template: PodTemplateSpec {
                metadata: None,
                spec: Some(PodSpec {
                    containers: vec![Container {
                        env: Some(vec![EnvVar {
                            name: "key".to_string(),
                            value: Some("value".to_string()),
                            ..Default::default()
                        }]),
                        name: "main".to_string(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        };

        let fetched_env = job_spec.get_env();
        assert!(fetched_env.is_ok());

        let fetched_env = fetched_env.unwrap();
        let container = fetched_env.first().unwrap();

        assert_eq!(container.name, "main");
        assert_eq!(
            *container.envs.get("key").unwrap(),
            EnvKind::Literal("value".to_string())
        );
    }

    #[test]
    fn expect_to_rebuild_env() {
        let mut job_spec = JobSpec {
            template: PodTemplateSpec {
                metadata: None,
                spec: Some(PodSpec {
                    containers: vec![Container {
                        env: Some(vec![EnvVar {
                            name: "key".to_string(),
                            value: Some("value".to_string()),
                            ..Default::default()
                        }]),
                        name: "main".to_string(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        };

        let mut envs = job_spec.get_env().unwrap();
        let container = envs.first_mut().expect("Expect to get the first container");

        let env = container
            .envs
            .get_mut("key")
            .expect("Expect to get mutable reference of the environment variable");

        *env = EnvKind::Literal("dodo".to_string());

        // Rebuild the environment variable and expect the job spec key = dodo
        let res = job_spec.rebuild_env(&mut envs);
        assert!(res.is_ok());

        let spec = job_spec
            .template
            .spec
            .expect("Expect to get the spec of the pod");
        let container = spec.containers.first().expect("Expect to get a container");
        let new_env = container.env.as_ref().unwrap().first().unwrap();

        assert_eq!(new_env.value.as_ref().unwrap(), "dodo");
    }
}
