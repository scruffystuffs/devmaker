use std::collections::HashMap;

pub(crate) const DEPS_SCRIPT: &str = "deps.sh";
pub(crate) const INFO_FILE: &str = "info.json";
pub(crate) const SECURE_SUFFIX: &str = "_SECURE";

pub(crate) type EnvMap = HashMap<String, String>;

pub(crate) fn secure_name_check<S: Into<String>>(name: S) -> (String, bool) {
    let mut name = name.into();
    let has_secure_suffix = name.ends_with(SECURE_SUFFIX);
    if has_secure_suffix {
        name = name.replace(SECURE_SUFFIX, "");
    }
    (name, has_secure_suffix)
}
