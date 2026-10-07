use super::strategy::IdStrategy;

pub use nid::Nanoid as NanoID;

impl IdStrategy for NanoID {
    fn generate() -> Self {
        Self::new()
    }
}
