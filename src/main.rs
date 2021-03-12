// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use async_std::{
    net::UdpSocket,
    process::Command,
    sync::{Arc, Barrier, RwLock},
    task,
};
use nix::{
    libc::{getrusage, RUSAGE_CHILDREN},
    unistd,
};
use std::{
    collections::HashMap,
    env,
    ffi::{CStr, CString},
    io::Result,
    mem,
    process::exit,
};

mod config;

use config::get_config;

async fn statsd_listener(barrier: Arc<Barrier>, statsd_buf: Arc<RwLock<String>>) -> Result<String> {
    let socket = UdpSocket::bind("127.0.0.1:8124").await;
    let socket = match socket {
        Ok(s) => s,
        Err(error) => panic!("Cannot bind to 127.0.0.1:8124: {}", error),
    };
    barrier.wait().await; // indicates to main task that socket is listening

    loop {
        let mut buf = vec![0u8; 4096];
        let (recv, _peer) = socket.recv_from(&mut buf).await?;

        let datum = String::from_utf8(buf[..recv].into()).unwrap_or_else(|_| String::new());
        statsd_buf.write().await.push_str(&datum);
    }
}

fn get_statsd_metrics(metrics: &mut HashMap<String, String>, udp_data: String) {
    let lines = udp_data.trim().lines();
    for line in lines {
        let metric = line.split('|').next();
        if metric.is_none() {
            continue;
        }
        let metric: Vec<&str> = metric.unwrap().split(':').collect();
        if metric.len() < 2 {
            continue;
        }
        metrics.insert(metric[0].into(), metric[1].into());
    }
}

fn get_kernel_metrics(metrics: &mut HashMap<String, String>) {
    let mut data = unsafe { mem::MaybeUninit::uninit().assume_init() };
    if unsafe { getrusage(RUSAGE_CHILDREN, &mut data) } == -1 {
        return;
    }
    metrics.insert("max.res.size".into(), format!("{}", data.ru_maxrss));
    metrics.insert("user.time".into(), format!("{}", data.ru_utime.tv_usec));
    metrics.insert("system.time".into(), format!("{}", data.ru_stime.tv_usec));
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
            .status()
            .await
            .unwrap()
            .code()
            .unwrap();
        if code != 0 {
            task::sleep(std::time::Duration::from_secs(1)).await;
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
    let _ = unistd::execvp(CString::new(filename).unwrap().as_c_str(), &args);
    // This process stops running past here.
}

async fn test_timeout(timeout: u64) {
    task::sleep(std::time::Duration::from_secs(timeout)).await;
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
    task::spawn(statsd_listener(statsd_started.clone(), statsd_buf.clone()));

    statsd_started.wait().await; // waits for socket to be listening

    if let Some(timeout) = config.timeout {
        task::spawn(test_timeout(timeout));
    }

    let command = config.run[0].clone();
    let args = config.run.iter().skip(1);
    let status = Command::new(command)
        .args(args)
        .envs(&config.env)
        .status()
        .await;
    if let Err(err) = status {
        eprintln!("Error running test: {}", err);
        exit(1);
    }
    let status = status.unwrap().code().unwrap();
    if status != 0 {
        eprintln!("Test exited with code {}, so aborting test.", status);
        exit(status);
    }

    let mut metrics = HashMap::new();
    if let Ok(hash) = env::var("GIT_COMMIT_HASH") {
        metrics.insert("version".into(), hash);
    }
    if let Ok(name) = env::var("SIRUN_NAME") {
        metrics.insert("name".into(), name);
    }
    get_kernel_metrics(&mut metrics);
    get_statsd_metrics(&mut metrics, statsd_buf.read().await.clone());

    println!("results: {:?}", metrics);
    exit(0);
}
