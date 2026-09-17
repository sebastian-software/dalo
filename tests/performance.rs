//! Performance envelope: the measurement run and the CI regression guard.
//!
//! `cargo test --release --test performance -- --ignored --nocapture` prints the
//! table published in `docs/compatibility.md`. The ordinary suite instead runs
//! [`noop_sync_should_not_regress_against_a_minimal_store`], which compares the
//! same command against a one-source baseline measured on the same machine.

mod common;

use common::scenario::{Scenario, ScenarioSpec};
use std::time::{Duration, Instant};

/// How many times each command is timed. The median is reported.
const RUNS: usize = 3;

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

fn median_of(scenario: &Scenario, arguments: &[&str]) -> Duration {
    median((0..RUNS).map(|_| run(scenario, arguments)).collect())
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

/// Guards the no-op `sync` hot path against a 5x regression.
///
/// A fixed wall-clock budget cannot do this job: CI runners are several times
/// slower than a developer machine, and a debug build is several times slower
/// than the release build the published envelope was measured with, so any
/// number generous enough never to fire on a slow runner would also be too
/// generous to catch a regression on a fast one.
///
/// The test therefore measures a *ratio* on whatever machine it runs on: no-op
/// `sync` over the smoke scenario (5 sources, 20 skills, 2 targets) divided by
/// no-op `sync` over a store with a single source, a single skill, and a single
/// target. Both numbers scale with the machine, the build profile, and the
/// filesystem, so the ratio does not. What the ratio does scale with is work
/// that grows with the store — a per-command rescan, a serial fetch per source,
/// or a quadratic resolve — which is exactly the regression shape this guards.
///
/// The measured ratio is 1.9–2.6 on an Apple M1 Ultra, and it is the same in a
/// debug and a release build (a no-op `sync` is dominated by Git subprocesses
/// and store I/O, not by optimized Rust), which is the evidence that it really
/// does cancel the machine out. [`NOOP_SYNC_RATIO_BOUND`] is 6.0. A 5x
/// regression in the scenario's no-op `sync` that leaves the single-source
/// store alone lands near 10, and even a regression that also slows the
/// baseline by half as much still clears 6, so the bound fires well before 5x.
/// In the other direction it sits more than 2x above the slowest ratio
/// observed, which medians over [`RUNS`] runs keep it far from: the test has no
/// wall-clock budget to blow, so a runner that is uniformly slow cannot fail
/// it at all.
#[test]
fn noop_sync_should_not_regress_against_a_minimal_store() {
    let baseline = Scenario::build(ScenarioSpec::baseline());
    let scenario = Scenario::build(ScenarioSpec::smoke());
    run(&baseline, &["sync"]);
    run(&scenario, &["sync"]);

    let baseline_noop = median_of(&baseline, &["sync"]);
    let scenario_noop = median_of(&scenario, &["sync"]);
    let ratio = scenario_noop.as_secs_f64() / baseline_noop.as_secs_f64();

    assert!(
        ratio <= NOOP_SYNC_RATIO_BOUND,
        "no-op sync over {} sources / {} skills took {scenario_noop:?}, \
         {ratio:.2}x the {baseline_noop:?} of a single-source store; \
         the bound is {NOOP_SYNC_RATIO_BOUND:.1}x",
        scenario.spec.sources(),
        scenario.spec.skills(),
    );

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
/// cargo test --release --locked --test performance -- --ignored --nocapture
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
