fn main() -> std::process::ExitCode {
    if botstral_link::release::print_version("desktop") {
        return std::process::ExitCode::SUCCESS;
    }
    if let Some(result) = botstral_desktop::release_acceptance::run() {
        return result;
    }
    botstral_desktop::shell::run();
    std::process::ExitCode::SUCCESS
}
