//! Core data types, enums, and market data structures.
//!
//! All `#[repr(C)]` structs use fixed-size symbol arrays (`[u8; 32]`) so they
//! can be stored in shared memory without heap allocation.

pub mod enums;
pub mod market_data;
pub mod symbol;
pub mod trading;

pub use enums::*;
pub use market_data::*;
pub use symbol::*;
pub use trading::*;
