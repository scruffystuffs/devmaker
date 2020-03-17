use std::env;

use anyhow::{anyhow, Result};
use dialoguer::{Input, PasswordInput};

use crate::common::{secure_name_check, EnvMap};
use crate::config::Config;
use crate::jobs::{JobSpec, ReadyJob};

#[inline]
fn encode_key(key: &str) -> String {
    key.to_uppercase().replace("-", "_").replace(" ", "_")
}

fn query_single_var(name: &str, config: &Config) -> Result<(String, String)> {
    let (runnable_name, is_secure) = secure_name_check(name);

    debug!("Querying var: {}", runnable_name);

    let value = try_empty_var(&runnable_name, config)
        .or_else(|| try_var_from_env(&runnable_name, config))
        .or_else(|| try_var_from_cmd(&runnable_name, config))
        .or_else(|| try_var_from_askfile(&runnable_name, config))
        .or_else(|| try_ask_user_for_var(&runnable_name, config, is_secure))
        .ok_or(anyhow!(format!("Cound not resolve var: {}", runnable_name)))?;

    Ok((runnable_name, value))
}

fn try_ask_user_for_var(name: &str, config: &Config, secure: bool) -> Option<String> {
    if !config.interactive {
        return None;
    };

    debug!("Interactive query: {}", name);

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

fn try_empty_var(name: &str, config: &Config) -> Option<String> {
    if config.empty_vars {
        debug!("No-fill: {}", name);
        Some(String::default())
    } else {
        None
    }
}

fn try_var_from_askfile(name: &str, config: &Config) -> Option<String> {
    debug!("Trying askfile for var: {}", name);
    config.get_file_var(name)
}

fn try_var_from_cmd(name: &str, config: &Config) -> Option<String> {
    debug!("Trying cmd line for var: {}", name);
    config.get_cmd_var(name)
}

fn try_var_from_env(name: &str, config: &Config) -> Option<String> {
    if config.allow_env {
        debug!("Trying environment for var: {}", name);

        if let Ok(val) = env::var(name) {
            return Some(val);
        }
    }
    None
}

pub(crate) fn fill_asked(mut spec: JobSpec, answers: &EnvMap) -> Result<ReadyJob> {
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

pub(crate) fn query(specs: &[JobSpec], config: &Config) -> Result<EnvMap> {
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
