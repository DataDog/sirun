use crate::metric_value::*;
use anyhow::*;
use async_std::{
    net::UdpSocket,
    sync::{Arc, Barrier, RwLock},
};
use std::{collections::HashMap, env::set_var};

pub(crate) async fn statsd_listener(
    barrier: Arc<Barrier>,
    statsd_buf: Arc<RwLock<String>>,
) -> Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    let port = socket.local_addr()?.port();
    set_var("SIRUN_STATSD_PORT", format!("{}", port));
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
) -> Result<HashMap<String, MetricValue>> {
    let mut metrics = HashMap::new();
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
