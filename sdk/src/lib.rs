use std::collections::HashMap;

use anyhow::{bail, Ok, Result};
use async_process::Command;
use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools;
use lazy_static::lazy_static;
use log::{debug, info};
use regex::Regex;
use url::Url;

type Hydration = HashMap<String, String>;

#[async_trait]
trait Provider {
    fn add(&mut self, value: String) -> Result<()>;
    async fn resolve(&self) -> Result<Hydration>;
}

#[derive(Default)]
struct Doppler {
    urls: Vec<Url>,
}

impl Doppler {
    fn new() -> Self {
        Default::default()
    }
}

#[async_trait]
impl Provider for Doppler {
    fn add(&mut self, value: String) -> Result<()> {
        match Url::parse(&value) {
            std::result::Result::Ok(url) if url.scheme() == "doppler" => {
                self.urls.push(url);
                Ok(())
            }
            _ => bail!("Not a doppler scheme"),
        }
    }
    async fn resolve(&self) -> Result<Hydration> {
        let fetches = self
            .urls
            .iter()
            .group_by(|u| u.host().expect("Missing host"))
            .into_iter()
            .flat_map(|(host, group)| {
                group
                    .into_iter()
                    .group_by(|u| u.path().split('/').nth(1).expect("Missing project"))
                    .into_iter()
                    .flat_map(|(project, group)| {
                        group
                            .into_iter()
                            .group_by(|u| u.path().split('/').nth(2).expect("Missing env"))
                            .into_iter()
                            .map(|(env, group)| {
                                let vars = group
                                    .into_iter()
                                    .map(|u| {
                                        (
                                            u.path().split('/').nth(3).expect("Missing variable"),
                                            u.to_string(),
                                        )
                                    })
                                    .collect::<HashMap<_, _>>();

                                let host = host.clone();
                                async move {
                                    let args = &[
                                        "--api-host",
                                        &format!("https://{}", host),
                                        "run",
                                        "--project",
                                        project,
                                        "--config",
                                        env,
                                        "--mount",
                                        "secrets.json",
                                        "--",
                                        "cat",
                                        "secrets.json",
                                    ];
                                    info!("doppler {}", args.join(" "));
                                    let output =
                                        Command::new("doppler").args(args).output().await?;

                                    let loaded =
                                        serde_json::from_slice::<Hydration>(&output.stdout)?;

                                    let hydration = vars
                                        .into_iter()
                                        .map(|(key, var)| {
                                            (
                                                var,
                                                loaded
                                                    .get(key)
                                                    .expect("Variable not found")
                                                    .clone(),
                                            )
                                        })
                                        .collect::<Hydration>();

                                    debug!("{:?}", hydration);
                                    Ok(hydration)
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Ok(try_join_all(fetches).await?.into_iter().flatten().collect())
    }
}

#[derive(Default)]

struct Raw {
    values: Vec<String>,
}

impl Raw {
    fn new() -> Self {
        Default::default()
    }
}

#[async_trait]
impl Provider for Raw {
    fn add(&mut self, value: String) -> Result<()> {
        self.values.push(value);
        Ok(())
    }
    async fn resolve(&self) -> Result<Hydration> {
        let ret = self.values.iter().map(|v| (v.clone(), v.clone())).collect();
        Ok(ret)
    }
}

struct Hydrater {
    providers: Vec<Box<dyn Provider>>,
}

impl Hydrater {
    fn new() -> Self {
        Self {
            providers: vec![Box::new(Doppler::new()), Box::new(Raw::new())],
        }
    }
    fn add(&mut self, value: String) -> Result<()> {
        for provider in self.providers.iter_mut() {
            if provider.add(value.clone()).is_ok() {
                return Ok(());
            }
        }
        bail!("No provider found")
    }
    async fn resolve(&self) -> Result<Hydration> {
        Ok(try_join_all(self.providers.iter().map(|p| p.resolve()))
            .await?
            .into_iter()
            .flatten()
            .collect())
    }
}

pub async fn hydrate(env: HashMap<String, String>) -> Result<HashMap<String, String>> {
    let mut hydrater = Hydrater::new();
    for value_or_uri in env.values() {
        hydrater.add(value_or_uri.clone())?
    }

    let hydration = hydrater.resolve().await?;
    println!("{:?}", env);

    let mut ret: HashMap<String, String> = HashMap::default();
    for (key, value_or_uri) in env.iter() {
        ret.insert(key.clone(), hydration.get(value_or_uri).unwrap().clone());
    }

    Ok(ret)
}

lazy_static! {
    static ref VAR: Regex = Regex::new(r"(\$\{?(\w+)\}?)").unwrap();
}

pub fn resolve_env(
    kvs: &HashMap<String, String>,
    existing_vars: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let ret = kvs
        .iter()
        .map(|(key, value)| {
            let hydration = VAR.captures_iter(value).fold(value.clone(), |agg, c| {
                agg.replace(&c[1], existing_vars.get(&c[2]).unwrap_or(&"".to_string()))
            });
            (key.clone(), hydration)
        })
        .collect();
    Ok(ret)
}
