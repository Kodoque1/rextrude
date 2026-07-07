use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use rigcheck::checks::{hygiene, quality, structure, sweep, CheckResult};
use rigcheck::report::Format;
use rigcheck::{evidence, model, spec};

#[derive(Parser)]
#[command(
    name = "rigcheck",
    about = "Validate a rigged machine GLB model against a sidecar spec"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Check {
        model: PathBuf,
        #[arg(long)]
        spec: Option<PathBuf>,
        #[arg(long)]
        evidence: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "human")]
        format: Format,
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        skip: Vec<String>,
    },
}

fn sidecar(model_path: &Path, ext: &str) -> PathBuf {
    let stem = model_path.file_stem().unwrap_or_default().to_string_lossy();
    model_path.with_file_name(format!("{stem}.{ext}"))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Check {
            model: model_path,
            spec: spec_path,
            evidence: evidence_path,
            format,
            only,
            skip,
        } => run_check(model_path, spec_path, evidence_path, format, only, skip),
    }
}

fn run_check(
    model_path: PathBuf,
    spec_path: Option<PathBuf>,
    evidence_path: Option<PathBuf>,
    format: Format,
    only: Vec<String>,
    skip: Vec<String>,
) -> ExitCode {
    let spec_path = spec_path.unwrap_or_else(|| sidecar(&model_path, "machine.toml"));
    let machine_spec = match spec::load(&spec_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let evidence_path = evidence_path.unwrap_or_else(|| sidecar(&model_path, "evidence.json"));
    let ev = if evidence_path.exists() {
        match evidence::load(&evidence_path) {
            Ok(e) => Some(e),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };

    let loaded_model = match model::load(&model_path, &machine_spec) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let mut results: Vec<CheckResult> = Vec::new();
    results.extend(structure::run(&loaded_model, &machine_spec));
    results.extend(hygiene::run(&loaded_model, &machine_spec));
    results.extend(sweep::run(&loaded_model, &machine_spec, ev.as_ref()));
    results.extend(quality::run(&loaded_model, &machine_spec, ev.as_ref()));

    if !only.is_empty() {
        results.retain(|r| {
            only.iter()
                .any(|f| r.id == *f || r.id.starts_with(f.as_str()))
        });
    }
    if !skip.is_empty() {
        results.retain(|r| {
            !skip
                .iter()
                .any(|f| r.id == *f || r.id.starts_with(f.as_str()))
        });
    }

    let text = rigcheck::report::render(format, &model_path, &results);
    println!("{text}");

    let code = rigcheck::report::exit_code(&results);
    ExitCode::from(code as u8)
}
