// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const STATE_CONTRACT: &str = "dev.pliegors.telemetry-state/v1";
const REPORT_CONTRACT: &str = "dev.pliegors.telemetry-report/v1";
const STATUS_CONTRACT: &str = "dev.pliegors.telemetry-status/v1";
const POLICY_VERSION: &str = "1.0.0";
const MAX_STATE_BYTES: u64 = 64 * 1024;
const MAX_EVENTS: usize = 64;
const LOCK_ATTEMPTS: usize = 40;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FunnelEvent {
    Install,
    New,
    Check,
    Dev,
    Build,
}

#[derive(Debug)]
pub(crate) enum TelemetryFailure {
    Usage(String),
    Storage(String),
}

impl TelemetryFailure {
    pub(crate) fn into_parts(self) -> (bool, String) {
        match self {
            Self::Usage(message) => (true, message),
            Self::Storage(message) => (false, message),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventRecord {
    sequence: u64,
    event: FunnelEvent,
    day_since_unix_epoch: u64,
    cli_version: String,
    platform: String,
    architecture: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    contract: String,
    policy_version: String,
    enabled: bool,
    enabled_at_day: Option<u64>,
    next_sequence: u64,
    events: Vec<EventRecord>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            contract: STATE_CONTRACT.to_owned(),
            policy_version: POLICY_VERSION.to_owned(),
            enabled: false,
            enabled_at_day: None,
            next_sequence: 0,
            events: Vec::new(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsentReport {
    enabled: bool,
    policy_version: &'static str,
    enabled_at_day: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryReport<'a> {
    contract: &'static str,
    generated_at_day: u64,
    consent: ConsentReport,
    fields: [&'static str; 6],
    events: &'a [EventRecord],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusReport<'a> {
    contract: &'static str,
    policy_version: &'static str,
    enabled: bool,
    local_event_count: usize,
    network_submission: &'static str,
    storage_path: &'a Path,
}

#[derive(Clone, Copy)]
enum OutputFormat {
    Human,
    Json,
}

struct Store {
    directory: PathBuf,
    state_path: PathBuf,
    lock_path: PathBuf,
}

struct StoreLock {
    path: PathBuf,
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn run(arguments: Vec<String>) -> Result<(), TelemetryFailure> {
    let store = Store::from_environment().map_err(TelemetryFailure::Storage)?;
    let mut arguments = arguments.into_iter();
    let command = arguments.next().ok_or_else(usage)?;
    let rest = arguments.collect::<Vec<_>>();
    match command.as_str() {
        "status" => {
            let format = parse_format(rest)?;
            let state = store.load().map_err(TelemetryFailure::Storage)?;
            print_status(&store, &state, format).map_err(TelemetryFailure::Storage)
        }
        "enable" => {
            reject_arguments(&rest)?;
            let already_enabled = store.enable().map_err(TelemetryFailure::Storage)?;
            if already_enabled {
                println!("PLIEGO telemetry: already enabled locally");
            } else {
                println!("PLIEGO telemetry: enabled locally; no network submission is configured");
            }
            println!("Preview with `pliego telemetry preview`; export is always explicit.");
            Ok(())
        }
        "preview" => {
            let format = parse_format(rest)?;
            let state = store.load().map_err(TelemetryFailure::Storage)?;
            print_report(&state, format).map_err(TelemetryFailure::Storage)
        }
        "export" => {
            let output = parse_export(rest)?;
            let state = store.load().map_err(TelemetryFailure::Storage)?;
            store
                .export(&state, &output)
                .map_err(TelemetryFailure::Storage)?;
            println!("PLIEGO telemetry export: {}", output.display());
            println!("No data was transmitted.");
            Ok(())
        }
        "disable" => {
            let delete_local = parse_disable(rest)?;
            store
                .disable(delete_local)
                .map_err(TelemetryFailure::Storage)?;
            if delete_local {
                println!("PLIEGO telemetry: disabled; local telemetry state deleted");
            } else {
                println!("PLIEGO telemetry: disabled; retained events remain local");
            }
            Ok(())
        }
        _ => Err(usage()),
    }
}

pub(crate) fn record(event: FunnelEvent) -> Result<bool, String> {
    Store::from_environment()?.record(event)
}

fn usage() -> TelemetryFailure {
    TelemetryFailure::Usage(
        "usage: pliego telemetry <status|enable|preview|export|disable> [options]".to_owned(),
    )
}

fn reject_arguments(arguments: &[String]) -> Result<(), TelemetryFailure> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(usage())
    }
}

fn parse_format(arguments: Vec<String>) -> Result<OutputFormat, TelemetryFailure> {
    match arguments.as_slice() {
        [] => Ok(OutputFormat::Human),
        [option, value] if option == "--format" && value == "human" => Ok(OutputFormat::Human),
        [option, value] if option == "--format" && value == "json" => Ok(OutputFormat::Json),
        _ => Err(TelemetryFailure::Usage(
            "telemetry status/preview accepts only `--format human` or `--format json`".to_owned(),
        )),
    }
}

fn parse_export(arguments: Vec<String>) -> Result<PathBuf, TelemetryFailure> {
    match arguments.as_slice() {
        [option, output] if option == "--output" && !output.is_empty() => Ok(PathBuf::from(output)),
        _ => Err(TelemetryFailure::Usage(
            "usage: pliego telemetry export --output <new-file>".to_owned(),
        )),
    }
}

fn parse_disable(arguments: Vec<String>) -> Result<bool, TelemetryFailure> {
    match arguments.as_slice() {
        [] => Ok(false),
        [option] if option == "--delete-local" => Ok(true),
        _ => Err(TelemetryFailure::Usage(
            "usage: pliego telemetry disable [--delete-local]".to_owned(),
        )),
    }
}

fn print_status(store: &Store, state: &State, format: OutputFormat) -> Result<(), String> {
    match format {
        OutputFormat::Human => {
            println!(
                "PLIEGO telemetry: {}",
                if state.enabled { "enabled" } else { "disabled" }
            );
            println!("Policy: {POLICY_VERSION}");
            println!("Local events: {} / {MAX_EVENTS}", state.events.len());
            println!("Network submission: none");
            println!("Storage: {}", store.state_path.display());
            Ok(())
        }
        OutputFormat::Json => print_json(&StatusReport {
            contract: STATUS_CONTRACT,
            policy_version: POLICY_VERSION,
            enabled: state.enabled,
            local_event_count: state.events.len(),
            network_submission: "none",
            storage_path: &store.state_path,
        }),
    }
}

fn print_report(state: &State, format: OutputFormat) -> Result<(), String> {
    let report = report(state);
    match format {
        OutputFormat::Human => {
            println!("PLIEGO voluntary telemetry preview");
            println!(
                "Consent: {}",
                if state.enabled { "enabled" } else { "disabled" }
            );
            println!("Events: {}", state.events.len());
            for event in &state.events {
                println!(
                    "  {:>3} {:?} day={} {} {}/{}",
                    event.sequence,
                    event.event,
                    event.day_since_unix_epoch,
                    event.cli_version,
                    event.platform,
                    event.architecture
                );
            }
            println!(
                "No paths, project names, arguments, errors, or environment values are collected."
            );
            Ok(())
        }
        OutputFormat::Json => print_json(&report),
    }
}

fn print_json(value: &impl Serialize) -> Result<(), String> {
    let bytes = canonical_json(value)?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|error| format!("cannot write telemetry JSON: {error}"))
}

fn report(state: &State) -> TelemetryReport<'_> {
    TelemetryReport {
        contract: REPORT_CONTRACT,
        generated_at_day: current_day(),
        consent: ConsentReport {
            enabled: state.enabled,
            policy_version: POLICY_VERSION,
            enabled_at_day: state.enabled_at_day,
        },
        fields: [
            "sequence",
            "event",
            "daySinceUnixEpoch",
            "cliVersion",
            "platform",
            "architecture",
        ],
        events: &state.events,
    }
}

impl Store {
    fn from_environment() -> Result<Self, String> {
        let home = if let Some(value) = std::env::var_os("PLIEGO_HOME") {
            if value.is_empty() {
                return Err("PLIEGO_HOME cannot be empty for telemetry storage".to_owned());
            }
            PathBuf::from(value)
        } else {
            let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            let value = std::env::var_os(variable).ok_or_else(|| {
                format!("cannot resolve telemetry storage; set PLIEGO_HOME or {variable}")
            })?;
            PathBuf::from(value).join(".pliego")
        };
        if !home.is_absolute() {
            return Err(format!(
                "telemetry storage root must be absolute: {}",
                home.display()
            ));
        }
        Ok(Self::at(home.join("telemetry")))
    }

    fn at(directory: PathBuf) -> Self {
        Self {
            state_path: directory.join("state.json"),
            lock_path: directory.join("state.lock"),
            directory,
        }
    }

    fn load(&self) -> Result<State, String> {
        if !self.state_path.exists() {
            return Ok(State::default());
        }
        let metadata = fs::symlink_metadata(&self.state_path)
            .map_err(|error| format!("cannot inspect telemetry state: {error}"))?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err("telemetry state must be a regular file".to_owned());
        }
        if metadata.len() > MAX_STATE_BYTES {
            return Err("telemetry state exceeds 64 KiB".to_owned());
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.state_path)
            .and_then(|mut file| file.read_to_end(&mut bytes))
            .map_err(|error| format!("cannot read telemetry state: {error}"))?;
        let state: State = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid telemetry state: {error}"))?;
        validate_state(&state)?;
        Ok(state)
    }

    fn enable(&self) -> Result<bool, String> {
        self.ensure_directory()?;
        let _lock = self.lock()?;
        let mut state = self.load()?;
        if state.enabled {
            return Ok(true);
        }
        state.enabled = true;
        state.enabled_at_day = Some(current_day());
        append_event(&mut state, FunnelEvent::Install)?;
        self.save(&state)?;
        Ok(false)
    }

    fn record(&self, event: FunnelEvent) -> Result<bool, String> {
        if !self.state_path.exists() {
            return Ok(false);
        }
        let _lock = self.lock()?;
        let mut state = self.load()?;
        if !state.enabled {
            return Ok(false);
        }
        append_event(&mut state, event)?;
        self.save(&state)?;
        Ok(true)
    }

    fn disable(&self, delete_local: bool) -> Result<(), String> {
        if !self.state_path.exists() {
            return Ok(());
        }
        let lock = self.lock()?;
        if delete_local {
            fs::remove_file(&self.state_path)
                .map_err(|error| format!("cannot delete telemetry state: {error}"))?;
            drop(lock);
            match fs::remove_dir(&self.directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("cannot remove telemetry directory: {error}")),
            }
            return Ok(());
        }
        let mut state = self.load()?;
        state.enabled = false;
        state.enabled_at_day = None;
        self.save(&state)
    }

    fn export(&self, state: &State, output: &Path) -> Result<(), String> {
        if output.as_os_str().is_empty() {
            return Err("telemetry export path cannot be empty".to_owned());
        }
        let bytes = canonical_json(&report(state))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        secure_file_options(&mut options);
        let mut file = options.open(output).map_err(|error| {
            format!(
                "cannot create telemetry export {}: {error}",
                output.display()
            )
        })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot write telemetry export: {error}"))
    }

    fn ensure_directory(&self) -> Result<(), String> {
        if self.directory.exists() {
            let metadata = fs::symlink_metadata(&self.directory)
                .map_err(|error| format!("cannot inspect telemetry directory: {error}"))?;
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err("telemetry directory must be a real directory".to_owned());
            }
            return Ok(());
        }
        create_secure_directory(&self.directory)
            .map_err(|error| format!("cannot create telemetry directory: {error}"))
    }

    fn lock(&self) -> Result<StoreLock, String> {
        self.ensure_directory()?;
        for _ in 0..LOCK_ATTEMPTS {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            secure_file_options(&mut options);
            match options.open(&self.lock_path) {
                Ok(mut file) => {
                    writeln!(file, "{}", std::process::id())
                        .and_then(|()| file.sync_all())
                        .map_err(|error| format!("cannot initialize telemetry lock: {error}"))?;
                    return Ok(StoreLock {
                        path: self.lock_path.clone(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&self.lock_path)? {
                        let _ = fs::remove_file(&self.lock_path);
                    } else {
                        thread::sleep(Duration::from_millis(25));
                    }
                }
                Err(error) => return Err(format!("cannot acquire telemetry lock: {error}")),
            }
        }
        Err("telemetry state is busy; retry the command".to_owned())
    }

    fn save(&self, state: &State) -> Result<(), String> {
        validate_state(state)?;
        let bytes = canonical_json(state)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err("telemetry state exceeds 64 KiB".to_owned());
        }
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = self
            .directory
            .join(format!(".state-{}-{suffix}.json", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        secure_file_options(&mut options);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("cannot stage telemetry state: {error}"))?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("cannot stage telemetry state: {error}"));
        }
        if cfg!(windows) && self.state_path.exists() {
            fs::remove_file(&self.state_path)
                .map_err(|error| format!("cannot replace telemetry state: {error}"))?;
        }
        if let Err(error) = fs::rename(&temporary, &self.state_path) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("cannot commit telemetry state: {error}"));
        }
        Ok(())
    }
}

fn append_event(state: &mut State, event: FunnelEvent) -> Result<(), String> {
    if state.next_sequence == u64::MAX {
        return Err("telemetry sequence exhausted".to_owned());
    }
    let record = EventRecord {
        sequence: state.next_sequence,
        event,
        day_since_unix_epoch: current_day(),
        cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        platform: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
    };
    state.next_sequence += 1;
    state.events.push(record);
    if state.events.len() > MAX_EVENTS {
        state.events.remove(0);
    }
    Ok(())
}

fn validate_state(state: &State) -> Result<(), String> {
    if state.contract != STATE_CONTRACT || state.policy_version != POLICY_VERSION {
        return Err("unsupported telemetry state contract or policy".to_owned());
    }
    if state.events.len() > MAX_EVENTS {
        return Err("telemetry state exceeds the 64-event bound".to_owned());
    }
    if state.enabled != state.enabled_at_day.is_some() {
        return Err("telemetry consent state is inconsistent".to_owned());
    }
    let mut previous = None;
    for event in &state.events {
        if previous.is_some_and(|value| event.sequence <= value)
            || event.sequence >= state.next_sequence
            || event.cli_version.is_empty()
            || event.cli_version.len() > 128
            || event.platform.is_empty()
            || event.platform.len() > 32
            || event.architecture.is_empty()
            || event.architecture.len() > 32
        {
            return Err("telemetry event contract is invalid".to_owned());
        }
        previous = Some(event.sequence);
    }
    Ok(())
}

fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("cannot encode telemetry JSON: {error}"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn current_day() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400
}

fn lock_is_stale(path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect telemetry lock: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("telemetry lock must be a regular file".to_owned());
    }
    let modified = metadata
        .modified()
        .map_err(|error| format!("cannot read telemetry lock age: {error}"))?;
    Ok(SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        > Duration::from_secs(30))
}

#[cfg(unix)]
fn secure_file_options(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn secure_file_options(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn create_secure_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_secure_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn fixture(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pliego-telemetry-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn absent_state_is_disabled_and_does_not_create_storage() {
        let directory = fixture("absent");
        let store = Store::at(directory.clone());
        assert_eq!(store.load().unwrap(), State::default());
        assert!(!store.record(FunnelEvent::New).unwrap());
        assert!(!directory.exists());
    }

    #[test]
    fn consent_is_explicit_bounded_and_deletable() {
        let directory = fixture("lifecycle");
        let store = Store::at(directory.clone());
        assert!(!store.enable().unwrap());
        assert!(store.enable().unwrap());
        for _ in 0..80 {
            assert!(store.record(FunnelEvent::Build).unwrap());
        }
        let state = store.load().unwrap();
        assert!(state.enabled);
        assert_eq!(state.events.len(), MAX_EVENTS);
        assert!(
            state
                .events
                .iter()
                .all(|event| event.event == FunnelEvent::Build)
        );
        store.disable(true).unwrap();
        assert!(!directory.exists());
    }

    #[test]
    fn report_contains_only_the_documented_allowlist() {
        let directory = fixture("report");
        let store = Store::at(directory.clone());
        store.enable().unwrap();
        store.record(FunnelEvent::New).unwrap();
        let bytes = canonical_json(&report(&store.load().unwrap())).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        for forbidden in [
            "project",
            "path",
            "argument",
            "error",
            "environment",
            "user",
            "email",
        ] {
            assert!(
                !text.to_ascii_lowercase().contains(forbidden),
                "report contains {forbidden}"
            );
        }
        assert!(text.contains("\"install\""));
        assert!(text.contains("\"new\""));
        store.disable(true).unwrap();
    }

    #[test]
    fn unknown_or_inconsistent_state_fails_closed() {
        let directory = fixture("tamper");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("state.json"),
            br#"{"contract":"dev.pliegors.telemetry-state/v1","policyVersion":"1.0.0","enabled":true,"enabledAtDay":null,"nextSequence":0,"events":[],"unknown":true}"#,
        )
        .unwrap();
        let store = Store::at(directory.clone());
        assert!(
            store
                .load()
                .unwrap_err()
                .contains("invalid telemetry state")
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
