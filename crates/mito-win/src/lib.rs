//! Capa FFI de Windows. Es el único crate del workspace que puede contener `unsafe`; cada bloque debe documentar su invariante con `// SAFETY:`.

pub mod clipboard;
pub mod console;
pub mod files;
pub mod links;
pub mod registry;
pub mod resources;
pub mod system;
pub mod taskbar;
