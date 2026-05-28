use std::io::IsTerminal;

const RESET: &str = "\x1b[0m";

pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const YELLOW: &str = "\x1b[33m";
pub(crate) const RED: &str = "\x1b[31m";

pub(crate) fn ansi_enabled(stream: &impl IsTerminal) -> bool {
    if env_non_empty("NO_COLOR") {
        return false;
    }
    if env_non_empty("CLICOLOR_FORCE") {
        return true;
    }
    stream.is_terminal()
}

pub(crate) fn paint(text: &'static str, color: &str, enabled: bool) -> String {
    if enabled {
        format!("{color}{text}{RESET}")
    } else {
        text.to_string()
    }
}

fn env_non_empty(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}
