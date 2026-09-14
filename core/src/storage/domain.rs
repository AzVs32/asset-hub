use getset::{CopyGetters, Getters};

#[derive(Debug, Clone, PartialEq, Eq, Getters, CopyGetters)]
pub struct StorageSource {
    #[getset(get_copy = "pub")]
    id: SourceId,
}

utils::gen_id_uuid_v7!(SourceId);
