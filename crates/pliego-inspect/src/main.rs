use pliego_inspect::{
    BaselineReport, InspectionReport, baseline, human_baseline, human_report, inspect_path,
    json_pretty,
};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = concat!(
    "pliego-inspect ",
    env!("CARGO_PKG_VERSION"),
    "

USAGE:
  pliego-inspect inspect <manifest> [--asset-root <dir>] [--format human|json]
                         [--output <file>] [--enforce-budgets]
  pliego-inspect baseline <targets.json> [--format human|json]
                          [--output <file>] [--enforce-budgets]

EXIT CODES:
  0  valid manifest or baseline; enforced budgets pass
  1  contract violation or enforced budget failure
  2  invalid command, unreadable input, or malformed JSON
"
);

#[derive(Debug, Clone, Copy)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug)]
struct Options {
    input: PathBuf,
    asset_root: Option<PathBuf>,
    format: OutputFormat,
    output: Option<PathBuf>,
    enforce_budgets: bool,
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<String>) -> Result<ExitCode, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err("missing command".to_owned());
    };
    if matches!(command, "help" | "--help" | "-h") {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    let options = parse_options(&args[1..])?;
    match command {
        "inspect" => run_inspect(options),
        "baseline" => run_baseline(options),
        other => Err(format!("unknown command {other:?}")),
    }
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let Some(input) = args.first() else {
        return Err("missing input path".to_owned());
    };
    if input.starts_with('-') {
        return Err("input path must follow the command".to_owned());
    }
    let mut options = Options {
        input: PathBuf::from(input),
        asset_root: None,
        format: OutputFormat::Human,
        output: None,
        enforce_budgets: false,
    };
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--asset-root" => {
                index += 1;
                options.asset_root =
                    Some(PathBuf::from(required_value(args, index, "--asset-root")?));
            }
            "--format" => {
                index += 1;
                options.format = match required_value(args, index, "--format")? {
                    "human" => OutputFormat::Human,
                    "json" => OutputFormat::Json,
                    value => return Err(format!("unsupported output format {value:?}")),
                };
            }
            "--output" => {
                index += 1;
                options.output = Some(PathBuf::from(required_value(args, index, "--output")?));
            }
            "--enforce-budgets" => options.enforce_budgets = true,
            value => return Err(format!("unknown option {value:?}")),
        }
        index += 1;
    }
    Ok(options)
}

fn required_value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| format!("{option} requires a value"))
}

fn run_inspect(options: Options) -> Result<ExitCode, String> {
    let report = inspect_path(&options.input, options.asset_root.as_deref())
        .map_err(|error| error.to_string())?;
    let rendered = render_inspection(&report, options.format)?;
    emit(rendered, options.output)?;
    Ok(contract_exit(
        report.valid,
        report.budgets_pass,
        options.enforce_budgets,
    ))
}

fn run_baseline(options: Options) -> Result<ExitCode, String> {
    if options.asset_root.is_some() {
        return Err("--asset-root is only valid for inspect".to_owned());
    }
    let report = baseline(&options.input).map_err(|error| error.to_string())?;
    let rendered = render_baseline(&report, options.format)?;
    emit(rendered, options.output)?;
    Ok(contract_exit(
        report.valid,
        report.budgets_pass,
        options.enforce_budgets,
    ))
}

fn render_inspection(report: &InspectionReport, format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Human => Ok(human_report(report)),
        OutputFormat::Json => json_pretty(report).map_err(|error| error.to_string()),
    }
}

fn render_baseline(report: &BaselineReport, format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Human => Ok(human_baseline(report)),
        OutputFormat::Json => json_pretty(report).map_err(|error| error.to_string()),
    }
}

fn emit(rendered: String, output: Option<PathBuf>) -> Result<(), String> {
    if let Some(path) = output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        fs::write(&path, rendered)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn contract_exit(valid: bool, budgets_pass: bool, enforce_budgets: bool) -> ExitCode {
    if valid && (!enforce_budgets || budgets_pass) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_rejects_unknown_format() {
        let error = parse_options(&[
            "manifest.json".to_owned(),
            "--format".to_owned(),
            "xml".to_owned(),
        ])
        .unwrap_err();
        assert!(error.contains("unsupported output format"));
    }

    #[test]
    fn budget_exit_is_opt_in() {
        assert_eq!(contract_exit(true, false, false), ExitCode::SUCCESS);
        assert_eq!(contract_exit(true, false, true), ExitCode::from(1));
        assert_eq!(contract_exit(false, true, false), ExitCode::from(1));
    }
}
