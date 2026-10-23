use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Token, Visibility, parse_quote};

#[derive(Default)]
struct Capabilities {
    generate: bool,
    display: bool,
    parse: bool,
    deref: bool,
    as_ref: bool,
}

pub(crate) fn derive(input: DeriveInput) -> syn::Result<TokenStream> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "Id requires a tuple struct with one private field",
        ));
    };
    let Fields::Unnamed(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input,
            "Id requires a tuple struct with one private field",
        ));
    };
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new_spanned(
            fields,
            "Id requires exactly one field",
        ));
    }
    let field = fields
        .unnamed
        .first()
        .ok_or_else(|| syn::Error::new_spanned(fields, "Id requires exactly one field"))?;
    field.modifiers.require_empty()?;
    if !matches!(field.vis, Visibility::Inherited) {
        return Err(syn::Error::new_spanned(
            &field.vis,
            "the ID field must be private to protect validation",
        ));
    }
    if let Some(attribute) = field
        .attrs
        .iter()
        .find(|attribute| attribute.path().is_ident("id"))
    {
        return Err(syn::Error::new_spanned(
            attribute,
            "place #[id(...)] on the struct, not the field",
        ));
    }

    let mut capabilities = Capabilities::default();
    for attribute in input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("id"))
    {
        attribute.parse_nested_meta(|meta| {
            let flag = if meta.path.is_ident("generate") {
                &mut capabilities.generate
            } else if meta.path.is_ident("display") {
                &mut capabilities.display
            } else if meta.path.is_ident("parse") {
                &mut capabilities.parse
            } else if meta.path.is_ident("as_ref") {
                &mut capabilities.as_ref
            } else if meta.path.is_ident("deref") {
                &mut capabilities.deref
            } else {
                return Err(meta.error(
                    "unknown Id option; expected `generate`, `display`, `parse`, `as_ref` or `deref`",
                ));
            };
            if *flag {
                return Err(meta.error("duplicate Id capability"));
            }
            if meta.input.peek(Token![=]) || meta.input.peek(syn::token::Paren) {
                return Err(meta.error("Id capabilities are bare flags, for example #[id(deref)]"));
            }
            *flag = true;
            Ok(())
        })?;
    }
    if !capabilities.generate
        && !capabilities.display
        && !capabilities.parse
        && !capabilities.as_ref
        && !capabilities.deref
    {
        return Err(syn::Error::new(
            input.ident.span(),
            "Id requires at least one capability: generate, display, parse, as_ref or deref",
        ));
    }
    let name = &input.ident;
    let value = &field.ty;
    let (read_impl_generics, read_type_generics, read_where_clause) =
        input.generics.split_for_impl();
    let mut output = TokenStream::new();
    if capabilities.deref {
        output.extend(quote! {
            impl #read_impl_generics ::core::ops::Deref for #name #read_type_generics #read_where_clause {
                type Target = #value;

                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }
        });
    }
    if capabilities.as_ref {
        output.extend(quote! {
            impl #read_impl_generics ::core::convert::AsRef<#value> for #name #read_type_generics #read_where_clause {
                fn as_ref(&self) -> &#value {
                    &self.0
                }
            }
        });
    }
    if capabilities.generate {
        let runtime = runtime_path(input.ident.span())?;
        let mut generics = input.generics.clone();
        generics.make_where_clause().predicates.push(parse_quote!(
            #value: #runtime::Generate
        ));
        let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
        output.extend(quote! {
            // Identity generation is explicit; there is no default identity.
            #[allow(clippy::new_without_default)]
            impl #impl_generics #name #type_generics #where_clause {
                /// Generate a fresh identity using the wrapped value type.
                pub fn new() -> Self {
                    <Self as #runtime::Generate>::generate()
                }
            }

            impl #impl_generics #runtime::Generate for #name #type_generics #where_clause {
                fn generate() -> Self {
                    Self(<#value as #runtime::Generate>::generate())
                }
            }
        });
    }

    if capabilities.display {
        let mut generics = input.generics.clone();
        generics.make_where_clause().predicates.push(parse_quote!(
            #value: ::core::fmt::Display
        ));
        let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
        output.extend(quote! {
            impl #impl_generics ::core::fmt::Display for #name #type_generics #where_clause {
                fn fmt(&self, __ah_id_f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    ::core::fmt::Display::fmt(&self.0, __ah_id_f)
                }
            }
        });
    }

    if capabilities.parse {
        let mut generics = input.generics.clone();
        generics.make_where_clause().predicates.push(parse_quote!(
            #value: ::core::str::FromStr
        ));
        let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
        output.extend(quote! {
            impl #impl_generics ::core::str::FromStr for #name #type_generics #where_clause {
                type Err = <#value as ::core::str::FromStr>::Err;

                fn from_str(__ah_id_value: &str) -> ::core::result::Result<Self, Self::Err> {
                    <#value as ::core::str::FromStr>::from_str(__ah_id_value).map(Self)
                }
            }

            impl #impl_generics ::core::convert::TryFrom<&str> for #name #type_generics #where_clause {
                type Error = <#value as ::core::str::FromStr>::Err;

                fn try_from(__ah_id_value: &str) -> ::core::result::Result<Self, Self::Error> {
                    __ah_id_value.parse()
                }
            }

            impl #impl_generics ::core::convert::TryFrom<::std::string::String> for #name #type_generics #where_clause {
                type Error = <#value as ::core::str::FromStr>::Err;

                fn try_from(__ah_id_value: ::std::string::String) -> ::core::result::Result<Self, Self::Error> {
                    __ah_id_value.parse()
                }
            }
        });
    }

    Ok(output)
}

fn runtime_path(span: Span) -> syn::Result<TokenStream> {
    match crate_name("ah-id").map_err(|error| syn::Error::new(span, error.to_string()))? {
        // Also works in integration tests and examples within the ah-id package.
        FoundCrate::Itself => Ok(quote!(::ah_id)),
        FoundCrate::Name(name) => {
            let name = syn::Ident::new(&name, span);
            Ok(quote!(::#name))
        }
    }
}
