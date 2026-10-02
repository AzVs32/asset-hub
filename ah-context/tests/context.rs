use std::any::type_name;
use std::convert::Infallible;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use ah_context::{ContextBuilder, ContextError, context};

#[asset_config::config(key = "http")]
struct HttpConfig {
    port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { port: 8080 }
    }
}

struct HttpClient {
    config: Arc<HttpConfig>,
}

static CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);
static STARTUP_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

#[context(init = initialize_gate)]
struct AStartupGate(u32);

fn initialize_gate() -> Result<AStartupGate, &'static str> {
    if STARTUP_ATTEMPTS.fetch_add(1, Ordering::SeqCst) == 0 {
        Err("not ready")
    } else {
        Ok(AStartupGate(42))
    }
}

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
fn global_loading_retries_failed_builds_then_publishes_a_configured_builder() {
    assert!(matches!(
        ah_context::get::<DriverCatalog>(),
        Err(ContextError::NotInitialized)
    ));
    let mut failed = ContextBuilder::new();
    failed
        .register_with(|| Err::<HttpClient, _>("client not ready"))
        .unwrap();
    let error = ah_context::load_with(failed).err().unwrap();
    assert!(matches!(&error, ContextError::Initialization { .. }));
    assert_eq!(error.source().unwrap().to_string(), "client not ready");
    let Err(error) = ah_context::load() else {
        panic!("the first initialization must fail");
    };
    assert!(matches!(
        &error,
        ContextError::Initialization { type_name: name, .. }
            if *name == type_name::<AStartupGate>()
    ));
    assert_eq!(error.source().unwrap().to_string(), "not ready");
    assert!(matches!(
        ah_context::get::<DriverCatalog>(),
        Err(ContextError::NotInitialized)
    ));

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ah-context-http-{}-{timestamp}.toml",
        std::process::id()
    ));
    std::fs::write(&path, "[http]\nport = 9090\n").unwrap();
    let configs = asset_config::load(&path);
    std::fs::remove_file(&path).unwrap();
    let http = configs.unwrap().get::<HttpConfig>().unwrap();
    let captured = Arc::clone(&http);
    let mut builder = ContextBuilder::from_auto().unwrap();
    builder
        .register_with(move || Ok::<_, Infallible>(HttpClient { config: captured }))
        .unwrap();
    let published = ah_context::load_with(builder).unwrap();
    let client = ah_context::get::<HttpClient>().unwrap();
    assert_eq!(client.config.port, 9090);
    assert!(Arc::ptr_eq(&http, &client.config));
    assert!(std::ptr::eq(published, ah_context::load().unwrap()));

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
    assert_eq!(STARTUP_ATTEMPTS.load(Ordering::SeqCst), 2);
    assert_eq!(ah_context::get::<AStartupGate>().unwrap().0, 42);
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

    let rejected_calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&rejected_calls);
    let mut rejected = ContextBuilder::new();
    rejected
        .register_with(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, Infallible>(Label("ignored".into()))
        })
        .unwrap();
    assert!(matches!(
        ah_context::load_with(rejected),
        Err(ContextError::AlreadyInitialized)
    ));
    assert_eq!(rejected_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        ah_context::get::<Label>(),
        Err(ContextError::NotRegistered { .. })
    ));
}

#[derive(Default)]
struct Counter(AtomicUsize);

struct Label(String);

#[test]
fn independent_builders_capture_dependencies_and_share_values_only_within_each_context() {
    let constructions = Arc::new(AtomicUsize::new(0));
    let build = |label: String| {
        let constructions = Arc::clone(&constructions);
        let mut builder = ContextBuilder::new();
        builder.register::<Counter>().unwrap();
        builder
            .register_with(move || {
                constructions.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(Label(label))
            })
            .unwrap();
        builder
    };
    let first = build("first".into());
    let second = build("second".into());
    assert_eq!(constructions.load(Ordering::SeqCst), 0);

    let first = first.build().unwrap();
    let second = second.build().unwrap();
    assert_eq!(constructions.load(Ordering::SeqCst), 2);
    assert_eq!(first.get::<Label>().unwrap().0, "first");
    assert_eq!(second.get::<Label>().unwrap().0, "second");
    let counter = first.get::<Counter>().unwrap();
    assert!(Arc::ptr_eq(&counter, &first.get::<Counter>().unwrap()));
    assert!(!Arc::ptr_eq(&counter, &second.get::<Counter>().unwrap()));
    counter.0.fetch_add(1, Ordering::Relaxed);
    assert_eq!(first.get::<Counter>().unwrap().0.load(Ordering::Relaxed), 1);
    assert_eq!(
        second.get::<Counter>().unwrap().0.load(Ordering::Relaxed),
        0
    );
}

#[test]
fn duplicate_registration_is_immediate_and_preserves_the_original_initializer() {
    type Alias = Counter;
    let constructions = Arc::new(AtomicUsize::new(0));
    let rejected_constructions = Arc::new(AtomicUsize::new(0));
    let mut builder = ContextBuilder::new();
    let count = Arc::clone(&constructions);
    builder
        .register_with(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(Counter::default())
        })
        .unwrap();

    assert!(matches!(
        builder.register::<Alias>(),
        Err(ContextError::DuplicateRegistration { type_name: name })
            if name == type_name::<Counter>()
    ));
    let count = Arc::clone(&rejected_constructions);
    assert!(matches!(
        builder.register_with(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(Counter::default())
        }),
        Err(ContextError::DuplicateRegistration { .. })
    ));
    assert_eq!(constructions.load(Ordering::SeqCst), 0);

    let context = builder.build().unwrap();
    assert!(context.get::<Counter>().is_ok());
    assert_eq!(constructions.load(Ordering::SeqCst), 1);
    assert_eq!(rejected_constructions.load(Ordering::SeqCst), 0);
}

#[test]
fn failed_build_preserves_the_source_drops_built_values_and_skips_later_initializers() {
    struct Tracked(Arc<AtomicUsize>);
    impl Drop for Tracked {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let drops = Arc::new(AtomicUsize::new(0));
    let later_constructions = Arc::new(AtomicUsize::new(0));
    let mut builder = ContextBuilder::new();
    let count = Arc::clone(&drops);
    builder
        .register_with(move || Ok::<_, std::io::Error>(Tracked(count)))
        .unwrap();
    builder
        .register_with(|| Err::<Label, _>(std::io::Error::other("not ready")))
        .unwrap();
    let count = Arc::clone(&later_constructions);
    builder
        .register_with(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(true)
        })
        .unwrap();

    let Err(error) = builder.build() else {
        panic!("the failing initializer must prevent building a context");
    };
    assert!(matches!(
        &error,
        ContextError::Initialization { type_name: name, .. }
            if *name == type_name::<Label>()
    ));
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .to_string(),
        "not ready"
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(later_constructions.load(Ordering::SeqCst), 0);

    let mut retry = ContextBuilder::new();
    retry
        .register_with(|| Ok::<_, std::io::Error>(Label("ready".into())))
        .unwrap();
    assert_eq!(retry.build().unwrap().get::<Label>().unwrap().0, "ready");
}

#[test]
fn empty_contexts_and_concrete_generics_can_be_built_without_automatic_registrations() {
    let empty = ContextBuilder::default().build().unwrap();
    assert!(matches!(
        empty.get::<DriverCatalog>(),
        Err(ContextError::NotRegistered { type_name: name })
            if name == type_name::<DriverCatalog>()
    ));

    let mut builder = ContextBuilder::new();
    builder.register::<vfs_core::Catalog<u64>>().unwrap();
    let context = builder.build().unwrap();
    assert_eq!(context.get::<vfs_core::Catalog<u64>>().unwrap().0, 0);
    assert!(matches!(
        context.get::<vfs_core::Catalog<u32>>(),
        Err(ContextError::NotRegistered { .. })
    ));
}

#[test]
fn initializer_errors_accept_strings_and_preserve_typed_or_boxed_error_chains() {
    type BoxError = Box<dyn Error + Send + Sync>;

    #[derive(Debug, thiserror::Error)]
    #[error("user initialization failed")]
    struct UserError {
        #[source]
        cause: std::io::Error,
    }

    fn initialization_error<E: Into<BoxError> + Send + 'static>(source: E) -> ContextError {
        let mut builder = ContextBuilder::new();
        builder
            .register_with(move || Err::<Label, _>(source))
            .unwrap();
        builder.build().err().unwrap()
    }

    for (error, message) in [
        (initialization_error("string slice"), "string slice"),
        (
            initialization_error(String::from("owned message")),
            "owned message",
        ),
    ] {
        assert!(matches!(
            &error,
            ContextError::Initialization { type_name: name, .. }
                if *name == type_name::<Label>()
        ));
        assert_eq!(
            error.to_string(),
            format!(
                "failed to initialize context type `{}`",
                type_name::<Label>()
            )
        );
        assert_eq!(error.source().unwrap().to_string(), message);
    }

    let boxed: BoxError = Box::new(UserError {
        cause: std::io::Error::other("boxed cause"),
    });
    for (error, cause) in [
        (
            initialization_error(UserError {
                cause: std::io::Error::other("typed cause"),
            }),
            "typed cause",
        ),
        (initialization_error(boxed), "boxed cause"),
    ] {
        assert!(matches!(
            &error,
            ContextError::Initialization { type_name: name, .. }
                if *name == type_name::<Label>()
        ));
        let source = error.source().unwrap();
        assert!(source.downcast_ref::<UserError>().is_some());
        assert_eq!(source.to_string(), "user initialization failed");
        let cause_error = source.source().unwrap();
        assert!(cause_error.downcast_ref::<std::io::Error>().is_some());
        assert_eq!(cause_error.to_string(), cause);
        let report = format!("{error}: {source}: {cause_error}");
        assert_eq!(report.matches("user initialization failed").count(), 1);
        assert_eq!(report.matches(cause).count(), 1);
    }
}
