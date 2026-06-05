//! bogominer by tomcat & more
//! an open-source native bogosort miner for swapjs' bogostream.

mod api;
mod config;
mod gui;
mod misc;
mod pool;
mod protocol;
mod rng;
mod solver;
mod stats;
mod tui;
mod worker;

use crate::misc::parse_cpu_tier;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "bogo", version, about)]
struct Args {
    #[arg(long)]
    tui: bool,

    #[arg(long, default_value = "max")]
    cpu: String,

    #[arg(long)]
    code: Option<String>,

    #[arg(long)]
    autostart: bool,
}

fn main() {
    let args = Args::parse();
    let cpu_target = parse_cpu_tier(&args.cpu);

    if args.tui {
        tui::run(cpu_target, args.code, args.autostart);
    } else {
        gui::run(cpu_target, args.code, args.autostart);
    }
}
