mod cli;
mod detect;
mod discover;
mod mcp;
mod scan;

#[cfg(test)]
mod testing;

fn main() -> std::process::ExitCode {
    cli::run()
}
