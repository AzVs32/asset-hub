use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemStruct;

use crate::args::{ConfigArgs, RegisterArgs};
use crate::auto::registration;
use crate::cfg;
use crate::paths::ConfigPaths;
use crate::serde::ensure_derives;

pub(crate) fn config(
    args: impl Into<TokenStream>,
    input: impl Into<TokenStream>,
) -> syn::Result<TokenStream> {
    expand(
        ConfigArgs::parse(args.into())?,
        syn::parse2(input.into())?,
        ConfigPaths::resolve()?,
    )
}

fn expand(args: ConfigArgs, mut item: ItemStruct, paths: ConfigPaths) -> syn::Result<TokenStream> {
    let ConfigArgs {
        key,
        validate,
        auto,
    } = args;
    let cfg = cfg::attributes(&item.attrs)?;
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
    let auto_registration = (auto && item.generics.params.is_empty()).then(|| {
        let registration = registration(&syn::parse_quote!(#ident), crate_path);
        quote! {
            #(#cfg)*
            #registration
        }
    });

    Ok(quote! {
        #item

        #(#cfg)*
        impl #impl_generics #crate_path::ConfigSection for #ident #ty_generics #where_clause {
            const KEY: &'static str = #key;
            #validate_impl
        }

        #auto_registration
    })
}

pub(crate) fn register(input: TokenStream) -> syn::Result<TokenStream> {
    let RegisterArgs { ty } = syn::parse2(input)?;
    let paths = ConfigPaths::resolve()?;
    Ok(registration(&ty, &paths.crate_path))
}

#[cfg(test)]
mod tests {
    use proc_macro2::{Span, TokenStream};
    use quote::quote;
    use syn::{Item, LitStr};

    use super::expand;
    use crate::args::ConfigArgs;
    use crate::paths::ConfigPaths;

    fn expand_section(args: TokenStream, input: TokenStream) -> syn::File {
        let paths = ConfigPaths {
            crate_path: quote!(::asset_config),
            serde_path: LitStr::new("asset_config::__private::serde", Span::call_site()),
        };
        syn::parse2(
            expand(
                ConfigArgs::parse(args).unwrap(),
                syn::parse2(input).unwrap(),
                paths,
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn conditional_compilation_gates_impl_and_registration_without_copying_derives() {
        let expanded = expand_section(
            quote!(key = "conditional"),
            quote! {
                #[cfg(feature = "enabled")]
                #[cfg_attr(all(), cfg(feature = "inner"), derive(Debug))]
                #[cfg_attr(all(), cfg_attr(all(), cfg(feature = "nested"), derive(Clone)))]
                #[cfg_attr(all(), derive(PartialEq))]
                #[derive(Default)]
                struct Conditional { value: bool }
            },
        );
        let expected = quote! {
            #[cfg(feature = "enabled")]
            #[cfg_attr(all(), cfg(feature = "inner"))]
            #[cfg_attr(all(), cfg_attr(all(), cfg(feature = "nested")))]
        }
        .to_string();

        assert_eq!(expanded.items.len(), 3);
        assert!(matches!(expanded.items[0], Item::Struct(_)));
        let Item::Impl(implementation) = &expanded.items[1] else {
            panic!("expected a ConfigSection implementation");
        };
        let Item::Const(registration) = &expanded.items[2] else {
            panic!("expected an automatic registration");
        };
        for attrs in [&implementation.attrs, &registration.attrs] {
            assert_eq!(quote!(#(#attrs)*).to_string(), expected);
        }
    }

    #[test]
    fn generic_definitions_and_manual_sections_gate_only_the_impl() {
        for (args, input) in [
            (
                quote!(key = "generic"),
                quote!(
                    struct Generic<T> {
                        value: T,
                    }
                ),
            ),
            (
                quote!(key = "generic", auto = true),
                quote!(
                    struct Generic<T> {
                        value: T,
                    }
                ),
            ),
            (
                quote!(key = "manual", auto = false),
                quote!(
                    struct Manual {
                        value: bool,
                    }
                ),
            ),
        ] {
            let expanded = expand_section(
                args,
                quote! {
                    #[cfg(any())]
                    #[derive(Default)]
                    #input
                },
            );

            assert_eq!(expanded.items.len(), 2);
            let Item::Impl(implementation) = &expanded.items[1] else {
                panic!("expected a ConfigSection implementation");
            };
            let attrs = &implementation.attrs;
            assert_eq!(
                quote!(#(#attrs)*).to_string(),
                quote!(#[cfg(any())]).to_string()
            );
        }
    }
}
