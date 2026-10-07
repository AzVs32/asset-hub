use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::quote;

pub(crate) fn resolve() -> syn::Result<TokenStream> {
    match crate_name("asset-context").map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("failed to locate `asset-context`: {error}"),
        )
    })? {
        FoundCrate::Itself => Ok(quote!(::asset_context)),
        FoundCrate::Name(name) => {
            let ident = syn::Ident::new(&name, Span::call_site());
            Ok(quote!(::#ident))
        }
    }
}
