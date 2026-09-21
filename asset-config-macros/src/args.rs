use proc_macro2::{Span, TokenStream};
use syn::{LitBool, LitStr, Path, parse::Parser};

pub(crate) struct ConfigArgs {
    pub(crate) key: LitStr,
    pub(crate) validate: Option<Path>,
    pub(crate) auto: bool,
}

impl ConfigArgs {
    pub(crate) fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut key = None;
        let mut validate = None;
        let mut auto: Option<LitBool> = None;

        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("key") {
                if key.is_some() {
                    return Err(meta.error("duplicate `key`"));
                }
                key = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("validate") {
                if validate.is_some() {
                    return Err(meta.error("duplicate `validate`"));
                }
                validate = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("auto") {
                if auto.is_some() {
                    return Err(meta.error("duplicate `auto`"));
                }
                auto = Some(meta.value()?.parse()?);
                return Ok(());
            }
            Err(meta.error("unsupported asset_config option"))
        });

        parser.parse2(args)?;
        let key: LitStr =
            key.ok_or_else(|| syn::Error::new(Span::call_site(), "missing `key = \"...\"`"))?;

        Ok(Self {
            key,
            validate,
            auto: auto.is_none_or(|v| v.value),
        })
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::ConfigArgs;

    #[test]
    fn parses_defaults_and_explicit_options() {
        let default = ConfigArgs::parse(quote!(key = "http")).unwrap();
        assert_eq!(default.key.value(), "http");
        assert!(default.auto);
        assert!(default.validate.is_none());

        let explicit =
            ConfigArgs::parse(quote!(key = "cache", validate = check_cache, auto = false)).unwrap();
        assert_eq!(explicit.key.value(), "cache");
        assert!(!explicit.auto);
        assert_eq!(
            explicit.validate.unwrap().get_ident().unwrap().to_string(),
            "check_cache"
        );

        assert!(
            ConfigArgs::parse(quote!(key = "database", auto = true))
                .unwrap()
                .auto
        );

        for key in [
            "server.http_api",
            "server.http-api",
            "Server.123",
            "server..http",
            "  ",
        ] {
            let args = quote!(key = #key);
            assert_eq!(ConfigArgs::parse(args).unwrap().key.value(), key);
        }
    }

    #[test]
    fn rejects_invalid_options() {
        for (args, message) in [
            (quote!(), "missing `key = \"...\"`"),
            (quote!(key = "a", key = "b"), "duplicate `key`"),
            (
                quote!(key = "a", auto = true, auto = false),
                "duplicate `auto`",
            ),
            (
                quote!(key = "a", unknown = true),
                "unsupported asset_config option",
            ),
        ] {
            let error = ConfigArgs::parse(args).err().unwrap();
            assert!(error.to_string().contains(message), "{error}");
        }

        assert!(ConfigArgs::parse(quote!(key = "a", auto = "yes")).is_err());
    }
}
