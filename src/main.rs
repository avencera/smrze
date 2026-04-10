fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error,smrze=warn")),
        )
        .with_target(false)
        .with_ansi(true)
        .init();

    color_eyre::install().expect("failed to install color-eyre");

    if let Err(error) = smrze::run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
