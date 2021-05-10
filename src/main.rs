// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

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
    run_setup(&config).await?;

    let mut sub_config: Config = config.clone();
    let json_config = serde_yaml::to_string(&config)?;
    sub_config.env.insert("SIRUN_ITERATION".into(), json_config);
    let status = run_cmd(&env::args().take(1).collect(), &sub_config.env).await?;
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
    if env::args().nth(1).unwrap() == "--summarize" {
        return summarize().await;
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
