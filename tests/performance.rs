//! Performance envelope: the measurement run and the CI regression guard.
//!
//! `cargo test --release --test performance -- --ignored measure_the_reference_scenario --nocapture` prints the
//! table published in `docs/compatibility.md`. CI separately runs the ignored
//! timing guard with a release build. The ordinary suite, including coverage,
//! checks the Git subprocess budget without a wall-clock assertion.

mod common;

use common::scenario::{Scenario, ScenarioSpec};
use std::time::{Duration, Instant};

/// How many times each command is timed. The median is reported.
const RUNS: usize = 3;

/// Alternating baseline/scenario pairs for the release-build timing guard.
const TIMING_PAIRS: usize = 5;

/// Ratio of no-op `sync` on the smoke scenario to no-op `sync` on a
/// single-source store, above which the test fails.
///
/// See the comment in [`noop_sync_should_not_regress_against_a_minimal_store`]
/// for how this number was chosen.
const NOOP_SYNC_RATIO_BOUND: f64 = 6.0;

/// Git subprocesses a no-op `sync` may spawn, as `fixed + per_source * sources`.
///
/// A no-op `sync` resolves the head commit of every enabled source, and for
/// every tracking team source additionally checks the working tree, reads
/// `core.sshCommand`, fetches, and compares against the upstream ref. That is
/// five calls per tracking source plus one `rev-parse HEAD` per source, so the
/// smoke scenario spawns 21 and the reference scenario 41. The bound below
/// allows 29 and 54 respectively: enough headroom for an added call, tight
/// enough that a re-clone or a second scan per source fails the test.
const NOOP_SYNC_GIT_CALLS_PER_SOURCE: usize = 5;
const NOOP_SYNC_GIT_CALLS_FIXED: usize = 4;

fn median(mut samples: Vec<Duration>) -> Duration {
    assert!(!samples.is_empty(), "median needs at least one sample");
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn run(scenario: &Scenario, arguments: &[&str]) -> Duration {
    let mut command = scenario.dalo();
    command.args(arguments);
    let start = Instant::now();
    let output = command.output().expect("dalo should run");
    let elapsed = start.elapsed();
    assert!(
        output.status.success(),
        "dalo {arguments:?} should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    elapsed
}

/// Runs one command with a `git` wrapper on `PATH` that records every call.
fn git_invocations(scenario: &Scenario, arguments: &[&str]) -> Vec<String> {
    let directory = scenario.scratch.join("git-log");
    std::fs::create_dir_all(&directory).expect("git log directory should be created");
    let logger = common::git_invocation_logger(&directory);
    scenario
        .dalo()
        .args(arguments)
        .env("PATH", &logger.path_env)
        .env("DALO_REAL_GIT", &logger.real_git)
        .env("DALO_GIT_INVOCATION_LOG", &logger.log)
        .assert()
        .success();
    logger.invocations()
}

/// Compare warm no-op syncs in an uninstrumented release build.
///
/// The 6x bound was chosen against measured ratios of 1.9–2.6 on an M1 Ultra.
/// It is a regression signal, not a hardware-independent guarantee: coverage
/// instrumentation, filesystem caches, and changing runner load affect the two
/// workloads differently. Keep this out of coverage/debug runs, alternate the
/// order of adjacent pairs, and retain every sample for diagnosis. The ordinary
/// suite independently enforces the deterministic Git subprocess budget.
#[test]
#[ignore = "timing guard; CI runs this explicitly with an uninstrumented release build"]
fn noop_sync_should_not_regress_against_a_minimal_store() {
    if cfg!(debug_assertions) {
        panic!("run the timing guard with --release");
    }
    let baseline = Scenario::build(ScenarioSpec::baseline());
    let scenario = Scenario::build(ScenarioSpec::smoke());
    run(&baseline, &["sync"]);
    run(&scenario, &["sync"]);
    // Warm the no-op paths separately from the first materialization.
    run(&baseline, &["sync"]);
    run(&scenario, &["sync"]);

    let mut ratios = Vec::new();
    for pair in 0..TIMING_PAIRS {
        let (baseline_noop, scenario_noop) = if pair % 2 == 0 {
            (run(&baseline, &["sync"]), run(&scenario, &["sync"]))
        } else {
            let scenario_noop = run(&scenario, &["sync"]);
            (run(&baseline, &["sync"]), scenario_noop)
        };
        let ratio = scenario_noop.as_secs_f64() / baseline_noop.as_secs_f64();
        println!(
            "pair {}: baseline={baseline_noop:?} scenario={scenario_noop:?} ratio={ratio:.2}x",
            pair + 1
        );
        ratios.push(ratio);
    }
    ratios.sort_by(f64::total_cmp);
    let ratio = ratios[ratios.len() / 2];

    assert!(
        ratio <= NOOP_SYNC_RATIO_BOUND,
        "no-op sync over {} sources / {} skills had a median ratio of \
         {ratio:.2}x relative to a single-source store; \
         the bound is {NOOP_SYNC_RATIO_BOUND:.1}x",
        scenario.spec.sources(),
        scenario.spec.skills(),
    );
}

#[test]
fn noop_sync_stays_within_git_subprocess_budget() {
    let scenario = Scenario::build(ScenarioSpec::smoke());
    run(&scenario, &["sync"]);
    let invocations = git_invocations(&scenario, &["sync"]);
    let allowed =
        NOOP_SYNC_GIT_CALLS_FIXED + NOOP_SYNC_GIT_CALLS_PER_SOURCE * scenario.spec.sources();
    assert!(
        invocations.len() <= allowed,
        "no-op sync spawned {} git subprocesses for {} sources, above the bound of {allowed}:\n{}",
        invocations.len(),
        scenario.spec.sources(),
        invocations.join("\n"),
    );
}

/// Prints the published envelope. Not part of the ordinary suite.
///
/// ```sh
/// cargo test --release --locked --test performance -- --ignored measure_the_reference_scenario --nocapture
/// ```
#[test]
#[ignore = "measurement run; builds the full reference store and prints a table"]
fn measure_the_reference_scenario() {
    let spec = ScenarioSpec::reference();
    let mut first_sync = Vec::new();
    let mut noop_sync = Vec::new();
    let mut status = Vec::new();
    let mut status_json = Vec::new();
    let mut doctor = Vec::new();

    for _ in 0..RUNS {
        let scenario = Scenario::build(spec);
        first_sync.push(run(&scenario, &["sync"]));
        noop_sync.push(run(&scenario, &["sync"]));
        status.push(run(&scenario, &["status"]));
        status_json.push(run(&scenario, &["--json", "status"]));
        doctor.push(run(&scenario, &["doctor"]));
    }

    let scenario = Scenario::build(spec);
    run(&scenario, &["sync"]);
    let invocations = git_invocations(&scenario, &["sync"]);

    println!();
    println!(
        "Reference scenario: {} sources ({} team, {} catalogs), {} skills ({} active), \
         {} targets, {} approvals, 1 instruction pack",
        spec.sources(),
        spec.team_sources,
        spec.catalogs,
        spec.skills(),
        spec.active_skills(),
        spec.targets,
        spec.approvals(),
    );
    println!("Runs per command: {RUNS} (median reported)");
    println!();
    println!("| Command | Median |");
    println!("| --- | --- |");
    for (label, samples) in [
        ("`dalo sync` (first run)", first_sync),
        ("`dalo sync` (no-op)", noop_sync),
        ("`dalo status`", status),
        ("`dalo status --json`", status_json),
        ("`dalo doctor`", doctor),
    ] {
        println!("| {label} | {} ms |", median(samples).as_millis());
    }
    println!();
    println!(
        "Git subprocesses spawned by a no-op sync: {}",
        invocations.len()
    );
}
