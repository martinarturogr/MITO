//! Capa FFI de Windows. Es el único crate del workspace que puede contener `unsafe`; cada bloque debe documentar su invariante con `// SAFETY:`.

pub mod files;
pub mod links;
pub mod system;
