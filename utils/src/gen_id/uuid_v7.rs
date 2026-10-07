use super::strategy::IdStrategy;

pub use uuid::Uuid as UUID_v7;

impl IdStrategy for UUID_v7 {
    fn generate() -> Self {
        Self::now_v7()
    }
}
