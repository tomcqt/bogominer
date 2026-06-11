//! bogominer by tomcat & more
//! an official open-source native bogosort miner for swapjs' bogostream.

#![feature(portable_simd)]

mod app;
mod backend;
mod misc;

fn main() {
    app::run();
}
