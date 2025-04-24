// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use anyhow::*;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_yaml::{from_str, to_string, Mapping, Value};
use std::fmt;
use std::{collections::HashMap, env, fs::read_to_string};

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Config {
    pub(crate) name: Option<String>,
    pub(crate) variant: Option<String>,
    pub(crate) service: Option<Vec<String>>,
    pub(crate) setup: Option<Vec<String>>,
    pub(crate) teardown: Option<Vec<String>>,
    pub(crate) run: Vec<String>,
    pub(crate) timeout: Option<u64>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) iterations: u64,
    pub(crate) instructions: bool,
    pub(crate) variants: Option<Vec<String>>,
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", to_string(self).unwrap())
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
    static ref SERVICE_KEY: Value = "service".into();
    static ref SETUP_KEY: Value = "setup".into();
    static ref TEARDOWN_KEY: Value = "teardown".into();
    static ref TIMEOUT_KEY: Value = "timeout".into();
    static ref ITERATIONS_KEY: Value = "iterations".into();
    static ref INSTRUCTIONS_KEY: Value = "instructions".into();
}

fn apply_config(config: &mut Config, config_val: &Value) -> Result<()> {
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

    if config_val.contains_key(&SERVICE_KEY) {
        config.service = Some(get_shell_command(config_val, &SERVICE_KEY)?);
    }

    if config_val.contains_key(&RUN_KEY) {
        config.run = get_shell_command(config_val, &RUN_KEY)?;
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

    if let Some(iterations_val) = config_val.get(&ITERATIONS_KEY) {
        config.iterations = iterations_val
            .as_u64()
            .ok_or_else(|| anyhow!("iterations must be an integer >=1"))?;
        ensure!(config.iterations > 0, "iterations must be an integer >=1");
    }

    if let Some(instructions_val) = config_val.get(&INSTRUCTIONS_KEY) {
        config.instructions = instructions_val
            .as_bool()
            .ok_or_else(|| anyhow!("'instructions' must be a boolean"))?;
    }

    if let Some(env) = config_val.get(&"env".to_owned().into()) {
        get_env(&mut config.env, &env)?;
    }
    Ok(())
}

pub(crate) fn get_config(filename: &str) -> Result<Config> {
    let mut config = Config {
        name: None,
        variant: None,
        service: None,
        setup: None,
        teardown: None,
        run: vec!["INIT".into()],
        timeout: None,
        env: HashMap::new(),
        instructions: false,
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
                    return Ok(config);
                } else if let Some(variants) = variants.as_mapping() {
                    config.variants = Some(
                        variants
                            .iter()
                            .map(|(k, _v)| k.as_str().unwrap().to_owned())
                            .collect(),
                    );
                    return Ok(config);
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

    if config.run.concat() == "INIT" {
        bail!("'run' must be provided");
    }

    Ok(config)
}
