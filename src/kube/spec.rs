use anyhow::{Result, anyhow};
use k8s_openapi::{
    api::{
        batch::v1::JobSpec,
        core::v1::{EnvVar, EnvVarSource, ResourceRequirements},
    },
    apimachinery::pkg::api::resource::Quantity,
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
    /// * `envs` - &mut Vec<ContainerEnv>
    fn rebuild_env(&mut self, envs: &mut Vec<ContainerEnv>) -> Result<()>;
    /// Update the resources of the pod
    ///
    /// # Arguments
    ///
    /// * `resources` - (String, String)
    fn update_resources(&mut self, resources: (String, String, String)) -> Result<()>;
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

    fn update_resources(&mut self, resources: (String, String, String)) -> Result<()> {
        let (memory, cpu, container_name) = resources;

        let Some(tmpl) = self.template.spec.as_mut() else {
            return Err(anyhow!("Unable to retrieve the spec for the job"));
        };

        let Some(container) = tmpl
            .containers
            .iter_mut()
            .filter(|ct| ct.name == container_name)
            .next_back()
        else {
            return Err(anyhow!("Unable to get the targeted container"));
        };

        match container.resources.as_mut() {
            Some(pds) => {
                let lim = pds.limits.as_mut().map_or(
                    BTreeMap::from([
                        ("cpu".to_string(), Quantity(cpu.clone())),
                        ("memory".to_string(), Quantity(memory.clone())),
                    ]),
                    |lim| {
                        lim.insert("cpu".to_string(), Quantity(cpu));
                        lim.insert("memory".to_string(), Quantity(memory));

                        lim.clone()
                    },
                );

                pds.limits = Some(lim);
            }
            None => {
                container.resources = Some(ResourceRequirements {
                    limits: Some(BTreeMap::from([
                        ("cpu".to_string(), Quantity(cpu)),
                        ("memory".to_string(), Quantity(memory)),
                    ])),
                    requests: None,
                    ..Default::default()
                })
            }
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::SpecHandler;
    use crate::kube::spec::EnvKind;
    use k8s_openapi::{
        api::{
            batch::v1::JobSpec,
            core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec, ResourceRequirements},
        },
        apimachinery::pkg::api::resource::Quantity,
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

    #[test]
    fn expect_to_update_resources() {
        let mut job_spec = JobSpec {
            template: PodTemplateSpec {
                metadata: None,
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "main".to_string(),
                        resources: Some(ResourceRequirements {
                            limits: Some(BTreeMap::from([
                                ("cpu".to_string(), Quantity("0.1".to_string())),
                                ("memory".to_string(), Quantity("0.5".to_string())),
                            ])),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        };

        let res =
            job_spec.update_resources(("10Mb".to_string(), "0.01".to_string(), "main".to_string()));
        assert!(res.is_ok());

        let pod = job_spec.template.spec.unwrap();
        let container = pod.containers.first().unwrap();
        let resources = container.resources.as_ref().unwrap();
        let limits = resources.limits.as_ref().unwrap();

        assert_eq!(
            limits.get("memory").unwrap().clone(),
            Quantity("10Mb".to_string())
        );
        assert_eq!(
            limits.get("cpu").unwrap().clone(),
            Quantity("0.01".to_string())
        );
    }
}
