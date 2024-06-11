pub mod adb;
pub mod args;
pub mod fs;

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
		print!("[INFO] ");
        println!($($arg)*);
    }};
}
