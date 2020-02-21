use serde::Deserialize;

use crate::common::EnvMap;

#[derive(Deserialize, Debug, Default)]
pub(crate) struct InfoSpec {
    pub depends: Option<Vec<String>>,
    pub env: Option<EnvMap>,
    pub ask: Option<Vec<String>>,
}
