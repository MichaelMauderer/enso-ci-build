use crate::prelude::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct Cargo;

impl Program for Cargo {
    fn executable_name() -> &'static str {
        "cargo"
    }
}
