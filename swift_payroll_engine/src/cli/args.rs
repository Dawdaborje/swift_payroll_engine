use clap::Parser;

#[derive(Debug, Parser)]
pub struct CliArgs {
    /// Path to the JSON file containing employee data
    #[arg(short, long, value_name = "FILE")]
    pub input: Option<std::path::PathBuf>,

    /// Server address for optional serve mode (existing)
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    pub serve: Option<String>,
}
