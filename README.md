# Devmaker
Set up a fresh ubuntu vm with one command.

## The basics

Ok, I don't have a real doc for this yet, so here's a super short outline.

`devmaker` starts at a root directory.
It globs for directories at this level which contain a `run.*` file.
Using those directories as job names, it scans the directory for 3 files:

* `run.*`
* `deps.sh` - optional
* `info.json` - optional

We then collect all *askable* variables (variables which must be given at runtime) from
the `ask` key of the `info.json` files.  If two files share the same askable variable name,
they will share the value as well.  If the variable name ends with `_SECURE`, that suffix is
stripped from the name during all subsequent phases, and that variable will be treated as
a password-like value in certain situations (more on that later).

After aggregating the names, we populate their values from one of 5 sources.  Mostly, this
is configured from the command line.  The sources are evaluated in this order, stopping
at the first activated source that has some value.

1. **Auto-fill with empty string** - only if `-e/--force-empty-vars` is set.  Mainly for testing.
2. **Use command-line-provided variables** - provided by `-w/--with-vars VAR`.  Run `devmaker --help`
   for more info.
3. **Pull from environment variables** - can be disabled using `-E/--no-allow-env` flags.
4. **Read from an askfile** - only used when specified with `-a/--askfile FILE`.
5. **Interactively prompt the user** - only enabled when `-i/--interactive` is set.  If the
   variable is secure, we provide a hidden input prompt which does not echo to the terminal.
   
Once we finish asking for variable values, we run each job, one-by-one, based on the `depends`
key given in the `info.json` file.  The ordering is unspecified, except that a job will not
be run before another job it depends on.  **THE ORDERING IS UNSPECIFIED.  DON'T RELY ON IT!**

A job run is very simple.  Before each process is run, we update the environment with any
provided and asked variables.  Then we run the `deps.sh` script if it exists, skipping if
it doesn't.  Then we run the `run.*` file found earlier, known as the *runner*.  If either
process returns a non-zero exit code, the job will stop executing, the error will be
reported, and no further jobs will run.

## Releases

Everything is done through github actions.  Releases are done by pushing to the repo.
Code must pass rustfmt and clippy checks, and will build and check on PR's and master
branch pushes.  If a tag is pushed, a draft release is created as well.  Cool beens.
