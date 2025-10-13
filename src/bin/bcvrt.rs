use std::env;
use std::ffi::OsString;
use std::process::{Command, exit};

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();

    match Command::new("bunker-convert").args(&args).status() {
        Ok(status) => {
            if let Some(code) = status.code() {
                if code != 0 {
                    exit(code);
                }
            } else {
                exit(1);
            }
        }
        Err(err) => {
            eprintln!("Failed to invoke bunker-convert: {err}");
            exit(1);
        }
    }
}
