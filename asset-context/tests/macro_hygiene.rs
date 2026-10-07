use asset_context::{ContextBuilder, context, register};

type Result<T> = ::core::result::Result<T, &'static str>;
struct Ok;

#[context(init = initialize)]
struct AttributeValue(u32);

fn initialize() -> Result<AttributeValue> {
    ::core::result::Result::Ok(AttributeValue(42))
}

#[derive(Default)]
struct ExistingValue(u32);

register!(ExistingValue);

#[test]
fn registrations_ignore_shadowed_result_and_ok_names() {
    let _ = Ok;
    let context = ContextBuilder::from_auto().unwrap().build().unwrap();
    assert_eq!(context.get::<AttributeValue>().unwrap().0, 42);
    assert_eq!(context.get::<ExistingValue>().unwrap().0, 0);
}
