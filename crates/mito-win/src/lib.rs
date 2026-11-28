//! Capa FFI de Windows. Es el único crate del workspace que puede contener `unsafe`; cada bloque debe documentar su invariante con `// SAFETY:`.

pub mod clipboard;
pub mod console;
pub mod files;
pub mod jobs;
pub mod links;
pub mod msi;
pub mod network;
pub mod registry;
pub mod resources;
pub mod shell;
pub mod system;
pub mod taskbar;
