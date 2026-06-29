mod args;
mod command;
mod execute;
mod output;

pub use execute::run_from_env;

#[cfg(test)]
mod tests;
