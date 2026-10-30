fn main() -> std::process::ExitCode {
    if botstral_link::release::print_version("agent") {
        return std::process::ExitCode::SUCCESS;
    }
    botstral_agent::entry(false)
}
