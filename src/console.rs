use colored::Colorize;
use std::sync::{
    LazyLock,
    atomic::{AtomicBool, Ordering},
};

static QUIET: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

pub(crate) fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

pub(crate) fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub(crate) fn info(message: impl AsRef<str>) {
    if is_quiet() {
        return;
    }

    eprintln!("{} {}", "info".cyan().bold(), message.as_ref());
}
