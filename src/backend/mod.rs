//! AOT backend: LLVM IR generation, program image serialization and the
//! `llc`/linker driver.

pub mod driver;
pub mod image;
pub mod llvm;
pub mod modules;

pub use driver::{build_executable, BuildOptions, EmitKind};
pub use image::{deserialize, serialize, ImageRoot};
