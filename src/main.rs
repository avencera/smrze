use clap::Parser;
use tracing::error;

fn main() {
    let cli = smrze::Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(if cli.quiet {
            tracing_subscriber::EnvFilter::new("error")
        } else {
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error,smrze=warn"))
        })
        .with_target(false)
        .with_ansi(true)
        .with_writer(std::io::stderr)
        .init();

    color_eyre::install().expect("failed to install color-eyre");

    if let Err(error) = smrze::run(cli) {
        error!("{error:#}");
        std::process::exit(1);
    }
}
