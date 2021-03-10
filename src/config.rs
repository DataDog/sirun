// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use lazy_static::lazy_static;
use serde_yaml::{from_str, Error, Mapping, Value};
use std::convert::{TryFrom, TryInto};
use std::{collections::HashMap, env, fs::read_to_string};

pub(crate) struct Config {
    pub(crate) setup: Option<Vec<String>>,
    pub(crate) run: Vec<String>,
    pub(crate) timeout: Option<u64>,
    pub(crate) env: HashMap<String, String>,
}

struct ProtoConfig {
    setup: Option<Vec<String>>,
    run: Option<Vec<String>>,
    timeout: Option<u64>,
    env: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigError(String);

impl TryFrom<ProtoConfig> for Config {
    type Error = ConfigError;

    fn try_from(config: ProtoConfig) -> Result<Config, ConfigError> {
        Ok(Config {
            setup: config.setup,
            run: match config.run {
                Some(run) => run,
                None => return Err("'run' must be provided".into()),
            },
            timeout: config.timeout,
            env: config.env,
        })
    }
}

impl From<String> for ConfigError {
    fn from(string: String) -> Self {
        ConfigError(string)
    }
}

impl From<&str> for ConfigError {
    fn from(string: &str) -> Self {
        string.to_owned().into()
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        format!("{}", err).into()
    }
}

impl From<std::env::VarError> for ConfigError {
    fn from(err: std::env::VarError) -> Self {
        format!("{}", err).into()
    }
}

impl From<Error> for ConfigError {
    fn from(err: Error) -> Self {
        format!("{}", err).into()
    }
}

macro_rules! errify {
    ($format:expr, $val:expr) => {
        return Err(format!($format, $val).into())
    };
}

fn get_shell_command(obj: &Mapping, name: &Value) -> Result<Vec<String>, ConfigError> {
    let run = obj
        .get(name)
        .unwrap()
        .as_str()
        .ok_or(format!("'{}' must be a string", name.as_str().unwrap()))?;

    shlex::split(run).ok_or_else(|| {
        format!(
            "'{}' must be a properly formed shell command",
            name.as_str().unwrap()
        )
        .into()
    })
}

fn get_env(env: &mut HashMap<String, String>, config_env: &Value) -> Result<(), ConfigError> {
    let config_env = config_env.as_mapping().ok_or("env must be an object")?;
    for (name, value) in config_env.iter() {
        let value = value.as_str().ok_or("env vars must be strings")?;
        let name = name.as_str().ok_or("env var names must be strings")?;
        env.insert(name.to_owned(), value.to_owned());
    }
    Ok(())
}

lazy_static! {
    static ref RUN: Value = "run".into();
    static ref SETUP: Value = "setup".into();
    static ref TIMEOUT: Value = "timeout".into();
}

fn apply_config(config: &mut ProtoConfig, config_val: &Value) -> Result<(), ConfigError> {
    let config_val = config_val.as_mapping().ok_or("invalid json")?;

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
                .ok_or("'timeout' must be a positive integer")?,
        );
    }

    if let Some(env) = config_val.get(&"env".to_owned().into()) {
        get_env(&mut config.env, &env)?;
    }
    Ok(())
}

pub(crate) fn get_config(filename: String) -> Result<Config, ConfigError> {
    let mut config = ProtoConfig {
        setup: None,
        run: None,
        timeout: None,
        env: HashMap::new(),
    };
    let json_str = read_to_string(filename)?;
    let config_val: Value = from_str(&json_str)?;

    apply_config(&mut config, &config_val)?;

    if let Some(variants) = config_val.get("variants") {
        let variant_key = env::var("SIRUN_VARIANT")?;
        let config_json;
        if let Some(variants) = variants.as_sequence() {
            let variant_key = variant_key.parse().unwrap();
            if variants.len() <= variant_key {
                errify!("variant index {} does not exist in array", variant_key);
            }
            config_json = Some(&variants[variant_key]);
        } else if let Some(variants) = variants.as_mapping() {
            config_json = match variants.get(&variant_key.clone().into()) {
                Some(val) => Some(val),
                None => errify!("variant key {} does not exist in object", variant_key),
            };
        } else {
            return Err("variants must be array or object".into());
        }
        apply_config(&mut config, &config_json.unwrap())?;
    }

    Ok(config.try_into()?)
}
