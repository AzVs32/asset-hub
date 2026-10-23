#[derive(ids::Id, Debug, Clone, Copy, PartialEq, Eq)]
#[id(generate, display, parse)]
struct MountId(ids::UuidV7);

fn main() -> Result<(), ids::InvalidIdError> {
    let id = MountId::new();
    assert_eq!(id.to_string().parse::<MountId>()?, id);
    Ok(())
}
