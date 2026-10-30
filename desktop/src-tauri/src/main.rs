fn main() {
    if botstral_link::release::print_version("desktop") {
        return;
    }
    botstral_desktop::shell::run();
}
