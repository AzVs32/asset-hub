use proc_macro2::TokenStream;
use quote::quote;
use syn::Type;

pub(crate) fn registration(ty: &Type, crate_path: &TokenStream) -> TokenStream {
    quote! {
        const _: () = {
            fn register(registry: &mut #crate_path::Registry)
                -> ::core::result::Result<(), #crate_path::ConfigError>
            {
                registry.register::<#ty>()?;
                ::core::result::Result::Ok(())
            }

            #[#crate_path::__private::linkme::distributed_slice(
                #crate_path::__private::ASSET_CONFIG_AUTO_REGISTRATIONS
            )]
            #[linkme(crate = #crate_path::__private::linkme)]
            static REGISTRATION: #crate_path::__private::AutoRegistration =
                #crate_path::__private::AutoRegistration {
                    key: <#ty as #crate_path::ConfigSection>::KEY,
                    register,
                };
        };
    }
}
