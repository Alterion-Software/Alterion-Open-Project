//! Write a plan of a given size to a .aprj file, for testing at scale.
//!
//! The benchmark this project is aimed at is a hundred and fifty thousand
//! tasks with links between them, which is far past anything the templates
//! hold. A plan that large is not something to keep in the repository: it is
//! two megabytes on disk and it says nothing a generator does not, so this
//! writes one on demand instead.
//!
//!     cargo run -p aop-core --release --example big_plan -- 150000 /tmp/big.aprj
//!
//! Release, not debug. Scheduling a plan this size is half a second optimised
//! and the better part of a minute unoptimised, and waiting for that tells
//! nobody anything.
//!
//! The shape is meant to be ordinary rather than adversarial: a summary every
//! twenty rows with its own children under it, and each run of children
//! chained end to start, which is how a real plan of phases and their steps is
//! put together. Links stay inside their group, because a summary is driven by
//! its children and linking it to one of them is a task waiting on itself,
//! which the scheduler rejects as a loop.
//!
//! The phases are also spread across the calendar rather than all starting on
//! the same Monday. That matters more than it sounds: a plan whose phases all
//! run in parallel puts every band in the timeline strip at full width, which
//! is the one shape where the strip is cheap and nothing about it is typical.
//! Real work overlaps, but it does not all start at once. Each phase begins a
//! little after the one before, wrapping round `SPREAD` so a plan of any size
//! covers a believable stretch of calendar instead of a thousand years.
use std::path::Path;
use std::time::Instant;

/// Rows per summary. Every twentieth row is a phase and the nineteen after it
/// are its steps.
const GROUP: usize = 20;

/// How many working days the phases are spread over before wrapping.
///
/// Two years of overlap. Enough that the timeline strip has to fit many bands
/// into its width, which is the case worth measuring, and not so much that a
/// plan of a hundred and fifty thousand tasks claims to run past the century.
const SPREAD: i64 = 500;

fn main() {
    let mut args = std::env::args().skip(1);
    let count: usize = args
        .next()
        .unwrap_or_else(|| "150000".into())
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("first argument is how many tasks: {e}");
            std::process::exit(1);
        });
    let out = args.next().unwrap_or_else(|| "/tmp/big.aprj".into());

    // Fixed, so the same command twice produces the same plan and a figure
    // measured today can be compared with one measured next week.
    let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 5)
        .unwrap()
        .and_hms_opt(8, 0, 0)
        .unwrap();

    let clock = Instant::now();
    let mut project = aop_core::Project::blank(start);
    for index in 0..count {
        let at = project.tasks.len();
        project.insert_task(at, format!("Task {index}"));
        if let Some(task) = project.tasks.last_mut() {
            task.outline_level = if index % GROUP == 0 { 0 } else { 1 };
            task.duration_minutes = 480;
            // Otherwise every duration prints with the question mark that
            // marks a guess, and a generated plan is not guessing.
            task.estimated = false;
        }
        // The first step of each phase is pinned, which drags the phase with
        // it. Pinning the summary itself would do nothing: a summary is
        // driven by its children.
        if index % GROUP == 1 {
            let offset = ((index / GROUP) as i64) % SPREAD;
            if let Some(task) = project.tasks.last_mut() {
                task.constraint = aop_core::ConstraintType::StartNoEarlierThan;
                task.constraint_date = Some(start + chrono::Duration::days(offset));
            }
        }
    }
    println!("  build      {:>9.1?}  ({count} tasks)", clock.elapsed());

    let clock = Instant::now();
    let ids: Vec<aop_core::TaskId> = project.tasks.iter().map(|task| task.id).collect();
    for index in 0..count.saturating_sub(1) {
        // Between the steps of one phase only: not from the phase to its first
        // step, and not across the boundary into the next phase.
        if index % GROUP != 0 && index % GROUP != GROUP - 1 {
            project
                .links
                .push(aop_core::Link::finish_to_start(ids[index], ids[index + 1]));
        }
    }
    println!("  link       {:>9.1?}  ({} links)", clock.elapsed(), project.links.len());

    let clock = Instant::now();
    if let Err(error) = aop_core::schedule(&mut project) {
        eprintln!("the generated plan does not schedule: {error}");
        std::process::exit(1);
    }
    println!("  schedule   {:>9.1?}", clock.elapsed());

    let clock = Instant::now();
    match aop_core::persist::save(Path::new(&out), &project) {
        Ok(path) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!(
                "  save       {:>9.1?}  ({} MB)\n  {}",
                clock.elapsed(),
                size / 1_048_576,
                path.display(),
            );
        }
        Err(error) => {
            eprintln!("save failed: {error}");
            std::process::exit(1);
        }
    }
}
