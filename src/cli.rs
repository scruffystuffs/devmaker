use std::path::PathBuf;

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about = "Apply startup scripts to a dev machine")]
pub(crate) struct Opt {
    /// Allow Devmaker to ask for askable vars interactively.
    #[structopt(short, long)]
    pub interactive: bool,

    /// Don't actually run anything, just report on how the process would have run.
    #[structopt(short = "n", long)]
    pub dry_run: bool,

    /// Don't try to pull askable vars from env variables.
    #[structopt(short = "E", long)]
    pub no_allow_env: bool,

    /// A `VARNAME=value` formatted file to read vars from.
    #[structopt(short, long)]
    pub ask_file: Option<String>,

    /// One or more strings in the format `VARNAME=value`.
    #[structopt(short = "w", long = "with-vars")]
    pub ask_vars: Option<Vec<String>>,

    /// A single job to run, ignoring dependencies.
    #[structopt(short, long)]
    pub single_job: Option<String>,

    /// Sets all queried vars to empty strings.  Useful for testing.
    #[structopt(short = "e", long)]
    pub force_empty_vars: bool,

    /// The root directory conatining all job specs.
    #[structopt(index = 1)]
    pub script_root: PathBuf,
}
