// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use anyhow::*;
use async_std::io;
use std::collections::HashMap;

use crate::metric_value::*;

fn mean(items: &Vec<f64>) -> f64 {
    let len = items.len() as f64;
    let total: f64 = items.iter().sum();
    total / len
}

fn stddev(m: f64, items: &Vec<f64>) -> f64 {
    mean(&items.iter().map(|x| f64::powf(x - m, 2.0)).collect()).sqrt()
}

fn summary(iterations: &Vec<MetricValue>) -> MetricValue {
    let mut stats: HashMap<String, Vec<f64>> = HashMap::new();
    for iteration in iterations {
        let iteration = iteration.as_map();
        for (k, v) in iteration {
            let stat = match stats.get_mut(k) {
                Some(k) => k,
                None => {
                    stats.insert(k.clone(), Vec::new());
                    stats.get_mut(k).unwrap()
                }
            };
            stat.push(v.clone().as_f64());
        }
    }
    let mut result = HashMap::new();
    for (name, items) in stats {
        let mut statistics = HashMap::new();
        let m = mean(&items);
        let s = stddev(m, &items);
        statistics.insert("mean".to_owned(), MetricValue::Num(m));
        statistics.insert("stddev".to_owned(), MetricValue::Num(s));
        statistics.insert("stddev_pct".to_owned(), MetricValue::Num((s / m) * 100.0));
        statistics.insert(
            "min".to_owned(),
            MetricValue::Num(items.iter().fold(f64::INFINITY, |a, &b| a.min(b))),
        );
        statistics.insert(
            "max".to_owned(),
            MetricValue::Num(items.iter().fold(f64::INFINITY, |a, &b| a.max(b))),
        );

        result.insert(name, MetricValue::Map(statistics));
    }

    MetricValue::Map(result)
}

pub(crate) async fn summarize() -> Result<()> {
    let stdin = io::stdin();
    let mut line = String::new();
    let mut result_data: MetricMap = HashMap::new();
    while stdin.read_line(&mut line).await? != 0 {
        if let Ok(mut json_data) = serde_json::from_str::<MetricMap>(&line) {
            let name = match json_data.get("name") {
                Some(name) => name.clone().as_string(),
                None => {
                    line = String::new();
                    continue;
                }
            };
            json_data.remove("name");
            let variant = match json_data.get("variant") {
                Some(variant) => variant.clone().as_string(),
                None => {
                    line = String::new();
                    continue;
                }
            };
            json_data.remove("variant");
            let name_data: &mut MetricMap = match result_data.get_mut(&name) {
                Some(data) => data.as_map_mut(),
                None => {
                    result_data.insert(name.to_owned(), MetricValue::Map(HashMap::new()));
                    result_data.get_mut(&name).unwrap().as_map_mut()
                }
            };

            if let Some((_, iterations)) = json_data.remove_entry("iterations") {
                json_data.insert("summary".to_owned(), summary(&iterations.as_vec()));
            } else {
                line = String::new();
                continue;
            }
            json_data.remove("iterations");
            name_data.insert(variant, MetricValue::Map(json_data));
        };
        line = String::new();
    }
    println!("{}", serde_json::to_string_pretty(&result_data).unwrap());
    Ok(())
}
