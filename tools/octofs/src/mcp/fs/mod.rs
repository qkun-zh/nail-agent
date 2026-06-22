















pub mod core;
pub mod directory;
pub mod file_ops;
pub mod search;
pub mod shell;
pub mod text_editing;
pub mod workdir;

#[cfg(test)]
mod fs_tests;


pub use core::{execute_batch_edit, execute_extract_lines, execute_text_editor, execute_view};

pub use shell::execute_shell_command;
pub use workdir::{execute_workdir_command, WorkdirResult};
