// Docs: docs/souls-format/lib.md

pub mod bnd4;
mod cursor;
pub mod dcx;
/// Shared fixtures for tests that read the real game install. Compiled only under `cfg(test)`.
#[cfg(test)]
mod test_support;
pub mod fmg;
pub mod fxr;
pub mod item_text;
pub mod locate;
pub mod names;
pub mod oodle;
pub mod param_bank;
pub mod paramdef;
pub mod paramdex;
#[cfg(feature = "write")]
pub mod rebuild;
pub mod regulation;
pub mod sfx_dir;
#[cfg(feature = "write")]
pub mod write;

pub use fxr::Fxr;
pub use item_text::ItemText;
pub use names::NameIndex;
pub use param_bank::{ParamBank, RowResult};
pub use paramdef::ParamRow;
pub use paramdex::ParamdefLibrary;
pub use regulation::Regulation;
pub use sfx_dir::SfxDirectory;
