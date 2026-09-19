use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::LitStr;

pub(crate) struct ConfigPaths {
    pub(crate) crate_path: TokenStream,
    pub(crate) serde_path: LitStr,
}

impl ConfigPaths {
    pub(crate) fn resolve() -> syn::Result<Self> {
        let found = crate_name("asset-config").map_err(|error| {
            syn::Error::new(
                Span::call_site(),
                format!("failed to locate `asset-config`: {error}"),
            )
        })?;

        let (crate_path, serde_path) = match found {
            FoundCrate::Itself => (quote!(crate), "crate::__private::serde".to_owned()),
            FoundCrate::Name(name) => {
                let ident = syn::Ident::new(&name, Span::call_site());
                (quote!(::#ident), format!("{name}::__private::serde"))
            }
        };

        Ok(Self {
            crate_path,
            serde_path: LitStr::new(&serde_path, Span::call_site()),
        })
    }
}
