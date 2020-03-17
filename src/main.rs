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
mod jobs;
mod vars;

use std::borrow::ToOwned;
use std::collections::HashSet;
use std::convert::TryInto;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process;

use anyhow::{anyhow, Error, Result};
use structopt::StructOpt;

use cli::Opt;
use common::{EnvMap, DEPS_SCRIPT, INFO_FILE};
use config::Config;
use info::InfoSpec;
use jobs::{JobSpec, ReadyJob};
use vars::{fill_asked, query};

fn cycle_error(scheduled: &HashSet<&String>, all: &[ReadyJob]) -> Error {
    let v: Vec<String> = all
        .iter()
        .filter_map(|j| {
            if scheduled.contains(j.name()) {
                None
            } else {
                Some((j.name()).clone())
            }
        })
        .collect();
    anyhow!(format!("Unschedulable jobs: {}", v.join(", ")))
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
                .ok_or(anyhow!(format!(
                    "Unusable directory name {}",
                    path.display()
                )))?;
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
    let asked_vars: EnvMap = query(&specs, config)?;

    info!("Populating asked variables");
    let respecs: Vec<ReadyJob> = specs
        .into_iter()
        .map(|spec| fill_asked(spec, &asked_vars))
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
            .find(|job| job.name() == jobname)
            .ok_or(anyhow!(format!("Cannot locate job: {}", jobname)))?
            .run(&root)
    } else {
        queue.iter().try_for_each(|job| job.run(&root))
    }
}

fn schedule_specs(jobs: &[ReadyJob]) -> Result<Vec<ReadyJob>> {
    let required_count = jobs.len();
    let mut scheduled = Vec::with_capacity(required_count);

    let mut scheduled_names: HashSet<&String> = HashSet::with_capacity(required_count);

    macro_rules! schedule {
        ($x:expr) => {
            debug!("Schedule: {}", $x.name());
            scheduled_names.insert(&$x.name());
            scheduled.push($x.clone());
        };
    }

    for job in jobs {
        if job.depends().is_empty() {
            schedule!(job);
        }
    }

    while scheduled.len() < required_count {
        let sched_count = scheduled.len();

        for job in jobs {
            if scheduled_names.contains(&job.name()) {
                continue;
            }

            if job
                .depends()
                .iter()
                .all(|name| scheduled_names.contains(&name))
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
