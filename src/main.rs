mod analysis;
mod converter;
mod error;

use error::AppError;
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

fn main() {
    println!("Hello, world!");

    // analyze -> decide -> encode -> evaluate
}
