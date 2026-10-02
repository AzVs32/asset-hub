use crate::{ContextBuilder, ContextError};

#[doc(hidden)]
pub struct AutoRegistration {
    pub type_name: fn() -> &'static str,
    pub register: fn(&mut ContextBuilder) -> Result<(), ContextError>,
}

#[linkme::distributed_slice]
pub static AH_CONTEXT_AUTO_REGISTRATIONS: [AutoRegistration];

pub(crate) fn collect(registrations: &[AutoRegistration]) -> Result<ContextBuilder, ContextError> {
    let mut registrations: Vec<_> = registrations.iter().collect();
    registrations.sort_unstable_by_key(|item| (item.type_name)());

    let mut builder = ContextBuilder::new();
    for registration in registrations {
        (registration.register)(&mut builder)?;
    }
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use std::any::type_name;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{AutoRegistration, collect};
    use crate::ContextError;

    static CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);

    struct AValue;
    impl Default for AValue {
        fn default() -> Self {
            CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }

    struct ZDuplicate;

    #[test]
    fn collecting_duplicate_auto_registrations_never_runs_initializers() {
        let registrations = [
            AutoRegistration {
                type_name: type_name::<AValue>,
                register: |builder| builder.register::<AValue>().map(|_| ()),
            },
            AutoRegistration {
                type_name: type_name::<ZDuplicate>,
                register: |builder| {
                    builder
                        .register_with(|| {
                            CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst);
                            Ok::<_, std::io::Error>(ZDuplicate)
                        })
                        .map(|_| ())
                },
            },
            AutoRegistration {
                type_name: type_name::<ZDuplicate>,
                register: |builder| {
                    builder
                        .register_with(|| {
                            CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst);
                            Err::<ZDuplicate, _>(std::io::Error::other("must not run"))
                        })
                        .map(|_| ())
                },
            },
        ];

        for _ in 0..2 {
            assert!(matches!(
                collect(&registrations),
                Err(ContextError::DuplicateRegistration { type_name: name })
                    if name == type_name::<ZDuplicate>()
            ));
            assert_eq!(CONSTRUCTIONS.load(Ordering::SeqCst), 0);
        }
    }
}
