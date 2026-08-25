//! Write one of the shipped starter plans to a .aprj file.
//!
//! For screenshots and documentation. The eight templates are the project's own content, so a plan
//! built from one carries nobody's real work: no client names, no staff names, no commercial dates.
//!
//!     cargo run -p aop-core --example demo_plan -- software /tmp/demo.aprj
use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let id = args.next().unwrap_or_else(|| "software".into());
    let out = args.next().unwrap_or_else(|| "/tmp/demo.aprj".into());

    let spec = aop_core::templates::by_id(&id).unwrap_or_else(|| {
        eprintln!("unknown template: {id}");
        eprintln!("try: {}", aop_core::templates::all().iter().map(|s| s.id).collect::<Vec<_>>().join(", "));
        std::process::exit(1);
    });

    // A fixed start date, so the same command twice produces the same plan. A screenshot that moves
    // with the calendar cannot be reproduced when somebody asks why it looks different.
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
        .unwrap()
        .and_hms_opt(8, 0, 0)
        .unwrap();

    let project = aop_core::templates::build(spec, start);
    match aop_core::persist::save(Path::new(&out), &project) {
        Ok(p) => println!("{} ({} tasks) -> {}", spec.name, spec.task_count(), p.display()),
        Err(e) => { eprintln!("save failed: {e}"); std::process::exit(1); }
    }
}
