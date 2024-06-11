use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Args, Debug)]
#[command(arg_required_else_help(true))]
pub struct PushArgs {
    pub source: PathBuf,
    pub dest: PathBuf,

    /// delete files on target that does not exist in source
    #[arg(short = 'd', long)]
    pub delete_if_dne: bool,

    /// ignore dirs starting with specified string
    #[arg(short, long)]
    pub ignore_dir: Vec<Box<str>>,
}

#[derive(Args, Debug)]
#[command(arg_required_else_help(true))]
pub struct PullArgs {
    pub source: PathBuf,
    pub dest: Option<PathBuf>,

    /// delete files on target that does not exist in source
    #[arg(short = 'd', long)]
    pub delete_if_dne: bool,

    /// ignore dirs starting with specified string
    #[arg(short, long)]
    pub ignore_dir: Vec<Box<str>>,

    /// set modified time of files
    #[arg(short = 't', long)]
    pub set_times: bool,
}

#[derive(Debug, Subcommand)]
pub enum SubCmds {
    Pull(PullArgs),
    Push(PushArgs),
}

#[derive(Parser, Debug)]
#[command(
    help_template = "{author-with-newline}{about-section}Version: {version}\n{usage-heading} \
    {usage}\n{all-args} {tab}"
)]
#[command(arg_required_else_help(true))]
#[clap(version = "1.0", author = "github.com/j-hc")]
pub struct Cli {
    #[clap(subcommand)]
    pub subcmd: SubCmds,
}
