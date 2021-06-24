// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

//! `sirun` (pronounced like "siren") is a tool for taking basic perfomance
//! measurements of a process covering its entire lifetime. It gets memory and
//! timing information from the kernel and also allows
//! [Statsd](https://github.com/statsd/statsd#usage) messages to be sent to
//! `udp://localhost:8125` (in the future this will be configurable), and those will
//! be included in the outputted metrics.
//!
//! It's intended that this tool be used for shorter-running benchmarks, and not for
//! long-lived processes that don't die without external interaction. You could
//! certainly use it for long-lived processes, but that's not where it shines.
//!
//! ## Usage
//!
//! Create a JSON or YAML file with the following properties.
//!
//! * **`name`**: This will be included in the results JSON.
//! * **`run`**: The command to run and test. You can format this like a shell
//!   command with arguments, but note that it will not use a shell as an
//!   intermediary process. Note that subprocesses will not be measured via the
//!   kernel, but they can still use Statsd. To send metrics to Statsd from inside
//!   this process, send them to `udp://localhost:8125`.
//! * **`setup`**: A command to run _before_ the test. Use this to ensure the
//!   availability of services, or retrieve some last-minute dependencies. This can
//!   be formatted the same way as `run`. It will be run repeatedly at 1 second
//!   intervals until it exits with status code 0.
//! * **`teardown`**: A command to run _after_ the test. This is run in the same
//!   manner as `setup`, except after the test has run instead of before.
//! * **`timeout`**: If provided, this is the maximum time, in seconds, a `run` test
//!   can run for. If it times out, `sirun` will exit with no results, aborting the
//!   test.
//! * **`env`**: A set of environment variables to make available to the `run` and
//!   `setup` programs. This should be an object whose keys are the environment
//!   variable names and whose values are the environment variable values.
//! * **`iterations`**: The number of times to run the the `run` test. The results
//!   for each iteration will be in an `iterations` array in the resultant JSON. The
//!   default is 1.
//! * **`cachegrind`**: If set to `true`, will run the test (after having already
//!   run it normally) using cachegrind to add an instruction count to the results
//!   JSON. Will only happen once, and be inserted into the top level JSON,
//!   regardless of `iterations`. This requires that `valgrind` is installed on your
//!   system.
//! * **`variants`**: An array or object whose values are config objects, whose
//!   properties may be any of the properties above. It's not recommended to include
//!   `name` in a variant. The variant name (if `variants` is an object) or index
//!   (if `variants` is an array) will be included in resultant JSON.
//!
//! ### Environment Variables
//!
//! * **`GIT_COMMIT_HASH`**: If set, will include a `version` in the
//!   results.
//! * **`SIRUN_NAME`**: If set, will include a `name` in the results. This overrides
//!   any `name` property set in config JSON/YAML.
//! * **`SIRUN_NO_STDIO`**: If set, supresses output from the tested program.
//! * **`SIRUN_VARIANT`**: Selects which variant of the test to run. If the
//!   `variants` property exists in the config JSON/YAML, and this variable is not
//!   set, then _all_ variants will be run, one-by-one, each having its own line of
//!   output JSON.
//!
//! ### Example
//!
//! Here's an example JSON file. As an example of a `setup` script, it's checking for
//! connectivity to Google. The `run` script doesn't do much, but it does send a
//! single metric (with name `udp.data` and value 50) via Statsd. It times out after
//! 4 seconds, and we're not likely to reach that point.
//!
//! ```js
//! {
//!   "setup": "curl -I http://www.google.com -o /dev/null",
//!   "run": "bash -c \"echo udp.data:50\\|g > /dev/udp/127.0.0.1/8125\"",
//!   "timeout": 4
//! }
//! ```
//!
//! You can then pass this JSON file to `sirun` on the command line. Remember that
//! you can use environment variables to set the git commit hash and test name in
//! the output.
//!
//! ```sh
//! SIRUN_NAME=test_some_stuff GIT_COMMIT_HASH=123abc sirun ./my_benchmark.json
//! ```
//!
//! This will output something like the following.
//!
//! ```
//!   % Total    % Received % Xferd  Average Speed   Time    Time     Time  Current
//!                                  Dload  Upload   Total   Spent    Left  Speed
//!   0     0    0     0    0     0      0      0 --:--:-- --:--:-- --:--:--     0
//! {"version":"123abc","name":"test_some_stuff",iterations:[{"user.time":6389.0,"system.time":8737.0,"udp.data":50.0,"max.res.size":2240512.0}]}
//! ```
//!
//! ### Summaries
//!
//! If you provide the `--summarize` option, `sirun` will switch to summary mode. In
//! summary mode, it will read from `stdin`, expecting line-by-line of output from
//! previous sirun runs. It will then aggregate them by test name and variant, and
//! provide summary statistics over iterations. The output is pretty-printed JSON.
//!
//! E.g.
//!
//! ```bash
//! $ sirun foo-test.json >> results.ndjson
//! $ sirun bar-test.json >> results.ndjson
//! $ sirun baz-test.json >> results.ndjson
//! $ cat results.ndjson | sirun --summarize > summary.json
//! ```

use anyhow::*;
use async_std::{
    net::UdpSocket,
    process::{Command, Stdio},
    sync::{Arc, Barrier, RwLock},
    task::{sleep, spawn},
};
use serde_json::json;
use std::{collections::HashMap, env, process::exit};
use which::which;

mod config;
use config::*;

mod rusage;
use rusage::*;

mod subproc;
use subproc::*;

mod statsd;
use statsd::*;

mod metric_value;
use metric_value::*;

mod summarize;
use summarize::*;

fn get_kernel_metrics(wall_time: f64, data: Rusage, metrics: &mut HashMap<String, MetricValue>) {
    metrics.insert("max.res.size".into(), data.max_res_size.into());
    metrics.insert("user.time".into(), data.user_time.into());
    metrics.insert("system.time".into(), data.system_time.into());

    let pct = (data.user_time + data.system_time) * 100.0 / wall_time;
    metrics.insert("cpu.pct.wall.time".into(), pct.into());
}

async fn test_timeout(timeout: u64) {
    sleep(std::time::Duration::from_secs(timeout)).await;
    eprintln!("Timeout of {} seconds exceeded.", timeout);
    exit(1);
}

async fn run_test(config: &Config, mut metrics: &mut HashMap<String, MetricValue>) -> Result<()> {
    if let Some(timeout) = config.timeout {
        spawn(test_timeout(timeout));
    }

    let start_time = std::time::Instant::now();
    let rusage_start = Rusage::new();
    let status = run_cmd(&config.run, &config.env).await?;
    let duration = start_time.elapsed().as_micros();
    let rusage_result = Rusage::new() - rusage_start;
    metrics.insert("wall.time".to_owned(), (duration as f64).into());
    let status = status.code().expect("no exit code");
    if status != 0 && status <= 128 {
        eprintln!("Test exited with code {}, so aborting test.", status);
        exit(status);
    }
    get_kernel_metrics(duration as f64, rusage_result, &mut metrics);
    Ok(())
}

async fn run_iteration(
    config: &Config,
    statsd_buf: Arc<RwLock<String>>,
) -> Result<HashMap<String, MetricValue>> {
    let mut sub_config: Config = config.clone();
    let json_config = serde_yaml::to_string(&config)?;
    sub_config.env.insert("SIRUN_ITERATION".into(), json_config);
    run_setup(&sub_config).await?;

    let status = run_cmd(
        &env::args().take(1).collect::<Vec<String>>(),
        &sub_config.env,
    )
    .await?;
    let status = status.code().expect("no exit code");
    if status != 0 && status <= 128 {
        exit(status);
    }
    let metrics = get_statsd_metrics(statsd_buf).await?;

    run_teardown(&config).await?;

    Ok(metrics)
}

async fn run_all_variants(variants: Vec<String>) -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let cmd = args[0].clone();
    let args: Vec<_> = args.iter().skip(1).collect();
    for variant in variants {
        env::set_var("SIRUN_VARIANT", variant);
        Command::new(&cmd)
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await?;
    }
    Ok(())
}

async fn main_main() -> Result<()> {
    if let Some(first_arg) = env::args().nth(1) {
        if first_arg == "--summarize" {
            return summarize().await;
        }
    }
    let config_file = env::args().nth(1).expect("missing file argument");
    let config = get_config(&config_file)?;

    if let Some(variants) = config.variants {
        run_all_variants(variants).await?;
        return Ok(());
    }

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();

    let statsd_started = Arc::new(Barrier::new(2));
    let statsd_buf = Arc::new(RwLock::new(String::new()));

    spawn(statsd_listener(statsd_started.clone(), statsd_buf.clone()));
    statsd_started.wait().await; // waits for socket to be listening

    let mut iterations = Vec::new();
    for _ in 0..config.iterations {
        iterations.push(MetricValue::Map(
            run_iteration(&config, statsd_buf.clone()).await?,
        ));
    }
    metrics.insert("iterations".into(), MetricValue::Arr(iterations));

    if config.cachegrind && which("valgrind").is_ok() {
        let command = "valgrind";
        let mut args = vec![
            "--tool=cachegrind".to_owned(),
            "--trace-children=yes".to_owned(),
            // Set some reasonable L1 and LL values. It is important that these
            // values are consistent across runs, instead of the default.
            "--I1=32768,8,64".to_owned(),
            "--D1=32768,8,64".to_owned(),
            "--LL=8388608,16,64".to_owned(),
        ];
        args.append(&mut config.run.clone());
        run_setup(&config).await?;
        let output = Command::new(command)
            .args(args)
            .envs(&config.env)
            .output()
            .await?;
        run_teardown(&config).await?;
        let stderr = String::from_utf8_lossy(&output.stderr);

        let lines = stderr.trim().lines().filter(|x| x.contains("I   refs:"));
        let mut instructions: f64 = 0.0;
        for line in lines {
            instructions += line
                .trim()
                .split_whitespace()
                .last()
                .expect("Bad cachegrind output: invalid instruction ref line")
                .replace(",", "")
                .parse::<f64>()
                .expect("Bad cachegrind output: invalid number");
        }
        if instructions <= 0.0 {
            eprintln!("Bad cachegrind output: no instructions parsed");
            exit(1);
        }
        metrics.insert("instructions".into(), instructions.into());
    }

    if let Ok(hash) = env::var("GIT_COMMIT_HASH") {
        metrics.insert("version".into(), hash.into());
    }
    if let Some(name) = config.name {
        metrics.insert("name".into(), name.into());
    }
    if let Some(variant) = config.variant {
        metrics.insert("variant".into(), variant.into());
    }

    println!("{}", json!(metrics).to_string());
    Ok(())
}

async fn iteration_main() -> Result<()> {
    let config = serde_yaml::from_str(&env::var("SIRUN_ITERATION").unwrap()).unwrap();

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();

    run_test(&config, &mut metrics).await?;

    let buf = format!(
        "max.res.size:{}|g\nuser.time:{}|g\nsystem.time:{}|g\nwall.time:{}|g\ncpu.pct.wall.time:{}|g\n",
        metrics.remove("max.res.size").unwrap().as_f64(),
        metrics.remove("user.time").unwrap().as_f64(),
        metrics.remove("system.time").unwrap().as_f64(),
        metrics.remove("wall.time").unwrap().as_f64(),
        metrics.remove("cpu.pct.wall.time").unwrap().as_f64()
        );
    let sock = UdpSocket::bind("127.0.0.1:0").await?;
    sock.send_to(buf.as_bytes(), "127.0.0.1:8125").await?;
    Ok(())
}

#[async_std::main]
async fn main() -> Result<()> {
    if env::var("SIRUN_ITERATION").is_ok() {
        iteration_main().await
    } else {
        main_main().await
    }
}
