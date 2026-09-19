use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::{LitStr, Path, parse::Parser};

pub(crate) struct ConfigArgs {
    pub(crate) key: LitStr,
    pub(crate) validate: Option<Path>,
}

impl ConfigArgs {
    pub(crate) fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut key = None;
        let mut validate = None;

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
            Err(meta.error("unsupported asset_config option"))
        });

        parser.parse(args)?;
        let key: LitStr =
            key.ok_or_else(|| syn::Error::new(Span::call_site(), "missing `key = \"...\"`"))?;
        if key.value().trim().is_empty() {
            return Err(syn::Error::new(key.span(), "`key` cannot be empty"));
        }

        Ok(Self { key, validate })
    }
}
