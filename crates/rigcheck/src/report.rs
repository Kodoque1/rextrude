use std::path::Path;

use serde::Serialize;

use crate::checks::{CheckResult, Status};

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Format {
    Human,
    Json,
    Github,
}

pub fn exit_code(results: &[CheckResult]) -> i32 {
    if results.iter().any(|r| r.status == Status::Fail) {
        1
    } else {
        0
    }
}

#[derive(Serialize)]
struct JsonReport<'a> {
    model: String,
    checks: &'a [CheckResult],
}

pub fn render(format: Format, model_path: &Path, results: &[CheckResult]) -> String {
    match format {
        Format::Human => render_human(results),
        Format::Json => {
            let report = JsonReport {
                model: model_path.display().to_string(),
                checks: results,
            };
            serde_json::to_string_pretty(&report).unwrap_or_default()
        }
        Format::Github => render_github(model_path, results),
    }
}

fn render_human(results: &[CheckResult]) -> String {
    let mut out = String::new();
    let mut n_pass = 0;
    let mut n_fail = 0;
    let mut n_skip = 0;
    for r in results {
        let tag = match r.status {
            Status::Pass => {
                n_pass += 1;
                "PASS"
            }
            Status::Fail => {
                n_fail += 1;
                "FAIL"
            }
            Status::Skipped => {
                n_skip += 1;
                "SKIP"
            }
        };
        out.push_str(&format!("[{tag}] {}\n", r.id));
        for v in &r.violations {
            match &v.node {
                Some(node) => out.push_str(&format!("    - {node}: {}\n", v.message)),
                None => out.push_str(&format!("    - {}\n", v.message)),
            }
        }
    }
    out.push_str(&format!(
        "\n{n_pass} passed, {n_fail} failed, {n_skip} skipped\n"
    ));
    out
}

fn render_github(model_path: &Path, results: &[CheckResult]) -> String {
    let mut out = String::new();
    for r in results {
        if r.status != Status::Fail {
            continue;
        }
        for v in &r.violations {
            out.push_str(&format!(
                "::error file={}::{}: {}\n",
                model_path.display(),
                r.id,
                v.message
            ));
        }
    }
    out.push_str(&render_human(results));
    out
}
