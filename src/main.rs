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
use nix::{
    libc::{getrusage, RUSAGE_CHILDREN},
    unistd::execvp,
};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::HashMap,
    env,
    ffi::{CStr, CString},
    mem::MaybeUninit,
    process::exit,
    time::Duration,
};

mod config;
use config::*;

#[derive(Serialize, Clone)]
#[serde(untagged)]
enum MetricValue {
    Str(String),
    Num(f64),
    Arr(Vec<HashMap<String, MetricValue>>),
}

impl From<String> for MetricValue {
    fn from(string: String) -> Self {
        MetricValue::Str(string)
    }
}

macro_rules! num_type {
    ($type:ty) => {
        impl From<$type> for MetricValue {
            fn from(num: $type) -> Self {
                MetricValue::Num(num as f64)
            }
        }
    };
}
num_type!(i32);
num_type!(i64);
num_type!(f64);

async fn statsd_listener(barrier: Arc<Barrier>, statsd_buf: Arc<RwLock<String>>) -> Result<String> {
    let socket = UdpSocket::bind("127.0.0.1:8125").await;
    let socket = match socket {
        Ok(s) => s,
        Err(error) => panic!("Cannot bind to 127.0.0.1:8125: {}", error),
    };
    barrier.wait().await; // indicates to main task that socket is listening

    loop {
        let mut buf = vec![0u8; 4096];
        let (recv, _peer) = socket.recv_from(&mut buf).await?;

        let datum = String::from_utf8(buf[..recv].into()).unwrap_or_else(|_| String::new());
        statsd_buf.write().await.push_str(&datum);
    }
}

fn get_statsd_metrics(metrics: &mut HashMap<String, MetricValue>, udp_data: String) -> Result<()> {
    let lines = udp_data.trim().lines();
    for line in lines {
        let metric: Vec<&str> = match line.split('|').next() {
            None => continue,
            Some(metric) => metric.split(':').collect(),
        };
        if metric.len() < 2 {
            continue;
        }
        metrics.insert(metric[0].into(), metric[1].parse::<f64>()?.into());
    }
    Ok(())
}

fn ms_from_timeval(tv: nix::libc::timeval) -> f64 {
    let seconds = tv.tv_sec;
    let ms = tv.tv_usec as i64;
    let val = seconds * 1000000 + ms;
    val as f64
}

fn get_and_assign_diff(
    prev: &HashMap<String, MetricValue>,
    metrics: &mut HashMap<String, MetricValue>,
    name: &str,
    val: f64,
) -> f64 {
    let prev_val: f64 = if let Some(val) = prev.get(name.into()) {
        match val {
            MetricValue::Num(val) => *val,
            _ => 0.0,
        }
    } else {
        0.0
    };
    let result = val - prev_val;
    metrics.insert(name.into(), result.into());
    result
}

fn get_kernel_metrics(
    wall_time: f64,
    prev: &HashMap<String, MetricValue>,
    metrics: &mut HashMap<String, MetricValue>,
) {
    let data = unsafe {
        let mut data = MaybeUninit::zeroed().assume_init();
        if getrusage(RUSAGE_CHILDREN, &mut data) == -1 {
            return;
        }
        data
    };
    get_and_assign_diff(prev, metrics, "max.res.size", data.ru_maxrss as f64);

    let utime = get_and_assign_diff(
        prev,
        metrics,
        "user.time",
        ms_from_timeval(data.ru_utime) as f64,
    );
    let stime = get_and_assign_diff(
        prev,
        metrics,
        "system.time",
        ms_from_timeval(data.ru_stime) as f64,
    );
    let pct = (utime + stime) * 100.0 / wall_time;
    metrics.insert("cpu.pct.wall.time".into(), pct.into());
}

fn get_stdio() -> Stdio {
    match env::var("SIRUN_NO_STDIO") {
        Ok(_) => Stdio::null(),
        Err(_) => Stdio::inherit(),
    }
}

async fn run_setup(
    setup: &[String],
    config_file: &str,
    env: &HashMap<String, String>,
) -> Result<()> {
    let mut code: i32 = 1;
    let mut attempts: u8 = 0;
    while code != 0 {
        if attempts == 100 {
            bail!("setup script did not complete successfully. aborting.");
        }
        let command = setup[0].clone();
        let args = setup.iter().skip(1);
        code = Command::new(command)
            .args(args)
            .envs(env.clone())
            .stdout(get_stdio())
            .stderr(get_stdio())
            .status()
            .await?
            .code()
            .expect("no exit code");
        if code != 0 {
            sleep(Duration::from_secs(1)).await;
            attempts += 1;
        }
    }

    // now run in a new process with execvp, skipping setup
    env::set_var("SIRUN_SKIP_SETUP", "true");
    let args: Vec<_> = env::args()
        .map(|s| CString::new(s.as_bytes()).unwrap())
        .collect();
    let args: Vec<&CStr> = args.iter().map(|s| s.as_c_str()).collect();
    let _ = execvp(CString::new(config_file).unwrap().as_c_str(), &args);
    // This process stops running past here.

    Ok(())
}

async fn test_timeout(timeout: u64) {
    sleep(std::time::Duration::from_secs(timeout)).await;
    eprintln!("Timeout of {} seconds exceeded.", timeout);
    exit(1);
}

async fn run_iteration(
    config: &Config,
    prev: &HashMap<String, MetricValue>,
    mut metrics: &mut HashMap<String, MetricValue>,
    statsd_buf: Arc<RwLock<String>>,
) -> Result<()> {
    if let Some(timeout) = config.timeout {
        spawn(test_timeout(timeout));
    }

    let command = config.run[0].clone();
    let args = config.run.iter().skip(1);
    let start_time = std::time::Instant::now();
    let status = Command::new(command)
        .args(args)
        .envs(&config.env)
        .stdout(get_stdio())
        .stderr(get_stdio())
        .status()
        .await?;
    let duration = start_time.elapsed().as_micros();
    metrics.insert("wall.time".to_owned(), (duration as f64).into());
    let status = status.code().expect("no exit code");
    if status != 0 && status <= 128 {
        eprintln!("Test exited with code {}, so aborting test.", status);
        exit(status);
    }
    get_kernel_metrics(duration as f64, &prev, &mut metrics);
    get_statsd_metrics(&mut metrics, statsd_buf.read().await.clone())?;
    statsd_buf.write().await.clear();
    Ok(())
}

#[async_std::main]
async fn main() -> Result<()> {
    let config_file = env::args().nth(1).expect("missing file argument");
    let config = get_config(&config_file)?;
    if let Some(setup) = &config.setup {
        if env::var("SIRUN_SKIP_SETUP").is_err() {
            run_setup(&setup, &config_file, &config.env).await?;
        }
    }

    let statsd_started = Arc::new(Barrier::new(2));
    let statsd_buf = Arc::new(RwLock::new(String::new()));
    spawn(statsd_listener(statsd_started.clone(), statsd_buf.clone()));
    statsd_started.wait().await; // waits for socket to be listening

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();
    let mut prev = HashMap::new();
    if config.iterations == 1 {
        run_iteration(&config, &prev, &mut metrics, statsd_buf.clone()).await?;
    } else {
        let mut iterations = Vec::new();
        for _ in 0..config.iterations {
            let mut iteration_metrics = HashMap::new();
            run_iteration(&config, &prev, &mut iteration_metrics, statsd_buf.clone()).await?;
            prev = iteration_metrics.clone();
            iterations.push(iteration_metrics);
        }
        metrics.insert("iterations".into(), MetricValue::Arr(iterations));
    }

    if config.cachegrind {
        let command = "valgrind";
        let mut args = vec!["--tool=cachegrind".to_owned()];
        args.append(&mut config.run.clone());
        let output = Command::new(command)
            .args(args)
            .envs(&config.env)
            .output()
            .await?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let instructions: f64 = stderr
            .trim()
            .lines()
            .filter(|x| x.contains("I   refs:"))
            .next()
            .expect("bad cachegrind output")
            .trim()
            .split_whitespace()
            .last()
            .expect("bad cachegrind output")
            .replace(",", "")
            .parse()?;
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
