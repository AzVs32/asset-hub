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
    let register = match init {
        Some(init) => quote!(builder.register_with::<#ty, _>(#init)?;),
        None => quote!(builder.register::<#ty>()?;),
    };

    Ok(quote! {
        const _: () = {
            fn register(builder: &mut #crate_path::ContextBuilder)
                -> Result<(), #crate_path::ContextError>
            {
                #register
                Ok(())
            }

            #[#crate_path::__private::linkme::distributed_slice(
                #crate_path::__private::AUTO_REGISTRATIONS
            )]
            #[linkme(crate = #crate_path::__private::linkme)]
            static REGISTRATION: #crate_path::__private::AutoRegistration =
                #crate_path::__private::AutoRegistration {
                    type_name: ::core::any::type_name::<#ty>,
                    register,
                };
        };
    })
}
