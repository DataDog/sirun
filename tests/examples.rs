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
    run!("--summarize")
        .write_stdin(std::fs::read(in_path).unwrap())
        .output()
        .expect(&std::fs::read_to_string(out_path).unwrap());
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
