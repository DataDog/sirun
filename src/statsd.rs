use crate::metric_value::*;
use anyhow::*;
use async_std::{
    net::UdpSocket,
    sync::{Arc, Barrier, RwLock},
};
use std::env;
use indexmap::IndexMap;

pub(crate) async fn statsd_listener(
    barrier: Arc<Barrier>,
    statsd_buf: Arc<RwLock<String>>,
) -> Result<()> {
    // If the env var is set, we'll use it, otherwise use 0 to grab an available port.
    let port: u16 = env::var("SIRUN_STATSD_PORT").map_or(0, |p| p.parse().unwrap_or(0));
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", port)).await;
    let socket = match socket {
        Ok(s) => s,
        Err(error) => panic!("Cannot bind to 127.0.0.1:{}: {}", port, error),
    };
    let port = socket.local_addr()?.port();
    env::set_var("SIRUN_STATSD_PORT", format!("{}", port));
    barrier.wait().await; // indicates to main task that socket is listening

    loop {
        let mut buf = vec![0u8; 4096];
        let (recv, _peer) = socket.recv_from(&mut buf).await?;

        let datum = String::from_utf8(buf[..recv].into()).unwrap_or_else(|_| String::new());
        statsd_buf.write().await.push_str(&datum);
    }
}

pub(crate) async fn get_statsd_metrics(
    udp_data: Arc<RwLock<String>>,
) -> Result<IndexMap<String, MetricValue>> {
    let mut metrics = IndexMap::new();
    let udp_string = udp_data.read().await.clone();
    let lines = udp_string.trim().lines();
    udp_data.write().await.clear();
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
    Ok(metrics)
}
