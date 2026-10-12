//! Every report in `evals/reports/`, one line each on the console and one row each
//! in `evals/RESULTS.md`: scenario, chief, staff, how many expectations held, the
//! numbers behind them. Reports are kept: a run is evidence, and a later run beside
//! it is the comparison. `cargo xtask eval summary` (`npm run eval:summary`) runs it.

use std::fs;
use std::io::Write;
use std::path::Path;

use cf_e2e::checkout;

use crate::results::{results_table, row, Report};
use crate::{Error, Result};

/// The reports in `folder`, oldest first: each file of it that ends in `.json`, by
/// its name, which begins with the time the run was made. A folder that is not
/// there holds none.
pub fn read_reports(folder: &Path) -> Result<Vec<Report>> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let file = folder.join(&name);
            let text = fs::read_to_string(&file).map_err(Error::file("read", &file))?;
            let value = serde_json::from_str(&text).map_err(|cause| {
                Error::Refused(format!("{} is no JSON: {cause}", file.display()))
            })?;
            Ok(Report {
                name: Some(name),
                value,
            })
        })
        .collect()
}

/// Says each report in `reports` in a line on `out`, and writes the results file
/// `results` from them: how many rows it has. None, and no file written, when
/// there are no reports yet.
pub fn summarize(reports: &Path, results: &Path, out: &mut dyn Write) -> Result<usize> {
    let kept = read_reports(reports)?;
    if kept.is_empty() {
        writeln!(out, "no reports yet")?;
        return Ok(0);
    }
    for report in &kept {
        writeln!(out, "{}", row(report).join(" · "))?;
    }
    fs::write(results, results_table(&kept)).map_err(Error::file("write", results))?;
    writeln!(
        out,
        "written: {} ({} rows)",
        checkout::relative(results),
        kept.len()
    )?;
    Ok(kept.len())
}
