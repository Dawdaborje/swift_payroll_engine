use clap::Parser;
use swift_payroll::cli;

fn main() {
    let args = cli::args::CliArgs::parse();
    if let Err(e) = cli::run(&args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
