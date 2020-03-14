#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![warn(clippy::cargo)]
#![deny(clippy::all)]

#![allow(clippy::multiple_crate_versions)]

#[macro_use]
extern crate log;

mod cli;
mod common;
mod config;
mod info;

use std::borrow::ToOwned;
use std::collections::HashSet;
use std::convert::TryInto;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, Permissions};
use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{anyhow, Error, Result};
use console::Style;
use dialoguer::{Input, PasswordInput};
use serde::Serialize;
use structopt::StructOpt;
use tempdir::TempDir;

use cli::Opt;
use common::{secure_name_check, EnvMap, DEPS_SCRIPT, INFO_FILE};
use config::Config;
use info::InfoSpec;

#[derive(Debug, Serialize)]
struct JobSpec {
    name: String,
    provided_env: EnvMap,
    depends: Vec<String>,
    ask_for_vars: Vec<String>,
    has_deps_script: bool,
}

#[derive(Debug, Clone)]
struct ReadyJob {
    name: String,
    env: EnvMap,
    depends: Vec<String>,
    has_deps_script: bool,
}

impl JobSpec {
    pub fn new(
        name: String,
        provided_env: EnvMap,
        depends: Vec<String>,
        ask_for_vars: Vec<String>,
        has_deps_script: bool,
    ) -> Self {
        Self {
            name,
            provided_env,
            depends,
            ask_for_vars,
            has_deps_script,
        }
    }

    #[inline]
    const fn get_ask_vars(&self) -> &Vec<String> {
        &self.ask_for_vars
    }
}

impl ReadyJob {
    pub fn new(name: String, env: EnvMap, depends: Vec<String>, has_deps_script: bool) -> Self {
        Self {
            name,
            env,
            depends,
            has_deps_script,
        }
    }

    #[inline]
    fn script_dir<P: AsRef<Path>>(&self, root: P) -> PathBuf {
        root.as_ref().join(&self.name)
    }

    fn create_proc_env<P: AsRef<Path>>(&self, root: P) -> Result<EnvMap> {
        let mut map = EnvMap::with_capacity(self.env.len());
        for (k, v) in &self.env {
            map.insert(encode_key(k), v.clone());
        }
        map.insert(
            "HOME".into(),
            dirs::home_dir()
                .ok_or_else(|| anyhow!("Cannot find home dir"))?
                .display()
                .to_string(),
        );
        map.insert("USER".into(), whoami::username());
        map.insert("USERNAME".into(), whoami::username());
        map.insert(
            "SCRIPT_DIR".into(),
            self.script_dir(root).display().to_string(),
        );
        Ok(map)
    }

    pub fn report(&self, job_num: usize) -> String {
        let mut report = String::new();
        report.push_str("Would run job ");
        report.push_str(&format!("{:03}", job_num));
        report.push_str(": ");
        report.push_str(&job_style().apply_to(&self.name).to_string());
        // report.push('\n');
        for d in &self.depends {
            report.push('\n');
            report.push_str(&info_style().apply_to("  Depends on: ").to_string());
            report.push_str(&info_style().apply_to(d).to_string());
        }
        if self.has_deps_script {
            report.push('\n');
            report.push_str(&info_style().apply_to("  Deps.sh: yes").to_string());
        };
        for (k, v) in &self.env {
            report.push('\n');
            report.push_str(&info_style().apply_to("  Env: ").to_string());
            report.push_str(&info_style().apply_to(k).to_string());
            report.push_str(&info_style().apply_to(" -> ").to_string());
            report.push_str(&info_style().apply_to(v).to_string());
        }
        report
    }

    fn run_process<P: AsRef<OsStr>>(&self, env: &EnvMap, runnable: P) -> Result<()> {
        debug!(
            "Executing runnable: {}",
            runnable.as_ref().to_string_lossy()
        );
        ensure_executable(&runnable.as_ref())?;
        let tmp_dir = TempDir::new(&self.name)?;
        let status = process::Command::new(runnable)
            .envs(env)
            .env("TMP_DIR", tmp_dir.path())
            .env("TEMP_DIR", tmp_dir.path())
            .status()?;
        drop(tmp_dir); // Statically enforce that we didn't drop until here.
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!(format!(
                "Job '{}' failed with exit code {}",
                self.name,
                status.code().unwrap_or(-1)
            )))
        }
    }

    fn find_runner<P: AsRef<Path>>(&self, root: P) -> Result<PathBuf> {
        let job_dir = root.as_ref().join(&self.name);
        let default = job_dir.join("run.sh");
        if default.is_file() {
            return Ok(default);
        }
        let pattern = format!("{}/run.*", job_dir.display());
        Ok(glob::glob(&pattern)?
            .next()
            .ok_or_else(|| anyhow!("No runner found"))??)
    }

    pub fn run<P: AsRef<Path>>(&self, root: P) -> Result<()> {
        let env = self.create_proc_env(&root)?;
        if self.has_deps_script {
            let deps_runnable = root.as_ref().join(&self.name).join(DEPS_SCRIPT);
            self.run_process(&env, deps_runnable)?;
        };
        let runner = self.find_runner(root)?;
        self.run_process(&env, runner)
    }
}

#[inline]
fn job_style() -> Style {
    Style::new().blue().bold()
}

#[inline]
fn info_style() -> Style {
    Style::new().dim()
}

#[inline]
fn encode_key(key: &str) -> String {
    key.to_uppercase().replace("-", "_").replace(" ", "_")
}

fn ensure_executable<P: AsRef<Path>>(file: P) -> Result<()> {
    if is_executable::is_executable(&file) {
        return Ok(());
    };
    let mode: u32 = fs::metadata(&file)?.permissions().mode() | 100;
    fs::set_permissions(&file, Permissions::from_mode(mode))?;
    Ok(())
}

fn get_job_names<P: AsRef<Path>>(root: P) -> Result<Vec<String>> {
    let pattern = root.as_ref().join("*/run.*").display().to_string();
    let mut match_collector = Vec::<String>::new();
    let mut hit_error = false;
    for runfile in glob::glob(&pattern)? {
        if let Ok(path) = runfile {
            let name = path
                .parent()
                .ok_or(anyhow!(format!("Unexpectable path {}", path.display())))?
                .file_name()
                .ok_or(anyhow!(format!("Unusable directory name {}", path.display())))?;
            if let Some(valid_name) = name.to_str() {
                match_collector.push(valid_name.to_string());
            } else {
                hit_error = true;
                eprintln!("Invalid job name: {}", name.to_string_lossy());
            }
        } else {
            hit_error = true;
            let glob_err = runfile.unwrap_err(); // We know it's an error.
            eprintln!("GlobError: {}", glob_err);
        }
    }
    if hit_error {
        Err(anyhow!("Failed to retrieve job names"))
    } else {
        Ok(match_collector)
    }
}

fn parse_job_files<P: AsRef<Path>>(name: &str, root: P) -> Result<JobSpec> {
    debug!("Parsing job files: {}", name);
    let script_dir = root.as_ref().join(name);
    let has_deps_script = script_dir.join(DEPS_SCRIPT).is_file();
    let info_spec = parse_info_file(&script_dir)?;
    Ok(JobSpec::new(
        name.to_owned(),
        info_spec.env.unwrap_or_default(),
        info_spec.depends.unwrap_or_default(),
        info_spec.ask.unwrap_or_default(),
        has_deps_script,
    ))
}

fn parse_info_file<P: AsRef<Path>>(root: P) -> Result<InfoSpec> {
    let info_path = root.as_ref().join(INFO_FILE);
    if !info_path.exists() {
        // no file, fall back to default settings
        return Ok(InfoSpec::default());
    };
    debug!("Parsing info file: {}", info_path.display());
    let file = File::open(info_path)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

fn try_empty_var(config: &Config) -> Option<String> {
    if config.empty_vars {
        Some(String::default())
    } else {
        None
    }
}

fn try_var_from_env(raw: &str, config: &Config) -> Option<String> {
    if config.allow_env {
        if let Ok(val) = env::var(secure_name_check(raw).0) {
            return Some(val);
        }
    }
    None
}

fn try_var_from_cmd(name: &str, config: &Config) -> Option<String> {
    config.get_cmd_var(name)
}

fn try_var_from_askfile(name: &str, config: &Config) -> Option<String> {
    config.get_file_var(name)
}

fn try_ask_user_for_var(name: &str, config: &Config, secure: bool) -> Option<String> {
    if !config.interactive {
        return None;
    };

    let message = "Please enter the value for the variable";

    if secure {
        let prompt = format!("<Secure> {} [{}]", message, name);
        PasswordInput::new()
            .with_prompt(&prompt)
            .allow_empty_password(true)
            .interact()
    } else {
        let prompt = format!("{}, [{}]", message, name);
        Input::new()
            .with_prompt(&prompt)
            .allow_empty(true)
            .interact()
    }
    .ok()
}

fn query_single_var(name: &str, config: &Config) -> Result<(String, String)> {
    let (runnable_name, is_secure) = secure_name_check(name);

    let value = try_empty_var(config)
        .or_else(|| try_var_from_env(&runnable_name, config))
        .or_else(|| try_var_from_cmd(&runnable_name, config))
        .or_else(|| try_var_from_askfile(&runnable_name, config))
        .or_else(|| try_ask_user_for_var(&runnable_name, config, is_secure))
        .ok_or(anyhow!(format!("Cound not resolve var: {}", runnable_name)))?;

    Ok((runnable_name, value))
}

fn query_vars(specs: &[JobSpec], config: &Config) -> Result<EnvMap> {
    let var_names: Vec<&str> = specs
        .iter()
        .flat_map(JobSpec::get_ask_vars)
        .map(String::as_str)
        .collect();
    let mut new_env = EnvMap::new();

    for name in var_names {
        if new_env.contains_key(name) {
            continue;
        }
        let (key, value) = query_single_var(name, config)?;
        new_env.insert(key, value);
    }

    Ok(new_env)
}

fn fill_asked_vars(mut spec: JobSpec, answers: &EnvMap) -> Result<ReadyJob> {
    let mut map = EnvMap::new();
    for raw in spec.ask_for_vars {
        let (name, _) = secure_name_check(raw);
        if let Some(value) = answers.get(&name) {
            map.insert(name, value.to_owned());
        } else {
            return Err(anyhow!(format!("Unresolvable variable: {}", name)));
        }
    }

    for (k, v) in spec.provided_env.drain() {
        map.insert(encode_key(&k), v);
    }

    Ok(ReadyJob::new(
        spec.name,
        map,
        spec.depends,
        spec.has_deps_script,
    ))
}

fn schedule_specs(jobs: &[ReadyJob]) -> Result<Vec<ReadyJob>> {
    let required_count = jobs.len();
    let mut scheduled = Vec::with_capacity(required_count);

    let mut scheduled_names = HashSet::with_capacity(required_count);

    macro_rules! schedule {
        ($x:expr) => {
            scheduled_names.insert(&$x.name);
            scheduled.push($x.clone());
        };
    }

    for job in jobs {
        if job.depends.is_empty() {
            schedule!(job);
        }
    }

    while scheduled.len() < required_count {
        let sched_count = scheduled.len();

        for job in jobs {
            if scheduled_names.contains(&job.name) {
                continue;
            }

            if job
                .depends
                .iter()
                .all(|name| scheduled_names.contains(name))
            {
                schedule!(job);
            }
        }

        // Compare the count at the beginning to the current.
        // If the count doesn't change, we've hit an unresolvable cycle.
        if scheduled.len() == sched_count {
            return Err(cycle_error(&scheduled_names, jobs));
        }
    }

    Ok(scheduled)
}

fn cycle_error(scheduled: &HashSet<&String>, all: &[ReadyJob]) -> Error {
    let v: Vec<String> = all
        .iter()
        .filter_map(|j| {
            if scheduled.contains(&j.name) {
                None
            } else {
                Some((&j.name).clone())
            }
        })
        .collect();
    anyhow!(format!("Unschedulable jobs: {}", v.join(", ")))
}

fn report_jobs(jobs: &[ReadyJob]) {
    for (position, job) in jobs.iter().enumerate() {
        println!("{}", job.report(position));
    }
}

fn run_all_jobs<P: AsRef<Path>>(root: P, config: &Config) -> Result<()> {
    info!(
        "Retrieving job names from root: {}",
        root.as_ref().display()
    );
    let names: Vec<String> = get_job_names(root.as_ref())?;

    info!("Parsing job files");
    let specs: Vec<JobSpec> = names
        .into_iter()
        .map(|name| parse_job_files(&name, root.as_ref()))
        .collect::<Result<Vec<JobSpec>, Error>>()?;

    info!("Querying ask variables");
    let asked_vars: EnvMap = query_vars(&specs, config)?;

    info!("Populating asked variables");
    let respecs: Vec<ReadyJob> = specs
        .into_iter()
        .map(|spec| fill_asked_vars(spec, &asked_vars))
        .collect::<Result<Vec<ReadyJob>, Error>>()?;

    info!("Scheduling jobs");
    let queue: Vec<ReadyJob> = schedule_specs(&respecs)?;

    if config.dry_run {
        report_jobs(&queue);
        return Ok(());
    };
    if let Some(jobname) = &config.single_job {
        queue
            .iter()
            .find(|job| &job.name == jobname)
            .ok_or(anyhow!(format!("Cannot locate job: {}", jobname)))?
            .run(&root)
    } else {
        queue.iter().try_for_each(|job| job.run(&root))
    }
}

fn inner_main() -> Result<()> {
    let config: Config = Opt::from_args().try_into()?;
    run_all_jobs(&config.root_dir, &config)
}

fn main() {
    env_logger::init();
    if let Err(e) = inner_main() {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}
