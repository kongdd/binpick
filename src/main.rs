mod app;
mod artifacts;
mod catalog;
mod cli;
mod files;
mod github;
mod installer;
mod maintenance;
mod model;
mod platform;

#[cfg(test)]
mod unit_tests;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
