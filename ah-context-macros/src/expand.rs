use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemStruct, Path, Type};

use crate::{
    args::{ContextArgs, RegisterArgs},
    cfg, paths,
};

pub(crate) fn context(
    args: impl Into<TokenStream>,
    input: impl Into<TokenStream>,
) -> syn::Result<TokenStream> {
    let ContextArgs { init } = ContextArgs::parse(args.into())?;
    let item: ItemStruct = syn::parse2(input.into())?;
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "context requires a concrete type; wrap the generic type in a non-generic struct",
        ));
    }

    let ident = &item.ident;
    let cfg = cfg::attributes(&item.attrs)?;
    let registration = registration(&syn::parse_quote!(#ident), init)?;

    Ok(quote! {
        #item

        #(#cfg)*
        #registration
    })
}

pub(crate) fn register(input: TokenStream) -> syn::Result<TokenStream> {
    let RegisterArgs { ty, init } = syn::parse2(input)?;
    registration(&ty, init)
}

fn registration(ty: &Type, init: Option<Path>) -> syn::Result<TokenStream> {
    let crate_path = paths::resolve()?;
    let create = match init {
        Some(init) => quote! {
            #init().map_err(|source| #crate_path::ContextError::Initialization {
                type_name: ::core::any::type_name::<#ty>(),
                source: ::std::boxed::Box::new(source),
            })?
        },
        None => quote!(<#ty as ::core::default::Default>::default()),
    };

    Ok(quote! {
        const _: () = {
            #[#crate_path::__private::linkme::distributed_slice(
                #crate_path::__private::REGISTRATIONS
            )]
            #[linkme(crate = #crate_path::__private::linkme)]
            static REGISTRATION: #crate_path::__private::Registration =
                #crate_path::__private::Registration {
                    type_id: ::core::any::TypeId::of::<#ty>,
                    type_name: ::core::any::type_name::<#ty>,
                    create: || {
                        let value: #ty = #create;
                        ::core::result::Result::Ok(::std::sync::Arc::new(value))
                    },
                };
        };
    })
}
