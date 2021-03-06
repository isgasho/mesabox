//
// Copyright (c) 2018, The MesaLock Linux Project Contributors
// All rights reserved.
//
// This work is licensed under the terms of the BSD 3-Clause License.
// For a copy, see the LICENSE file.
//

use assert_cmd::prelude::*;
use std::process::Command;
use std::time::{Duration, Instant};

const NAME: &str = "sleep";

const SLEEP_TIME: f32 = 5.75;
const DIFF: f32 = 1.5;

#[test]
fn test_one_param() {
    let now = Instant::now();

    new_cmd!()
        .args(&[SLEEP_TIME.to_string()])
        .assert()
        .success();

    validate_duration(now.elapsed(), SLEEP_TIME);
}

#[test]
fn test_many_params() {
    let now = Instant::now();

    new_cmd!()
        .args(&[(SLEEP_TIME / 4.0).to_string(), (SLEEP_TIME / 2.0).to_string(), (SLEEP_TIME / 8.0).to_string(), (SLEEP_TIME / 8.0).to_string()])
        .assert()
        .success();

    validate_duration(now.elapsed(), SLEEP_TIME);
}

fn validate_duration(duration: Duration, sleep_time: f32) {
    let time = duration.as_secs() as f32 + duration.subsec_nanos() as f32 / 1_000_000_000.0;
    if (time - sleep_time).abs() > DIFF {
        panic!("slept for too long ({} secs instead of {} secs)", time, sleep_time);
    }
}
