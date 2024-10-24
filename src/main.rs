use sloth_lang_core::*;

use std::{io::Read, path::PathBuf};

use clap::{Arg, Parser};

#[derive(Parser)]
#[command(version, about = "sloth-lang interpreter", long_about = None)]
struct Cli {
    /// script path, execution start here
    script: PathBuf,
    #[arg(short, long)]
    debug: bool,
}
fn main() {
    let args = Cli::parse();
    let cwd = std::env::current_dir().unwrap();
    let mut full_path = PathBuf::new();
    full_path.push(&cwd);
    full_path.push(&args.script);
    let mut file = std::fs::File::open(full_path).unwrap();
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).unwrap();
    let res = run_string_debug(&buffer, false, args.debug);
    if args.debug {
        eprintln!("{res:?}");
    }
}
