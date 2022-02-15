use sloth_lang_core::*;

use clap::Parser;

/// simple interface for invocating sloth-lang interpreter
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// source file name
    #[clap(short, long)]
    file: String,
}

fn main() {
    let args = Args::parse();
    println!("openning {}...", args.file);
    use std::fs;
    let file_to_intetpret =
        fs::read_to_string(&args.file).unwrap_or_else(|_| panic!("cannot open file {}", args.file));
    println!("running");
    run_string(&file_to_intetpret).unwrap();
}
