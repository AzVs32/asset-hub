use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::ItemStruct;

use crate::args::ConfigArgs;
use crate::auto::registration;
use crate::paths::ConfigPaths;
use crate::serde::ensure_derives;

pub(crate) fn config(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream2> {
    let ConfigArgs {
        key,
        validate,
        auto,
    } = ConfigArgs::parse(args.into())?;
    let mut item: ItemStruct = syn::parse(input)?;
    if auto && !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "generic config types cannot be auto-registered; use `auto = false` and register a concrete type manually",
        ));
    }
    let paths = ConfigPaths::resolve()?;
    let crate_path = &paths.crate_path;
    ensure_derives(&mut item, crate_path, &paths.serde_path)?;

    let ident = &item.ident;
    let mut generics = item.generics.clone();
    if !generics.params.is_empty() {
        let (_, ty_generics, _) = item.generics.split_for_impl();
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(
                #ident #ty_generics: Default
                    + #crate_path::__private::serde::Serialize
                    + #crate_path::__private::serde::de::DeserializeOwned
                    + Send + Sync + 'static
            ));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let validate_impl = validate.map(|validate_fn| {
        quote! {
            fn validate(&self) -> Result<(), #crate_path::ConfigError> {
                #validate_fn(self)
                    .map_err(|error| #crate_path::ConfigError::validate(Self::KEY, error))
            }
        }
    });
    let auto_registration = auto.then(|| registration(ident, &key, crate_path));

    Ok(quote! {
        #item

        impl #impl_generics #crate_path::ConfigSection for #ident #ty_generics #where_clause {
            const KEY: &'static str = #key;
            #validate_impl
        }

        #auto_registration
    })
}
