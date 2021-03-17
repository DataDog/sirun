// Unless explicitly stated otherwise all files in this repository are licensed
// under the MIT/Apache-2.0 License, at your convenience
//
// This product includes software developed at Datadog (https://www.datadoghq.com/). Copyright 2021 Datadog, Inc.

use assert_cmd;
use predicates::prelude::*;
use serial_test::serial;

macro_rules! run {
    ($file:expr) => {
        assert_cmd::Command::cargo_bin("sirun").unwrap().arg($file)
    };
}

#[test]
#[serial]
fn simple_json() {
    run!("examples/simple.json").assert().success();
}

#[test]
#[serial]
fn simple_yml() {
    run!("examples/simple.yml").assert().success();
}

#[test]
#[serial]
fn simple_name() {
    run!("examples/simple.json")
        .env("SIRUN_NAME", "test test")
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

#[cfg(target_arch = "linux")]
#[test]
#[serial]
fn cachegrind() {
    run!("./examples/cachegrind.json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"instructions\":"));
}
