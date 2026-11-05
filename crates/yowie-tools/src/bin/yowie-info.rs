//! Prints the GPU stack without opening a window: the wgpu version, every
//! adapter with its backend, type and driver, its features and key limits,
//! and whether BC compression, timestamp queries and immediates are available.

fn main() {
    print!("{}", yowie_tools::info::report());
}
