// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use anyhow::*;
use async_std::{
    net::UdpSocket,
    process::{Command, Stdio, Child, ExitStatus},
    sync::{Arc, Barrier, RwLock},
    task::{sleep, spawn},
};
use serde_json::json;
use std::{collections::HashMap, env, os::unix::process::ExitStatusExt, process::exit};
use which::which;
use indexmap::IndexMap;

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

#[cfg(target_os = "linux")]
async fn run_with_instruction_count(child: &mut Child, config: &Config) -> Result<(ExitStatus, Option<u64>)> {
    use perfcnt::AbstractPerfCounter;
    use perfcnt::linux::{PerfCounterBuilderLinux, HardwareEventType};
    if config.instructions {
        let pid = child.id();
        let mut counter = PerfCounterBuilderLinux::from_hardware_event(HardwareEventType::Instructions)
            .for_pid(pid as i32)
            .finish()?;
        counter.start()?;
        let status = child.status().await?;
        counter.stop()?;
        let instructions = counter.read()?;

        Ok((status, Some(instructions)))
    } else {
        Ok((child.status().await?, None))
    }
}

#[cfg(not(target_os = "linux"))]
async fn run_with_instruction_count(child: &mut Child, _config: &Config) -> Result<(ExitStatus, Option<u64>)> {
    Ok((child.status().await?, None))
}

async fn run_test(config: &Config, mut metrics: &mut HashMap<String, MetricValue>) -> Result<()> {
    if let Some(timeout) = config.timeout {
        spawn(test_timeout(timeout));
    }

    let start_time = std::time::Instant::now();
    let rusage_start = Rusage::new();
    let mut child = run_cmd(&config.run, &config.env)?;
    let (status, instructions) = run_with_instruction_count(&mut child, config).await?;
    let duration = start_time.elapsed().as_micros();
    metrics.insert("wall.time".to_owned(), (duration as f64).into());
    let rusage_result = Rusage::new() - rusage_start;
    if let Some(instructions) = instructions {
        metrics.insert("instructions".to_owned(), (instructions as f64).into());
    }
    if let Some(status) = status.code() {
        if status != 0 && status <= 128 {
            eprintln!(
                "Test exited with code {}, so aborting test.\n\nTest Config:\n{}",
                status, config
            );
            exit(status);
        }
    } else {
        if let Some(status) = status.signal() {
            eprintln!(
                "Test was terminated via signal {}, so aborting test.\n\nTest Config:\n{}",
                status, config
            );
            exit(1);
        }
    }
    get_kernel_metrics(duration as f64, rusage_result, &mut metrics);
    Ok(())
}

fn run_service(config: &Config) -> Result<Option<Child>> {
    Ok(match &config.service {
        Some(command_arr) => Some(run_cmd(command_arr, &config.env)?),
        None => None,
    })
}

async fn run_iteration(
    config: &Config,
    statsd_buf: Arc<RwLock<String>>,
) -> Result<IndexMap<String, MetricValue>> {
    let mut sub_config: Config = config.clone();
    let json_config = serde_yaml::to_string(&config)?;
    sub_config.env.insert("SIRUN_ITERATION".into(), json_config);
    let service = run_service(&sub_config)?;
    run_setup(&sub_config).await?;
    let mut child = run_cmd(
        &env::args().take(1).collect::<Vec<String>>(),
        &sub_config.env,
    )?;
    let status = child.status().await?;
    let status = status.code().expect("no exit code");
    if status != 0 && status <= 128 {
        exit(status);
    }
    let metrics = get_statsd_metrics(statsd_buf).await?;

    run_teardown(&config).await?;
    if let Some(mut service) = service {
        service.kill()?;
    }

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
    let statsd_addr = format!("127.0.0.1:{}", env::var("SIRUN_STATSD_PORT")?);
    sock.send_to(buf.as_bytes(), &statsd_addr).await?;
    if let Some(instructions) = metrics.remove("instructions") {
        sock.send_to(format!("instructions:{}|g\n", instructions.as_f64()).as_bytes(), &statsd_addr).await?;
    }
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
