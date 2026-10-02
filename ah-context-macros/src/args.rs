use proc_macro2::TokenStream;
use syn::{
    Path, Token, Type,
    parse::{Parse, ParseStream, Parser},
};

pub(crate) struct ContextArgs {
    pub(crate) init: Option<Path>,
}

pub(crate) struct RegisterArgs {
    pub(crate) ty: Type,
    pub(crate) init: Option<Path>,
}

impl Parse for RegisterArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ty = input.parse()?;
        let init = if input.is_empty() {
            None
        } else {
            input.parse::<Token![,]>()?;
            ContextArgs::parse(input.parse()?)?.init
        };
        Ok(Self { ty, init })
    }
}

impl ContextArgs {
    pub(crate) fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut init = None;
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("init") {
                if init.is_some() {
                    return Err(meta.error("duplicate `init`"));
                }
                init = Some(meta.value()?.parse()?);
                return Ok(());
            }
            Err(meta.error("unsupported ah_context option"))
        });
        parser.parse2(args)?;
        Ok(Self { init })
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::{ContextArgs, RegisterArgs};

    #[test]
    fn accepts_no_options_or_a_custom_factory() {
        let default = ContextArgs::parse(quote!()).unwrap();
        assert!(default.init.is_none());

        let explicit = ContextArgs::parse(quote!(init = module::build)).unwrap();
        assert_eq!(explicit.init.unwrap().segments.len(), 2);
    }

    #[test]
    fn rejects_duplicate_unknown_and_ill_typed_options() {
        for args in [
            quote!(init = first, init = second),
            quote!(unknown = true),
            quote!(init = "build"),
        ] {
            assert!(ContextArgs::parse(args).is_err());
        }
    }

    #[test]
    fn parses_external_and_concrete_generic_types() {
        let default: RegisterArgs = syn::parse2(quote!(vfs_core::DriverKinds)).unwrap();
        assert!(matches!(default.ty, syn::Type::Path(_)));
        assert!(default.init.is_none());

        let explicit: RegisterArgs =
            syn::parse2(quote!(vfs_core::DriverKinds, init = runtime::build,)).unwrap();
        assert_eq!(explicit.init.unwrap().segments.len(), 2);

        let generic: RegisterArgs = syn::parse2(quote!(vfs_core::Catalog<u32>,)).unwrap();
        assert!(matches!(generic.ty, syn::Type::Path(_)));
        assert!(generic.init.is_none());
    }
}
