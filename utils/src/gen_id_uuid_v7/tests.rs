use crate::UtilsError;

crate::gen_id_uuid_v7!(DefaultId);
crate::gen_id_uuid_v7!(SlottedId, Slot);

#[test]
fn default_id_parsing_keeps_utils_error() {
    let id = DefaultId::new();
    assert_eq!(id.to_string().parse::<DefaultId>().unwrap(), id);
    assert!(matches!(
        "invalid-id".parse::<DefaultId>(),
        Err(UtilsError::ParseId(_))
    ));
}

#[test]
fn slot_ids_keep_reserved_values_and_parsing() {
    assert_eq!(SlottedId::new().to_slot(), None);
    for slot in [Slot::Slot0, Slot::Slot1] {
        let id = SlottedId::from_slot(slot);
        assert_eq!(id.to_slot(), Some(slot));
        assert_eq!(id.to_string().parse::<SlottedId>().unwrap(), id);
    }
}
