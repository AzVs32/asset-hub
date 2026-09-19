use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, ItemStruct, LitStr, Meta, Path, Token, punctuated::Punctuated};

pub(crate) fn ensure_derives(
    item: &mut ItemStruct,
    crate_path: &TokenStream,
    serde_path: &LitStr,
) -> syn::Result<()> {
    let mut derives = Vec::new();
    if !has_derive(&item.attrs, "Serialize")? {
        derives.push(quote!(#crate_path::__private::serde::Serialize));
    }
    if !has_derive(&item.attrs, "Deserialize")? {
        derives.push(quote!(#crate_path::__private::serde::Deserialize));
    }

    if derives.is_empty() {
        return Ok(());
    }

    item.attrs.push(syn::parse_quote!(#[derive(#(#derives),*)]));
    if !has_serde_crate_override(&item.attrs)? {
        item.attrs
            .push(syn::parse_quote!(#[serde(crate = #serde_path)]));
    }
    Ok(())
}

fn has_derive(attrs: &[Attribute], name: &str) -> syn::Result<bool> {
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("derive")) {
        let paths = attr.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)?;
        if paths
            .iter()
            .filter_map(|path| path.segments.last())
            .any(|segment| segment.ident == name)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_serde_crate_override(attrs: &[Attribute]) -> syn::Result<bool> {
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let meta = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        if meta.iter().any(|entry| entry.path().is_ident("crate")) {
            return Ok(true);
        }
    }
    Ok(false)
}
