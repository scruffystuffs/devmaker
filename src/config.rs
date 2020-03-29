use std::convert::TryFrom;
use std::fs::File;
use std::io::{prelude::*, BufReader};
use std::path::{Path, PathBuf};
use std::result::Result as StdResult;

use anyhow::{anyhow, Error, Result};
use regex::Regex;

use crate::cli::Opt;
use crate::common::{secure_name_check, EnvMap};

pub(crate) struct Config {
    pub ask_file_vars: Option<EnvMap>,
    pub cmd_vars: Option<EnvMap>,
    pub root_dir: PathBuf,
    pub single_job: Option<String>,

    pub allow_env: bool,
    pub dry_run: bool,
    pub empty_vars: bool,
    pub interactive: bool,
}

/// these functions are tough with the borrow checker.
/// it's just map.get(name), but with option handling and reference balancing
macro_rules! opt_map_helper {
    ($map:expr, $name:expr) => {
        ($map.as_ref()?)
            .get(&secure_name_check($name.as_ref()).0)
            .cloned()
    };
}

impl Config {
    pub fn get_cmd_var<S: AsRef<str>>(&self, name: S) -> Option<String> {
        opt_map_helper!(&self.cmd_vars, name)
    }

    pub fn get_file_var<S: AsRef<str>>(&self, name: S) -> Option<String> {
        opt_map_helper!(&self.ask_file_vars, name)
    }
}

impl TryFrom<Opt> for Config {
    type Error = Error;
    fn try_from(o: Opt) -> StdResult<Self, Self::Error> {
        let allow_env = !&o.no_allow_env;
        let ask_file_vars = if let Some(file) = o.ask_file {
            parse_askfile(file)?
        } else {
            None
        };
        let cmd_vars = if let Some(pairs) = o.ask_vars {
            parse_cmd_vars(pairs)?
        } else {
            None
        };
        let dry_run = o.dry_run;
        let empty_vars = o.force_empty_vars;
        let interactive = o.interactive;
        let root_dir: PathBuf = o.script_root;
        let single_job = o.single_job;

        Ok(Self {
            allow_env,
            ask_file_vars,
            cmd_vars,
            dry_run,
            empty_vars,
            interactive,
            root_dir,
            single_job,
        })
    }
}

fn try_parse_var_string(line: &str, from: &str) -> Result<Option<(String, String)>> {
    let pattern = Regex::new(r"^\s*([A-Z\d][A-Z\d_]+)\s*=\s*(.+?)\s*$")?;
    let captures = pattern.captures(line).ok_or(anyhow!(format!(
        "Unparseable line found in {}: {}",
        from, line
    )))?;
    // The groups are not optional
    let key = captures
        .get(1)
        .ok_or_else(|| anyhow!("Capture group 1 did not match"))?
        .as_str()
        .to_owned();
    let value = captures
        .get(2)
        .ok_or_else(|| anyhow!("Capture group 2 did not match"))?
        .as_str()
        .to_owned();

    Ok(Some((key, value)))
}

fn parse_var_strings<I: IntoIterator<Item = String>>(iter: I) -> Result<Option<EnvMap>> {
    let mut map = EnvMap::new();
    for pair in iter {
        if let Some((key, value)) = try_parse_var_string(&pair, "askfile")? {
            // Overwrite conflicting lines
            map.insert(key, value);
        }
    }
    Ok(Some(map))
}

fn parse_askfile<P: AsRef<Path>>(file: P) -> Result<Option<EnvMap>> {
    debug!("Parsing askfile: {}", file.as_ref().display());
    let reader = BufReader::new(File::open(file)?);
    let pairs: Vec<_> = reader.lines().collect::<Result<_, _>>()?;
    parse_var_strings(pairs)
}

fn parse_cmd_vars(pairs: Vec<String>) -> Result<Option<EnvMap>> {
    parse_var_strings(pairs)
}
