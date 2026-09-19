use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::ItemStruct;

use crate::args::ConfigArgs;
use crate::paths::ConfigPaths;
use crate::serde::ensure_derives;

pub(crate) fn config(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream2> {
    let ConfigArgs { key, validate } = ConfigArgs::parse(args)?;
    let mut item: ItemStruct = syn::parse(input)?;
    let paths = ConfigPaths::resolve()?;
    let crate_path = &paths.crate_path;
    ensure_derives(&mut item, crate_path, &paths.serde_path)?;

    let ident = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    let validate_impl = validate.map(|validate_fn| {
        quote! {
            fn validate(&self) -> Result<(), #crate_path::ConfigError> {
                #validate_fn(self)
                    .map_err(|error| #crate_path::ConfigError::validate(Self::KEY, error))
            }
        }
    });

    Ok(quote! {
        #item

        impl #impl_generics #crate_path::ConfigSection for #ident #ty_generics #where_clause {
            const KEY: &'static str = #key;
            #validate_impl
        }
    })
}
