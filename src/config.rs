// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use anyhow::*;
use lazy_static::lazy_static;
use serde_yaml::{from_str, Mapping, Value};
use std::convert::{TryFrom, TryInto};
use std::{collections::HashMap, env, fs::read_to_string};

pub(crate) struct Config {
    pub(crate) setup: Option<Vec<String>>,
    pub(crate) run: Vec<String>,
    pub(crate) timeout: Option<u64>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) cachegrind: bool,
    pub(crate) iterations: u64,
}

struct ProtoConfig {
    setup: Option<Vec<String>>,
    run: Option<Vec<String>>,
    timeout: Option<u64>,
    env: HashMap<String, String>,
    cachegrind: bool,
    iterations: u64,
}

impl TryFrom<ProtoConfig> for Config {
    type Error = Error;

    fn try_from(config: ProtoConfig) -> Result<Config> {
        Ok(Config {
            setup: config.setup,
            run: match config.run {
                Some(run) => run,
                None => bail!("'run' must be provided"),
            },
            timeout: config.timeout,
            env: config.env,
            cachegrind: config.cachegrind,
            iterations: config.iterations,
        })
    }
}

fn get_shell_command(obj: &Mapping, name: &Value) -> Result<Vec<String>> {
    let run = obj
        .get(name)
        .unwrap()
        .as_str()
        .ok_or(anyhow!("'{}' must be a string", name.as_str().unwrap()))?;

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
        .ok_or(anyhow!("env must be an object"))?;
    for (name, value) in config_env.iter() {
        let value = value.as_str().ok_or(anyhow!("env vars must be strings"))?;
        let name = name
            .as_str()
            .ok_or(anyhow!("env var names must be strings"))?;
        env.insert(name.to_owned(), value.to_owned());
    }
    Ok(())
}

lazy_static! {
    static ref RUN: Value = "run".into();
    static ref SETUP: Value = "setup".into();
    static ref TIMEOUT: Value = "timeout".into();
    static ref CACHEGRIND: Value = "cachegrind".into();
    static ref ITERATIONS: Value = "iterations".into();
}

fn apply_config(config: &mut ProtoConfig, config_val: &Value) -> Result<()> {
    let config_val = config_val.as_mapping().ok_or(anyhow!("invalid json"))?;

    if config_val.contains_key(&RUN) {
        config.run = Some(get_shell_command(config_val, &RUN)?);
    }

    if config_val.contains_key(&SETUP) {
        config.setup = Some(get_shell_command(config_val, &SETUP)?);
    }

    if let Some(timeout_val) = config_val.get(&TIMEOUT) {
        config.timeout = Some(
            timeout_val
                .as_u64()
                .ok_or(anyhow!("'timeout' must be a positive integer"))?,
        );
    }

    if let Some(cachegrind_val) = config_val.get(&CACHEGRIND) {
        config.cachegrind = cachegrind_val
            .as_bool()
            .ok_or(anyhow!("'cachegrind' must be a boolean"))?;
    }

    if let Some(iterations_val) = config_val.get(&ITERATIONS) {
        config.iterations = iterations_val
            .as_u64()
            .ok_or(anyhow!("iterations must be an integer >=1"))?;
        if config.iterations == 0 {
            bail!("iterations must be an integer >=1");
        }
    }

    if let Some(env) = config_val.get(&"env".to_owned().into()) {
        get_env(&mut config.env, &env)?;
    }
    Ok(())
}

pub(crate) fn get_config(filename: &str) -> Result<Config> {
    let mut config = ProtoConfig {
        setup: None,
        run: None,
        timeout: None,
        env: HashMap::new(),
        cachegrind: false,
        iterations: 1,
    };
    let json_str = read_to_string(filename)?;
    let config_val: Value = from_str(&json_str)?;

    apply_config(&mut config, &config_val)?;

    if let Some(variants) = config_val.get("variants") {
        let variant_key = env::var("SIRUN_VARIANT")?;
        let config_json = if let Some(variants) = variants.as_sequence() {
            let variant_key = variant_key.parse()?;
            if variants.len() <= variant_key {
                bail!("variant index {} does not exist in array", variant_key);
            }
            &variants[variant_key]
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
