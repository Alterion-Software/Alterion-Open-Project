//! Turn a spreadsheet into a plan, without a window.
//!
//! The same reader the Import page uses, driven from the command line, so a
//! workbook can be converted in a script or looked at while working out why an
//! import came out wrong. It prints what it guessed and everything it could
//! not read, because a silent conversion of somebody else's spreadsheet is
//! how data goes missing without anybody noticing.
//!
//!   cargo run -p aop-core --example convert -- <workbook.xlsx> [out.aprj]

use aop_core::sheet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: convert <workbook.xlsx> [out.aprj]");
        std::process::exit(2);
    };
    let input = std::path::PathBuf::from(input);
    let output = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| input.with_extension("aprj"));

    let dump = std::env::var("DUMP").ok();
    let sheets = sheet::survey(&input)?;
    if let Some(want) = dump {
        // Look at the sheet as it actually is, because every guess this
        // reader makes is a guess about what is in these cells.
        for s in &sheets {
            if s.name != want {
                continue;
            }
            for (n, row) in s.rows.iter().enumerate().take(20) {
                let cells: Vec<String> = row
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| !matches!(c, sheet::Cell::Empty))
                    .map(|(i, c)| format!("{}={:?}", sheet::column_letter(i), c.text()))
                    .collect();
                println!("row {:>3}: {}", n + 1, cells.join("  "));
            }
            return Ok(());
        }
    }
    println!("{} sheet(s) in {}", sheets.len(), input.display());
    for s in &sheets {
        println!("  {:<28} {} rows", s.name, s.rows.len());
    }

    // The sheet the reader itself would choose: the first whose columns
    // include something it can use as a name.
    let chosen = sheets
        .iter()
        .find(|s| {
            let mapping = sheet::Mapping::guess(s);
            mapping.columns.contains(&Some(sheet::Field::Name))
        })
        .or_else(|| sheets.first())
        .ok_or("the workbook has no sheets")?;

    let mut mapping = sheet::Mapping::guess(chosen);
    // Columns named by hand, when the guesser cannot be expected to know. A
    // heading like "In Dependencies (Predecessors)" is a predecessor column
    // to a person and not to a matcher, and "No." holding 1, 1.1, 1.1.1 is
    // the whole outline rather than an identifier.
    //   MAP="A=OutlineLevel,Q=Predecessors"
    if let Ok(pairs) = std::env::var("MAP") {
        for pair in pairs.split(',').filter(|p| !p.trim().is_empty()) {
            let Some((letter, name)) = pair.split_once('=') else { continue };
            let index = letter
                .trim()
                .bytes()
                .fold(0usize, |acc, b| acc * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize)
                - 1;
            let field = sheet::Field::ALL
                .iter()
                .find(|f| format!("{f:?}").eq_ignore_ascii_case(name.trim()));
            if let Some(field) = field {
                // One field cannot come from two columns.
                for held in mapping.columns.iter_mut() {
                    if *held == Some(*field) {
                        *held = None;
                    }
                }
                if index < mapping.columns.len() {
                    mapping.columns[index] = Some(*field);
                }
            }
        }
    }
    println!("\nreading \"{}\"", chosen.name);
    println!("  heading row {}", mapping.heading_row + 1);
    for (index, field) in mapping.columns.iter().enumerate() {
        if let Some(field) = field {
            let heading = chosen
                .rows
                .get(mapping.heading_row)
                .and_then(|row| row.get(index))
                .map(|cell| cell.text())
                .unwrap_or_default();
            println!("  {:<4} {:<28} -> {:?}", sheet::column_letter(index), heading, field);
        }
    }

    let name = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported plan".into());
    let import = sheet::read(chosen, &mapping, &name)?;

    println!("\n{} task(s)", import.project.tasks.len());
    // The outline, because a flat list of 1500 rows is not a plan.
    let mut levels = std::collections::BTreeMap::new();
    for task in &import.project.tasks {
        *levels.entry(task.outline_level).or_insert(0usize) += 1;
    }
    println!("  outline levels: {levels:?}");
    for task in import.project.tasks.iter().take(12) {
        println!(
            "  {}{}  {} to {}",
            "  ".repeat(task.outline_level as usize),
            task.name,
            task.scheduled.start.date(),
            task.scheduled.finish.date()
        );
    }
    println!("{} resource(s)", import.project.resources.len());
    println!("{} link(s)", import.project.links.len());
    for notice in &import.report.notices {
        println!(
            "  row {}: {} \"{}\" {}",
            notice.row, notice.heading, notice.value, notice.why
        );
    }

    aop_core::persist::save(&output, &import.project)?;
    println!("\nwritten to {}", output.display());
    Ok(())
}
