use anyhow::*;
use async_std::{
    process::{Command, ExitStatus, Stdio},
    task::sleep,
};
use std::{collections::HashMap, env, os::unix::process::ExitStatusExt, time::Duration};

use crate::config::*;

async fn run_setup_or_teardown(typ: &str, config: &Config) -> Result<()> {
    if env::var("SIRUN_SKIP_SETUP").is_ok() {
        return Ok(());
    }
    let command_arr = if typ == "setup" {
        &config.setup
    } else {
        &config.teardown
    };
    let command_arr = match command_arr {
        Some(command_arr) => command_arr,
        None => return Ok(()),
    };
    let env = &config.env;
    let mut code: i32 = 1;
    let mut attempts: u8 = 0;
    while code != 0 {
        if attempts == 100 {
            bail!("{} script did not complete successfully. aborting.", typ);
        }
        let status = run_cmd(command_arr, env).await?;
        let maybe_code = status.code();
        if let Some(maybe_code) = maybe_code {
            code = maybe_code;
            if code != 0 {
                sleep(Duration::from_secs(1)).await;
                attempts += 1;
            }
        } else {
            let signal = status.signal().unwrap();
            bail!(
                "{} script was terminated by signal {}. aborting.",
                typ,
                signal
            );
        }
    }

    Ok(())
}

pub(crate) async fn run_setup(config: &Config) -> Result<()> {
    run_setup_or_teardown("setup", config).await
}

pub(crate) async fn run_teardown(config: &Config) -> Result<()> {
    run_setup_or_teardown("teardown", config).await
}

fn get_stdio() -> Stdio {
    match env::var("SIRUN_NO_STDIO") {
        Ok(_) => Stdio::null(),
        Err(_) => Stdio::inherit(),
    }
}

pub(crate) async fn run_cmd(
    command_arr: &[String],
    env: &HashMap<String, String>,
) -> Result<ExitStatus> {
    let command = command_arr[0].clone();
    let args = command_arr.iter().skip(1);
    Command::new(command)
        .args(args)
        .envs(env.clone())
        .stdout(get_stdio())
        .stderr(get_stdio())
        .status()
        .await
        .map_err(|e| e.into())
}
