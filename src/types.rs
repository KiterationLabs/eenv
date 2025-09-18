use clap::ValueEnum;

#[derive(ValueEnum, Clone, Debug)]
pub enum HookAction { Install, Uninstall }

#[derive(Debug, Clone, Copy)]
pub struct EenvState {
    pub enc: bool,
    pub example: bool,
    pub env: bool,
    pub eenvjson: bool,
}