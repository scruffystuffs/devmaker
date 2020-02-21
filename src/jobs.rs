use std::ffi::OsStr;
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{anyhow, Result};
use console::Style;
use derive_getters::Getters;
use serde::Serialize;
use tempdir::TempDir;

use crate::common::{EnvMap, DEPS_SCRIPT};

#[derive(Debug, Serialize)]
pub(crate) struct JobSpec {
    pub name: String,
    pub provided_env: EnvMap,
    pub depends: Vec<String>,
    pub ask_for_vars: Vec<String>,
    pub has_deps_script: bool,
}

#[derive(Clone, Debug, Getters)]
pub(crate) struct ReadyJob {
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
    pub const fn get_ask_vars(&self) -> &Vec<String> {
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
            map.insert(k.clone(), v.clone());
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

fn ensure_executable<P: AsRef<Path>>(file: P) -> Result<()> {
    if is_executable::is_executable(&file) {
        return Ok(());
    };
    let mode: u32 = fs::metadata(&file)?.permissions().mode() | 100;
    fs::set_permissions(&file, Permissions::from_mode(mode))?;
    Ok(())
}

#[inline]
fn info_style() -> Style {
    Style::new().dim()
}

#[inline]
fn job_style() -> Style {
    Style::new().blue().bold()
}
