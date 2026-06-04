// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use assert_cmd;
use predicates::prelude::*;
use serial_test::serial;
use std::path::PathBuf;

macro_rules! run {
    ($file:expr) => {
        assert_cmd::Command::cargo_bin("sirun").unwrap().arg($file)
    };
}

macro_rules! json_has {
    ($name:expr, $blk:expr) => {
        run!($name)
            .env("SIRUN_NO_STDIO", "1")
            .assert()
            .success()
            .stdout(predicate::function(|out| {
                let val = serde_yaml::from_str::<serde_yaml::Value>(out).unwrap();
                let val = val.as_mapping().unwrap();
                $blk(val)
            }));
    };
}

#[test]
#[serial]
fn simple_json() {
    run!("examples/simple.json").assert().success();
}

#[test]
#[serial]
fn wall_and_cpu_pct() {
    json_has!("examples/simple.json", move |map: &serde_yaml::Mapping| {
        let map = map
            .get(&"iterations".into())
            .unwrap()
            .as_sequence()
            .unwrap();
        let map = map.get(0).unwrap().as_mapping().unwrap();
        let wall_time = map.get(&"wall.time".into()).unwrap().as_f64().unwrap();
        let stime = map.get(&"system.time".into()).unwrap().as_f64().unwrap();
        let utime = map.get(&"user.time".into()).unwrap().as_f64().unwrap();
        let pct = map
            .get(&"cpu.pct.wall.time".into())
            .unwrap()
            .as_f64()
            .unwrap();
        (pct - ((stime + utime) * 100.0 / wall_time)).abs() < f64::EPSILON
    });
}

#[test]
#[serial]
fn simple_yml() {
    run!("examples/simple.yml").assert().success();
}

#[test]
#[serial]
fn simple_name_env() {
    run!("examples/simple.json")
        .env("SIRUN_NAME", "test test")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\":\"test test\""));
}

#[test]
#[serial]
fn simple_name() {
    run!("examples/simple-name.json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\":\"test test\""));
}

#[test]
#[serial]
fn simple_version() {
    run!("examples/simple.json")
        .env("GIT_COMMIT_HASH", "123abc")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"version\":\"123abc\""));
}

#[test]
#[serial]
fn no_setup() {
    run!("examples/no-setup.json").assert().success();
}

#[test]
#[serial]
fn teardown() {
    run!("examples/teardown.json")
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "the test was run\na teardown was run",
        ));
}

#[test]
#[serial]
fn variants() {
    run!("./examples/variants.json")
        .env("SIRUN_VARIANT", "0")
        .assert()
        .success()
        .stdout(predicate::str::contains("variant 0"));
    run!("./examples/variants.json")
        .env("SIRUN_VARIANT", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("variant 1"));
    run!("./examples/variants.json")
        .env("SIRUN_VARIANT", "1")
        .env("SIRUN_NO_STDIO", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"variant\":\"1\""));
}

#[test]
#[serial]
fn all_variants() {
    run!("./examples/variants.json")
        .assert()
        .success()
        .stdout(predicate::str::contains("variant 0").and(predicate::str::contains("variant 1")));
}

#[test]
#[serial]
fn timeout() {
    run!("examples/timeout.json").assert().failure();
}

#[test]
#[serial]
fn env() {
    run!("./examples/env.json")
        .env("SIRUN_VARIANT", "0")
        .assert()
        .success()
        .stdout(predicate::str::contains("something zero"));
    run!("./examples/env.json")
        .env("SIRUN_VARIANT", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("something one"));
}

#[test]
#[serial]
fn sigint() {
    run!("./examples/sigint.json")
        .assert()
        .success()
        .stdout(predicate::str::contains("user.time"));
}

#[test]
#[serial]
fn stdio() {
    run!("./examples/simple.json")
        .env("SIRUN_NO_STDIO", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("setup was run").not());
}

#[test]
#[serial]
fn iterations() {
    run!("./examples/iterations.json")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "you should see this\nyou should see this\nyou should see this",
        ));
    json_has!("./examples/iterations.json", |map: &serde_yaml::Mapping| {
        map.get(&"iterations".into())
            .unwrap()
            .as_sequence()
            .unwrap()
            .len()
            == 20
    });
}

#[test]
#[serial]
fn iterations_nohup() {
    run!("./examples/iterations-nohup.json").assert().success();
}

#[test]
#[serial]
fn iterations_not_cumulative() {
    json_has!("./examples/iterations.json", |map: &serde_yaml::Mapping| {
        let iter = map
            .get(&"iterations".into())
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter();
        let mut prev = 0.0;
        for iteration in iter {
            let val = iteration
                .as_mapping()
                .unwrap()
                .get(&"user.time".into())
                .unwrap()
                .as_f64()
                .unwrap();
            if val < prev {
                return true;
            } else {
                prev = val;
            }
        }
        false
    });
}

#[test]
#[serial]
fn long() {
    json_has!("./examples/long.json", |map: &serde_yaml::Mapping| {
        map.get(&"iterations".into())
            .unwrap()
            .get(0)
            .unwrap()
            .as_mapping()
            .unwrap()
            .get(&"user.time".into())
            .unwrap()
            .as_f64()
            .unwrap()
            > 1000000.0
    });
}

#[test]
#[serial]
fn summarize() {
    let mut in_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    in_path.push("tests/fixtures/summary/in.ndjson");
    let mut out_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    out_path.push("tests/fixtures/summary/out.json");
    let expected_output: &str = &std::fs::read_to_string(out_path).unwrap();

    run!("--summarize")
        .write_stdin(std::fs::read(in_path).unwrap())
        .assert()
        .success()
        .stdout(predicate::eq(expected_output));
}

#[test]
#[serial]
fn assigned_port() {
    run!("./examples/assigned-port.json")
        .env("SIRUN_STATSD_PORT", "8125")
        .env("SIRUN_NO_STDIO", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"udp.data\":8125"));
}

#[test]
#[serial]
#[cfg(target_os = "linux")]
fn insctrution_counts() {
    if caps::has_cap(None, caps::CapSet::Permitted, caps::Capability::CAP_SYS_PTRACE).unwrap() {
        json_has!("./examples/instructions.json", move |map: &serde_yaml::Mapping| {
            let count = map.get(&"instructions".into())
                .unwrap()
                .as_f64()
                .unwrap();
            count > 0.0
        });
    }
}

#[test]
#[serial]
fn service() {
    run!("./examples/service.json").assert().success();
}

#[test]
#[serial]
fn ready_signal_resets_wall_time() {
    json_has!(
        "./examples/ready-signal.json",
        |map: &serde_yaml::Mapping| {
            let wall_time = map
                .get(&"iterations".into())
                .unwrap()
                .as_sequence()
                .unwrap()[0]
                .as_mapping()
                .unwrap()
                .get(&"wall.time".into())
                .unwrap()
                .as_f64()
                .unwrap();
            // Startup is 500ms; post-ready work is ~0ms.
            // With ready signal: wall.time << 500_000μs.
            // Threshold of 200_000μs gives generous headroom.
            wall_time < 200_000.0
        }
    );
}

#[test]
#[serial]
fn ready_signal_fallback_when_no_signal() {
    // App exits without writing to SIRUN_READY_FD — full timing used.
    json_has!("./examples/simple.json", |map: &serde_yaml::Mapping| {
        // simple.json has no ready signal; we just verify it still runs normally.
        map.get(&"iterations".into())
            .unwrap()
            .as_sequence()
            .unwrap()
            .len()
            == 1
    });
}

#[test]
#[serial]
fn ready_signal_cpu_pct_bounded() {
    json_has!(
        "./examples/ready-signal-cpu.json",
        |map: &serde_yaml::Mapping| {
            let iter = map
                .get(&"iterations".into())
                .unwrap()
                .as_sequence()
                .unwrap()[0]
                .as_mapping()
                .unwrap();
            let cpu_pct = iter
                .get(&"cpu.pct.wall.time".into())
                .unwrap()
                .as_f64()
                .unwrap();
            // Without the fix, user.time covers the full CPU-intensive startup
            // while wall.time covers only the post-ready period (near zero),
            // making cpu.pct.wall.time >> 100%. With the fix it must stay <= 100%.
            cpu_pct <= 100.0
        }
    );
}

#[test]
#[serial]
fn ready_signal_cpu_pct_100x() {
    let mut passes = 0u32;
    let mut failures = 0u32;
    let mut cpu_pcts: Vec<f64> = Vec::new();

    for _ in 0..100 {
        let output = assert_cmd::Command::cargo_bin("sirun")
            .unwrap()
            .arg("./examples/ready-signal-cpu.json")
            .env("SIRUN_NO_STDIO", "1")
            .output()
            .unwrap();

        if output.status.success() {
            if let Ok(val) =
                serde_yaml::from_slice::<serde_yaml::Value>(&output.stdout)
            {
                if let Some(cpu_pct) = val
                    .as_mapping()
                    .and_then(|m| m.get(&"iterations".into()))
                    .and_then(|v| v.as_sequence())
                    .and_then(|s| s.get(0))
                    .and_then(|v| v.as_mapping())
                    .and_then(|m| m.get(&"cpu.pct.wall.time".into()))
                    .and_then(|v| v.as_f64())
                {
                    cpu_pcts.push(cpu_pct);
                    if cpu_pct <= 100.0 {
                        passes += 1;
                    } else {
                        failures += 1;
                    }
                } else {
                    failures += 1;
                }
            } else {
                failures += 1;
            }
        } else {
            failures += 1;
        }
    }

    let max_pct = cpu_pcts
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    eprintln!(
        "spawn_blocking 100x: {}/100 passed, \
         max cpu.pct.wall.time = {:.1}%",
        passes, max_pct
    );
    assert!(
        passes >= 95,
        "spawn_blocking: {}/100 passed (max cpu.pct.wall.time = {:.1}%)",
        passes,
        max_pct
    );
}
