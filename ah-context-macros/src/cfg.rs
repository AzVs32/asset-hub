use syn::{Attribute, Meta, Token, punctuated::Punctuated};

/// Propagates only conditional-compilation attributes to generated items.
/// Other cfg_attr contents (such as derive) still belong to the original struct.
pub(crate) fn attributes(attrs: &[Attribute]) -> syn::Result<Vec<Attribute>> {
    attrs
        .iter()
        .filter_map(|attr| match project(&attr.meta) {
            Ok(Some(meta)) => Some(Ok(syn::parse_quote!(#[#meta]))),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn project(meta: &Meta) -> syn::Result<Option<Meta>> {
    if meta.path().is_ident("cfg") {
        return Ok(Some(meta.clone()));
    }
    if !meta.path().is_ident("cfg_attr") {
        return Ok(None);
    }
    let Meta::List(list) = meta else {
        return Ok(None);
    };
    let args = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    let mut args = args.into_iter();
    let Some(condition) = args.next() else {
        return Ok(None);
    };
    let mut projected = Vec::new();
    for arg in args {
        if let Some(meta) = project(&arg)? {
            projected.push(meta);
        }
    }
    if projected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            syn::parse_quote!(cfg_attr(#condition, #(#projected),*)),
        ))
    }
}
