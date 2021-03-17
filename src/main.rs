// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

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
    io::Result,
    mem::MaybeUninit,
    process::exit,
    time::Duration,
};

mod config;
use config::get_config;

#[derive(Serialize)]
#[serde(untagged)]
enum MetricValue {
    Str(String),
    Num(f64),
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

fn get_statsd_metrics(metrics: &mut HashMap<String, MetricValue>, udp_data: String) {
    let lines = udp_data.trim().lines();
    for line in lines {
        let metric: Vec<&str> = match line.split('|').next() {
            None => continue,
            Some(metric) => metric.split(':').collect(),
        };
        if metric.len() < 2 {
            continue;
        }
        metrics.insert(metric[0].into(), metric[1].parse::<f64>().unwrap().into());
    }
}

fn get_kernel_metrics(metrics: &mut HashMap<String, MetricValue>) {
    let data = unsafe {
        let mut data = MaybeUninit::zeroed().assume_init();
        if getrusage(RUSAGE_CHILDREN, &mut data) == -1 {
            return;
        }
        data
    };
    metrics.insert("max.res.size".into(), data.ru_maxrss.into());
    metrics.insert("user.time".into(), data.ru_utime.tv_usec.into());
    metrics.insert("system.time".into(), data.ru_stime.tv_usec.into());
}

fn get_stdio() -> Stdio {
    match env::var("SIRUN_NO_STDIO") {
        Ok(_) => Stdio::null(),
        Err(_) => Stdio::inherit(),
    }
}

async fn run_setup(setup: &[String], env: &HashMap<String, String>) {
    let mut code: i32 = 1;
    let mut attempts: u8 = 0;
    while code != 0 {
        if attempts == 100 {
            eprintln!("setup script did not complete successfully. aborting.");
            exit(1);
        }
        let command = setup[0].clone();
        let args = setup.iter().skip(1);
        code = Command::new(command)
            .args(args)
            .envs(env.clone())
            .stdout(get_stdio())
            .stderr(get_stdio())
            .status()
            .await
            .unwrap()
            .code()
            .unwrap();
        if code != 0 {
            sleep(Duration::from_secs(1)).await;
            attempts += 1;
        }
    }

    // now run in a new process with execvp, skipping setup
    env::set_var("SIRUN_SKIP_SETUP", "true");
    let filename = env::args().next().unwrap();
    let args: Vec<_> = env::args()
        .map(|s| CString::new(s.as_bytes()).unwrap())
        .collect();
    let args: Vec<&CStr> = args.iter().map(|s| s.as_c_str()).collect();
    let _ = execvp(CString::new(filename).unwrap().as_c_str(), &args);
    // This process stops running past here.
}

async fn test_timeout(timeout: u64) {
    sleep(std::time::Duration::from_secs(timeout)).await;
    eprintln!("Timeout of {} seconds exceeded.", timeout);
    exit(1);
}

#[async_std::main]
async fn main() {
    let config = get_config(env::args().nth(1).unwrap());
    if config.is_err() {
        eprintln!("{:?}", config.err().unwrap());
        exit(1);
    }
    let config = config.unwrap();
    if let Some(setup) = config.setup {
        if env::var("SIRUN_SKIP_SETUP").is_err() {
            run_setup(&setup, &config.env).await;
        }
    }
    let statsd_started = Arc::new(Barrier::new(2));
    let statsd_buf = Arc::new(RwLock::new(String::new()));
    spawn(statsd_listener(statsd_started.clone(), statsd_buf.clone()));

    statsd_started.wait().await; // waits for socket to be listening

    if let Some(timeout) = config.timeout {
        spawn(test_timeout(timeout));
    }

    let command = config.run[0].clone();
    let args = config.run.iter().skip(1);
    let status = Command::new(command)
        .args(args)
        .envs(&config.env)
        .stdout(get_stdio())
        .stderr(get_stdio())
        .status()
        .await;
    if let Err(err) = status {
        eprintln!("Error running test: {}", err);
        exit(1);
    }
    let status = status.unwrap().code().unwrap();
    if status != 0 && status <= 128 {
        eprintln!("Test exited with code {}, so aborting test.", status);
        exit(status);
    }

    let mut metrics: HashMap<String, MetricValue> = HashMap::new();

    if config.cachegrind {
        let command = "valgrind";
        let mut args = vec!["--tool=cachegrind".to_owned()];
        args.append(&mut config.run.clone());
        let output = Command::new(command)
            .args(args)
            .envs(&config.env)
            .output()
            .await
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let instructions: f64 = stderr
            .trim()
            .lines()
            .filter(|x| x.contains("I   refs:"))
            .nth(0)
            .unwrap()
            .trim()
            .split_whitespace()
            .last()
            .unwrap()
            .replace(",", "")
            .parse()
            .unwrap();
        metrics.insert("instructions".into(), instructions.into());
        eprintln!("got valgrind output: {}", stderr);
    }

    if let Ok(hash) = env::var("GIT_COMMIT_HASH") {
        metrics.insert("version".into(), hash.into());
    }
    if let Ok(name) = env::var("SIRUN_NAME") {
        metrics.insert("name".into(), name.into());
    }
    get_kernel_metrics(&mut metrics);
    get_statsd_metrics(&mut metrics, statsd_buf.read().await.clone());

    println!("{}", json!(metrics).to_string());
    exit(0);
}
