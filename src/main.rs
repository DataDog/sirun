use async_std::{
    net::UdpSocket,
    process::Command,
    sync::{Arc, Barrier, RwLock},
    task,
};
use libc;
use std::{collections::HashMap, env, io::Result, mem};

async fn statsd_listener(barrier: Arc<Barrier>, statsd_buf: Arc<RwLock<String>>) -> Result<String> {
    let socket: UdpSocket = UdpSocket::bind("127.0.0.1:8125").await?;
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
        let metric = line.split("|").nth(0);
        if metric.is_none() {
            continue;
        }
        let metric: Vec<&str> = metric.unwrap().split(":").collect();
        if metric.len() < 2 {
            continue;
        }
        metrics.insert(metric[0].into(), metric[1].into());
    }
}

fn get_kernel_metrics(metrics: &mut HashMap<String, String>) {
    let mut data = unsafe { mem::MaybeUninit::uninit().assume_init() };
    if unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut data) } == -1 {
        return;
    }
    metrics.insert("max.res.size".into(), format!("{}", data.ru_maxrss));
    metrics.insert("user.time".into(), format!("{}", data.ru_utime.tv_usec));
    metrics.insert("system.time".into(), format!("{}", data.ru_stime.tv_usec));
}

#[async_std::main]
async fn main() -> Result<()> {
    let statsd_started = Arc::new(Barrier::new(2));
    let statsd_buf = Arc::new(RwLock::new(String::new()));
    task::spawn(statsd_listener(statsd_started.clone(), statsd_buf.clone()));

    statsd_started.wait().await; // waits for socket to be listening

    let command = env::args().nth(1).unwrap();
    let args = env::args().skip(2);
    Command::new(command).args(args).status().await.unwrap();

    let mut metrics = HashMap::new();
    if let Ok(hash) = env::var("GIT_COMMIT_HASH") {
        metrics.insert("version".into(), hash.into());
    }
    if let Ok(name) = env::var("SIRUN_NAME") {
        metrics.insert("name".into(), name.into());
    }
    get_kernel_metrics(&mut metrics);
    get_statsd_metrics(&mut metrics, statsd_buf.read().await.clone());

    println!("results: {:?}", metrics);

    Ok(())
}
