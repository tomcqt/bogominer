//! bogominer by tomcat & more
//! an official open-source native bogosort miner for swapjs' bogostream.

#![feature(portable_simd)]

mod app;
mod backend;
mod compute;
mod misc;

fn main() {
    app::run();
}
