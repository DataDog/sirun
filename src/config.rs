use serde_json::{from_str, Value};
use shlex;
use std::fs::read_to_string;

pub(crate) struct Config {
    pub(crate) setup: Option<Vec<String>>,
    pub(crate) run: Vec<String>,
    pub(crate) timeout: Option<u64>,
}

fn get_shell_command(
    obj: &serde_json::Map<String, Value>,
    name: &str,
) -> std::result::Result<Vec<String>, String> {
    let run = obj.get(name).unwrap();
    if !run.is_string() {
        return Err(format!("'{}' must be a string", name).into());
    }
    let run = run.as_str().unwrap();
    let run = shlex::split(run);
    if let Some(run) = run {
        Ok(run)
    } else {
        Err(format!("'{}' must be a properly formed shell command", name).into())
    }
}

pub(crate) fn get_config(filename: String) -> std::result::Result<Config, String> {
    let json_str = match read_to_string(filename) {
        Ok(json_str) => json_str,
        Err(err) => return Err(format!("{}", err)),
    };
    let config_val: Value = match from_str(&json_str) {
        Ok(config_val) => config_val,
        Err(err) => return Err(format!("{}", err)),
    };
    let config_val = match config_val.as_object() {
        Some(config_val) => config_val,
        None => return Err("invalid json".into()),
    };

    if !config_val.contains_key("run") {
        return Err("json must contain key 'run'".into());
    }
    let run = match get_shell_command(&config_val, "run") {
        Ok(run) => run,
        Err(err) => return Err(err),
    };

    let mut setup = None;
    if config_val.contains_key("setup") {
        setup = match get_shell_command(&config_val, "setup") {
            Ok(setup) => Some(setup),
            Err(err) => return Err(err),
        };
    }

    let mut timeout = None;
    if let Some(timeout_val) = config_val.get("timeout") {
        if !timeout_val.is_u64() {
            return Err("'timeout' must be a positive integer".into());
        }
        timeout = timeout_val.as_u64();
    }

    Ok(Config {
        setup,
        run,
        timeout,
    })
}
