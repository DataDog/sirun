// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use anyhow::*;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_yaml::{from_str, Mapping, Value};
use std::convert::{TryFrom, TryInto};
use std::{collections::HashMap, env, fs::read_to_string};

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Config {
    pub(crate) name: Option<String>,
    pub(crate) variant: Option<String>,
    pub(crate) setup: Option<Vec<String>>,
    pub(crate) teardown: Option<Vec<String>>,
    pub(crate) run: Vec<String>,
    pub(crate) timeout: Option<u64>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) cachegrind: bool,
    pub(crate) iterations: u64,
    pub(crate) variants: Option<Vec<String>>,
}

struct ProtoConfig {
    name: Option<String>,
    variant: Option<String>,
    setup: Option<Vec<String>>,
    teardown: Option<Vec<String>>,
    run: Option<Vec<String>>,
    timeout: Option<u64>,
    env: HashMap<String, String>,
    cachegrind: bool,
    iterations: u64,
    variants: Option<Vec<String>>,
}

impl TryFrom<ProtoConfig> for Config {
    type Error = Error;

    fn try_from(config: ProtoConfig) -> Result<Config> {
        Ok(Config {
            name: config.name,
            variant: config.variant,
            setup: config.setup,
            teardown: config.teardown,
            run: match config.run {
                Some(run) => run,
                None => bail!("'run' must be provided"),
            },
            timeout: config.timeout,
            env: config.env,
            cachegrind: config.cachegrind,
            iterations: config.iterations,
            variants: config.variants,
        })
    }
}

fn get_shell_command(obj: &Mapping, name: &Value) -> Result<Vec<String>> {
    let run = obj
        .get(name)
        .unwrap()
        .as_str()
        .ok_or_else(|| anyhow!("'{}' must be a string", name.as_str().unwrap()))?;

    shlex::split(run).ok_or_else(|| {
        anyhow!(
            "'{}' must be a properly formed shell command",
            name.as_str().unwrap()
        )
    })
}

fn get_env(env: &mut HashMap<String, String>, config_env: &Value) -> Result<()> {
    let config_env = config_env
        .as_mapping()
        .ok_or_else(|| anyhow!("env must be an object"))?;
    for (name, value) in config_env.iter() {
        let value = value
            .as_str()
            .ok_or_else(|| anyhow!("env vars must be strings"))?;
        let name = name
            .as_str()
            .ok_or_else(|| anyhow!("env var names must be strings"))?;
        env.insert(name.to_owned(), value.to_owned());
    }
    Ok(())
}

lazy_static! {
    static ref NAME_KEY: Value = "name".into();
    static ref RUN_KEY: Value = "run".into();
    static ref SETUP_KEY: Value = "setup".into();
    static ref TEARDOWN_KEY: Value = "teardown".into();
    static ref TIMEOUT_KEY: Value = "timeout".into();
    static ref CACHEGRIND_KEY: Value = "cachegrind".into();
    static ref ITERATIONS_KEY: Value = "iterations".into();
}

fn apply_config(config: &mut ProtoConfig, config_val: &Value) -> Result<()> {
    let config_val = config_val
        .as_mapping()
        .ok_or_else(|| anyhow!("invalid json"))?;

    if let Ok(name) = env::var("SIRUN_NAME") {
        config.name = Some(name)
    } else if let Some(name_val) = config_val.get(&NAME_KEY) {
        config.name = Some(
            name_val
                .as_str()
                .ok_or_else(|| anyhow!("'name' must be a string"))?
                .to_owned(),
        );
    }

    if config_val.contains_key(&RUN_KEY) {
        config.run = Some(get_shell_command(config_val, &RUN_KEY)?);
    }

    if config_val.contains_key(&SETUP_KEY) {
        config.setup = Some(get_shell_command(config_val, &SETUP_KEY)?);
    }

    if config_val.contains_key(&TEARDOWN_KEY) {
        config.teardown = Some(get_shell_command(config_val, &TEARDOWN_KEY)?);
    }

    if let Some(timeout_val) = config_val.get(&TIMEOUT_KEY) {
        config.timeout = Some(
            timeout_val
                .as_u64()
                .ok_or_else(|| anyhow!("'timeout' must be a positive integer"))?,
        );
    }

    if let Some(cachegrind_val) = config_val.get(&CACHEGRIND_KEY) {
        config.cachegrind = cachegrind_val
            .as_bool()
            .ok_or_else(|| anyhow!("'cachegrind' must be a boolean"))?;
    }

    if let Some(iterations_val) = config_val.get(&ITERATIONS_KEY) {
        config.iterations = iterations_val
            .as_u64()
            .ok_or_else(|| anyhow!("iterations must be an integer >=1"))?;
        ensure!(config.iterations > 0, "iterations must be an integer >=1");
    }

    if let Some(env) = config_val.get(&"env".to_owned().into()) {
        get_env(&mut config.env, &env)?;
    }
    Ok(())
}

pub(crate) fn get_config(filename: &str) -> Result<Config> {
    let mut config = ProtoConfig {
        name: None,
        variant: None,
        setup: None,
        teardown: None,
        run: None,
        timeout: None,
        env: HashMap::new(),
        cachegrind: false,
        iterations: 1,
        variants: None,
    };
    let json_str = read_to_string(filename)?;
    let config_val: Value = from_str(&json_str)?;

    apply_config(&mut config, &config_val)?;

    if let Some(variants) = config_val.get("variants") {
        let variant_key = match env::var("SIRUN_VARIANT") {
            Ok(variant_key) => variant_key,
            Err(_) => {
                if let Some(variants) = variants.as_sequence() {
                    let usize_ids: Vec<usize> = (0..variants.len()).collect();
                    config.variants = Some(usize_ids.iter().map(|i| i.to_string()).collect());
                    return config.try_into();
                } else if let Some(variants) = variants.as_mapping() {
                    config.variants = Some(
                        variants
                            .iter()
                            .map(|(k, _v)| k.as_str().unwrap().to_owned())
                            .collect(),
                    );
                    return config.try_into();
                } else {
                    bail!("variants must be an array or object");
                }
            }
        };

        config.variant = Some(variant_key.clone());
        let config_json = if let Some(variants) = variants.as_sequence() {
            let id = variant_key.parse()?;
            ensure!(
                variants.len() > id,
                "variant index {} does not exist in array",
                id
            );
            &variants[id]
        } else if let Some(variants) = variants.as_mapping() {
            match variants.get(&variant_key.clone().into()) {
                Some(val) => val,
                None => bail!("variant key {} does not exist in object", variant_key),
            }
        } else {
            bail!("variants must be array or object")
        };
        apply_config(&mut config, &config_json)?;
    }

    config.try_into()
}
