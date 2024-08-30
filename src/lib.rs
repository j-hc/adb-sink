pub mod adb;
pub mod args;
pub mod fs;

use std::sync::OnceLock;

pub static VERBOSE: OnceLock<bool> = OnceLock::new();

pub fn is_verbose() -> bool {
    *VERBOSE.get().expect("set in main")
}

#[macro_export]
macro_rules! logi {
    ($($arg:tt)*) => {{
		print!("[INFO] ");
        println!($($arg)*);
    }};
}

#[macro_export]
macro_rules! logw {
    ($($arg:tt)*) => {{
		print!("[WARN] ");
        println!($($arg)*);
    }};
}

#[macro_export]
macro_rules! logv {
    ($($arg:tt)*) => {{
        if ::adb_sink::is_verbose() {
            print!("[VERBOSE] ");
            println!($($arg)*);
        }
    }};
}
