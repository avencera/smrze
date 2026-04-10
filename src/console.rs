use colored::Colorize;

pub(crate) fn info(message: impl AsRef<str>) {
    eprintln!("{} {}", "info".cyan().bold(), message.as_ref());
}

pub(crate) fn success(message: impl AsRef<str>) {
    eprintln!("{} {}", "done".green().bold(), message.as_ref());
}
