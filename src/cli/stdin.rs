use crate::error::Result;
use serde_json::Value;
use std::io::Read;

/// Read and parse JSON from stdin.
pub fn read_stdin_json() -> Result<Value> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let value = serde_json::from_str(&buf)?;
    Ok(value)
}
