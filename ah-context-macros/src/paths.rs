use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::quote;

pub(crate) fn resolve() -> syn::Result<TokenStream> {
    match crate_name("ah-context").map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("failed to locate `ah-context`: {error}"),
        )
    })? {
        FoundCrate::Itself => Ok(quote!(::ah_context)),
        FoundCrate::Name(name) => {
            let ident = syn::Ident::new(&name, Span::call_site());
            Ok(quote!(::#ident))
        }
    }
}
