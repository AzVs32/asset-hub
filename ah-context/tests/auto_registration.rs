use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use ah_context::{ContextError, context};

static CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);

#[context()]
struct DriverCatalog {
    entries: AtomicUsize,
}

impl Default for DriverCatalog {
    fn default() -> Self {
        CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst);
        Self {
            entries: AtomicUsize::new(0),
        }
    }
}

#[context(init = build_settings)]
struct Settings {
    label: String,
}

fn build_settings() -> Result<Settings, std::io::Error> {
    Ok(Settings {
        label: "configured".into(),
    })
}

struct Unregistered;

mod vfs_core {
    pub struct DriverKinds {
        pub kinds: Vec<&'static str>,
    }

    #[derive(Default)]
    pub struct Catalog<T>(pub T);
}

ah_context::register!(vfs_core::DriverKinds, init = create);
ah_context::register!(vfs_core::Catalog<u32>);

fn create() -> Result<vfs_core::DriverKinds, std::io::Error> {
    Ok(vfs_core::DriverKinds {
        kinds: vec!["local", "memory"],
    })
}

mod first {
    #[ah_context::context()]
    #[derive(Default)]
    pub struct SameName;
}

mod second {
    #[ah_context::context()]
    #[derive(Default)]
    pub struct SameName;
}

#[context()]
#[cfg(any())]
struct DisabledWithoutDefault;

#[context()]
#[cfg_attr(all(), cfg(any()), derive(Default))]
struct DisabledByCfgAttr;

#[context()]
#[cfg_attr(all(), cfg_attr(all(), cfg(any()), derive(Debug)))]
struct DisabledByNestedCfgAttr;

#[test]
fn startup_constructs_once_and_all_access_shares_the_published_values() {
    assert!(matches!(
        ah_context::get::<DriverCatalog>(),
        Err(ContextError::NotInitialized)
    ));
    let barrier = Arc::new(Barrier::new(24));
    let threads: Vec<_> = (0..24)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let context = ah_context::load().unwrap();
                let catalog = ah_context::get::<DriverCatalog>().unwrap();
                assert!(Arc::ptr_eq(
                    &catalog,
                    &context.get::<DriverCatalog>().unwrap()
                ));
                catalog.entries.fetch_add(1, Ordering::SeqCst);
                catalog
            })
        })
        .collect();
    let instances: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(CONSTRUCTIONS.load(Ordering::SeqCst), 1);
    assert_eq!(instances[0].entries.load(Ordering::SeqCst), 24);
    for instance in &instances {
        assert!(Arc::ptr_eq(instance, &instances[0]));
    }
    assert!(std::ptr::eq(
        ah_context::load().unwrap(),
        ah_context::load().unwrap()
    ));
    assert_eq!(ah_context::get::<Settings>().unwrap().label, "configured");
    assert!(ah_context::get::<first::SameName>().is_ok());
    assert!(ah_context::get::<second::SameName>().is_ok());
    assert_eq!(
        ah_context::get::<vfs_core::DriverKinds>().unwrap().kinds,
        ["local", "memory"]
    );
    assert_eq!(ah_context::get::<vfs_core::Catalog<u32>>().unwrap().0, 0);
    assert!(matches!(
        ah_context::get::<Unregistered>(),
        Err(ContextError::NotRegistered { .. })
    ));
}
