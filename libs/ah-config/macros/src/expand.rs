use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprPath, Fields, LitStr, Token, bracketed, ext::IdentExt,
    punctuated::Punctuated,
};

#[derive(Clone)]
enum DefaultValue {
    Trait,
    Expression(Expr),
}

impl DefaultValue {
    fn expression(&self) -> TokenStream {
        match self {
            Self::Trait => quote!(::core::default::Default::default()),
            Self::Expression(value) => quote!(#value),
        }
    }
}

#[derive(Default)]
struct Options {
    key: Option<LitStr>,
    parent: Option<syn::TypePath>,
    spaces: Option<Vec<LitStr>>,
    default: Option<DefaultValue>,
    validate: Option<ExprPath>,
    tag: Option<LitStr>,
    rename: Option<LitStr>,
    deny_unknown_fields: bool,
}

#[derive(Clone, Copy)]
enum Location {
    Config,
    Value,
    Field,
    Variant,
}

impl Options {
    fn parse(attributes: &[Attribute], location: Location) -> syn::Result<Self> {
        let mut options = Self::default();
        for attribute in attributes {
            if attribute.path().is_ident("serde") {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "use #[config(...)] for configuration rules instead of #[serde(...)]",
                ));
            }
            if !attribute.path().is_ident("config") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                let container = matches!(location, Location::Config | Location::Value);
                if meta.path.is_ident("default_with") {
                    return Err(meta.error("use default = factory() instead of default_with"));
                } else if meta.path.is_ident("default") {
                    if matches!(location, Location::Variant) {
                        return Err(meta.error("select the default on the enum with #[config(default = Self::Variant)]"));
                    }
                    if options.default.is_some() { return Err(meta.error("duplicate default option")); }
                    options.default = Some(if meta.input.peek(Token![=]) {
                        DefaultValue::Expression(meta.value()?.parse()?)
                    } else { DefaultValue::Trait });
                } else if meta.path.is_ident("rename") && !container {
                    set_once(&mut options.rename, meta.value()?.parse()?, &meta)?;
                } else if meta.path.is_ident("key") && matches!(location, Location::Config) {
                    set_once(&mut options.key, meta.value()?.parse()?, &meta)?;
                } else if meta.path.is_ident("parent") && matches!(location, Location::Config) {
                    set_once(&mut options.parent, meta.value()?.parse()?, &meta)?;
                } else if meta.path.is_ident("spaces") && matches!(location, Location::Config) {
                    if options.spaces.is_some() { return Err(meta.error("duplicate spaces option")); }
                    let content;
                    let value = meta.value()?;
                    bracketed!(content in value);
                    options.spaces = Some(Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?.into_iter().collect());
                } else if meta.path.is_ident("validate") && matches!(location, Location::Config) {
                    set_once(&mut options.validate, meta.value()?.parse()?, &meta)?;
                } else if meta.path.is_ident("tag") && container {
                    set_once(&mut options.tag, meta.value()?.parse()?, &meta)?;
                } else if meta.path.is_ident("rename_all") {
                    return Err(meta.error("configuration names use fixed snake_case; use rename on individual fields or variants"));
                } else if meta.path.is_ident("deny_unknown_fields") && container {
                    if options.deny_unknown_fields { return Err(meta.error("duplicate deny_unknown_fields option")); }
                    options.deny_unknown_fields = true;
                } else { return Err(meta.error("unsupported config option at this location")); }
                Ok(())
            })?;
        }
        Ok(options)
    }
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    meta: &syn::meta::ParseNestedMeta<'_>,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error("duplicate config option"));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) fn derive(input: DeriveInput, registered: bool) -> syn::Result<TokenStream> {
    let runtime = match crate_name("ah-config")
        .map_err(|error| syn::Error::new(input.ident.span(), error.to_string()))?
    {
        FoundCrate::Itself => quote!(::ah_config),
        FoundCrate::Name(name) => {
            let name = syn::Ident::new(&name, Span::call_site());
            quote!(::#name)
        }
    };
    expand(input, registered, runtime)
}

fn expand(input: DeriveInput, registered: bool, runtime: TokenStream) -> syn::Result<TokenStream> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "configuration declarations require a concrete non-generic type",
        ));
    }
    let args = Options::parse(
        &input.attrs,
        if registered {
            Location::Config
        } else {
            Location::Value
        },
    )?;
    let name = &input.ident;
    if !matches!(input.data, Data::Enum(_)) && args.tag.is_some() {
        return Err(syn::Error::new_spanned(
            name,
            "tag is supported only on enums",
        ));
    }
    if let Some(tag) = &args.tag
        && tag.value().is_empty()
    {
        return Err(syn::Error::new_spanned(tag, "tag must not be empty"));
    }
    let serde_path = LitStr::new(
        &format!("{}::__private::serde", runtime.to_string().replace(' ', "")),
        Span::call_site(),
    );
    let mut container = TokenStream::new();
    if let Some(tag) = &args.tag {
        container.extend(quote!(#[serde(tag = #tag)]));
    }
    if args.deny_unknown_fields {
        container.extend(quote!(#[serde(deny_unknown_fields)]));
    }
    let proxy = format_ident!("__AhConfigProxy");
    let mut functions = TokenStream::new();
    let default_variant = selected_variant(&input, &args)?;
    let mut borrows = false;
    let (serialize_body, deserialize_body, serialize_value, deserialize_value) = match &input.data {
        Data::Struct(data) => {
            let decoded = fields(
                &data.fields,
                name,
                "struct",
                false,
                &mut functions,
                &mut borrows,
            )?;
            let encoded = fields(
                &data.fields,
                name,
                "struct",
                true,
                &mut TokenStream::new(),
                &mut borrows,
            )?;
            let encode = construct(&data.fields, quote!(#proxy), |member| quote!(&self.#member));
            let decode = construct(&data.fields, quote!(Self), |member| quote!(value.#member));
            (
                struct_definition(&data.fields, &proxy, encoded, true),
                struct_definition(&data.fields, &proxy, decoded, false),
                encode,
                decode,
            )
        }
        Data::Enum(data) => {
            if args.tag.is_none()
                && data
                    .variants
                    .iter()
                    .any(|variant| !matches!(variant.fields, Fields::Unit))
            {
                return Err(syn::Error::new_spanned(
                    name,
                    "enums with fields require #[config(tag = \"selector\")]",
                ));
            }
            let mut encoded_variants = TokenStream::new();
            let mut decoded_variants = TokenStream::new();
            let mut encode_arms = TokenStream::new();
            let mut decode_arms = TokenStream::new();
            let mut names = std::collections::HashSet::new();
            for (variant_index, variant) in data.variants.iter().enumerate() {
                let options = Options::parse(&variant.attrs, Location::Variant)?;
                let ident = &variant.ident;
                let external = external_name(ident, options.rename);
                if !names.insert(external.value()) {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "duplicate external variant name",
                    ));
                }
                let rename = quote!(#[serde(rename = #external)]);
                let decoded = fields(
                    &variant.fields,
                    name,
                    &format!("variant_{variant_index}"),
                    false,
                    &mut functions,
                    &mut borrows,
                )?;
                let encoded = fields(
                    &variant.fields,
                    name,
                    &format!("variant_{variant_index}"),
                    true,
                    &mut TokenStream::new(),
                    &mut borrows,
                )?;
                encoded_variants.extend(quote!(#rename #ident #encoded,));
                decoded_variants.extend(quote!(#rename #ident #decoded,));
                let pattern_self = variant_binding(&variant.fields, quote!(Self::#ident));
                let pattern_proxy = variant_binding(&variant.fields, quote!(#proxy::#ident));
                encode_arms.extend(quote!(#pattern_self => #pattern_proxy,));
                decode_arms.extend(quote!(#pattern_proxy => #pattern_self,));
            }
            (
                quote!(enum #proxy<'__ah_config> { #encoded_variants }),
                quote!(enum #proxy { #decoded_variants }),
                quote!(match self { #encode_arms }),
                quote!(match value { #decode_arms }),
            )
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "configuration requires a struct or enum",
            ));
        }
    };
    // Unit-only values do not need a borrowing lifetime in the serialization proxy.
    let serialize_body = if borrows {
        serialize_body
    } else {
        remove_proxy_lifetime(serialize_body)?
    };
    let default_value = if let Some(value) = default_variant {
        Some(value)
    } else {
        match &args.default {
            Some(DefaultValue::Trait) => {
                let Data::Struct(data) = &input.data else {
                    return Err(syn::Error::new_spanned(
                        name,
                        "select an enum default with #[config(default = Self::Variant)]",
                    ));
                };
                Some(default_constructor(&data.fields, quote!(Self))?)
            }
            Some(value) => Some(value.expression()),
            None => None,
        }
    };
    let default_impl = default_value
        .map(|value| {
            quote!(impl ::core::default::Default for #name { fn default() -> Self { #value } })
        })
        .unwrap_or_default();
    let declaration = if registered {
        declaration(&input, &args, &runtime)?
    } else {
        TokenStream::new()
    };
    let merge = default_merge(&input, &runtime)?;
    Ok(quote! {
        const _: () = {
            impl #name { #functions }
            impl #runtime::__private::serde::Serialize for #name {
                fn serialize<S>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error>
                where S: #runtime::__private::serde::Serializer {
                    #[derive(#runtime::__private::serde::Serialize)]
                    #[serde(crate = #serde_path)]
                    #container
                    #serialize_body
                    #runtime::__private::serde::Serialize::serialize(&(#serialize_value), serializer)
                }
            }
            impl<'__ah_de> #runtime::__private::serde::Deserialize<'__ah_de> for #name {
                fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
                where D: #runtime::__private::serde::Deserializer<'__ah_de> {
                    #[derive(#runtime::__private::serde::Deserialize)]
                    #[serde(crate = #serde_path)]
                    #container
                    #deserialize_body
                    let value = <#proxy as #runtime::__private::serde::Deserialize>::deserialize(deserializer)?;
                    let value = #deserialize_value;
                    ::core::result::Result::Ok(value)
                }
            }
            #default_impl
            #merge
            #declaration
        };
    })
}

fn default_merge(input: &DeriveInput, runtime: &TokenStream) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let delegate = |ty: &syn::Type| {
        quote! {
            // Autoref dispatch preserves support for field types that only implement Serde.
            use #runtime::__private::PrepareDefaults as _;
            (&#runtime::__private::MergeProbe::<#ty>(::core::marker::PhantomData))
                .prepare(defaults, supplied);
        }
    };
    let body = match &input.data {
        Data::Enum(_) => quote! { *defaults = supplied.clone(); },
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let mut visits = TokenStream::new();
                for field in &fields.named {
                    let options = Options::parse(&field.attrs, Location::Field)?;
                    let Some(ident) = &field.ident else {
                        continue;
                    };
                    let key = external_name(ident, options.rename);
                    let visit = delegate(&field.ty);
                    visits.extend(quote! {
                        if let (::core::option::Option::Some(defaults), ::core::option::Option::Some(supplied)) =
                            (defaults.get_mut(#key), supplied.get(#key)) {
                            #visit
                        }
                    });
                }
                quote! {
                    if let (#runtime::__private::Value::Dict(_, defaults), #runtime::__private::Value::Dict(_, supplied)) =
                        (defaults, supplied) {
                        #visits
                    }
                }
            }
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => delegate(&fields.unnamed[0].ty),
            // Tuples serialize as arrays, which are already replaced as a whole.
            _ => TokenStream::new(),
        },
        Data::Union(_) => TokenStream::new(),
    };
    Ok(quote! {
        impl #runtime::__private::DefaultMerge for #name {
            fn prepare_defaults(defaults: &mut #runtime::__private::Value, supplied: &#runtime::__private::Value) {
                #body
            }
        }
    })
}

fn fields(
    fields: &Fields,
    owner: &syn::Ident,
    prefix: &str,
    serialize: bool,
    functions: &mut TokenStream,
    borrows: &mut bool,
) -> syn::Result<TokenStream> {
    let mut output = TokenStream::new();
    let mut names = std::collections::HashSet::new();
    for (index, field) in fields.iter().enumerate() {
        let args = Options::parse(&field.attrs, Location::Field)?;
        let ty = &field.ty;
        let ident = &field.ident;
        let rename = if let Some(ident) = ident {
            let external = external_name(ident, args.rename);
            if !names.insert(external.value()) {
                return Err(syn::Error::new_spanned(
                    ident,
                    "duplicate external field name",
                ));
            }
            Some(quote!(#[serde(rename = #external)]))
        } else {
            None
        };
        let default = if serialize {
            None
        } else {
            args.default.map(|value| {
                let function =
                    format_ident!("__ah_config_default_{}_{}", prefix.to_lowercase(), index);
                let expression = value.expression();
                functions.extend(quote!(fn #function() -> #ty { #expression }));
                let path = LitStr::new(&format!("{owner}::{function}"), Span::call_site());
                quote!(#[serde(default = #path)])
            })
        };
        let ty = if serialize {
            *borrows = true;
            quote!(&'__ah_config #ty)
        } else {
            quote!(#ty)
        };
        if matches!(fields, Fields::Named(_)) {
            output.extend(quote!(#rename #default #ident: #ty,));
        } else {
            output.extend(quote!(#rename #default #ty,));
        }
    }
    Ok(match fields {
        Fields::Named(_) => quote!({ #output }),
        Fields::Unnamed(_) => quote!(( #output )),
        Fields::Unit => TokenStream::new(),
    })
}

/// Apply the fixed naming convention unless a single name was explicitly provided.
fn external_name(ident: &syn::Ident, explicit: Option<LitStr>) -> LitStr {
    explicit.unwrap_or_else(|| {
        let mut name = String::new();
        for (index, ch) in ident.unraw().to_string().char_indices() {
            if index > 0 && ch.is_uppercase() {
                name.push('_');
            }
            name.push(ch.to_ascii_lowercase());
        }
        LitStr::new(&name, ident.span())
    })
}

fn struct_definition(
    fields: &Fields,
    proxy: &syn::Ident,
    body: TokenStream,
    borrowing: bool,
) -> TokenStream {
    let lifetime = borrowing.then(|| quote!(<'__ah_config>));
    match fields {
        Fields::Named(_) => quote!(struct #proxy #lifetime #body),
        Fields::Unnamed(_) => quote!(struct #proxy #lifetime #body;),
        Fields::Unit => quote!(struct #proxy #lifetime;),
    }
}

fn remove_proxy_lifetime(tokens: TokenStream) -> syn::Result<TokenStream> {
    let mut item: syn::Item = syn::parse2(tokens)?;
    match &mut item {
        syn::Item::Struct(item) => item.generics = syn::Generics::default(),
        syn::Item::Enum(item) => item.generics = syn::Generics::default(),
        _ => unreachable!(),
    }
    Ok(quote!(#item))
}

/// Share field bindings between match patterns and reconstructed variants.
fn variant_binding(fields: &Fields, target: TokenStream) -> TokenStream {
    let bindings = fields.iter().enumerate().map(|(index, field)| {
        let ident = field
            .ident
            .clone()
            .unwrap_or_else(|| format_ident!("__field_{index}"));
        quote!(#ident)
    });
    match fields {
        Fields::Named(_) => quote!(#target { #(#bindings),* }),
        Fields::Unnamed(_) => quote!(#target ( #(#bindings),* )),
        Fields::Unit => target,
    }
}

fn construct(
    fields: &Fields,
    target: TokenStream,
    mut value: impl FnMut(syn::Member) -> TokenStream,
) -> TokenStream {
    let values: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let member = field
                .ident
                .clone()
                .map(syn::Member::Named)
                .unwrap_or_else(|| syn::Member::Unnamed(syn::Index::from(index)));
            let expression = value(member.clone());
            if matches!(fields, Fields::Named(_)) {
                quote!(#member: #expression)
            } else {
                expression
            }
        })
        .collect();
    match fields {
        Fields::Named(_) => quote!(#target { #(#values),* }),
        Fields::Unnamed(_) => quote!(#target ( #(#values),* )),
        Fields::Unit => target,
    }
}

fn selected_variant(input: &DeriveInput, args: &Options) -> syn::Result<Option<TokenStream>> {
    let (Data::Enum(data), Some(DefaultValue::Expression(Expr::Path(expression)))) =
        (&input.data, &args.default)
    else {
        return Ok(None);
    };
    let path = &expression.path;
    if expression.qself.is_some() || path.leading_colon.is_some() {
        return Ok(None);
    }
    let segments: Vec<_> = path.segments.iter().collect();
    let selected = match segments.as_slice() {
        [variant] if matches!(variant.arguments, syn::PathArguments::None) => &variant.ident,
        [owner, variant]
            if (owner.ident == "Self" || owner.ident == input.ident)
                && matches!(owner.arguments, syn::PathArguments::None)
                && matches!(variant.arguments, syn::PathArguments::None) =>
        {
            &variant.ident
        }
        _ => return Ok(None),
    };
    let Some(variant) = data
        .variants
        .iter()
        .find(|variant| variant.ident == *selected)
    else {
        return Ok(None);
    };
    let ident = &variant.ident;
    Ok(Some(default_constructor(
        &variant.fields,
        quote!(Self::#ident),
    )?))
}

fn default_constructor(fields: &Fields, target: TokenStream) -> syn::Result<TokenStream> {
    let mut defaults = Vec::new();
    for field in fields {
        let args = Options::parse(&field.attrs, Location::Field)?;
        let value = match args.default {
            Some(value) => value.expression(),
            None if is_option(&field.ty) => quote!(::core::option::Option::None),
            None => {
                return Err(syn::Error::new_spanned(
                    field,
                    "fields of a default value require #[config(default...)] or Option<T>",
                ));
            }
        };
        defaults.push(value);
    }
    let mut values = defaults.into_iter();
    Ok(construct(fields, target, |_| {
        values.next().unwrap_or_default()
    }))
}

fn is_option(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "Option"))
}

fn declaration(
    input: &DeriveInput,
    args: &Options,
    runtime: &TokenStream,
) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let key = args.key.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(name, "Config requires #[config(key = \"section\")]")
    })?;
    if key.value().contains('.') {
        return Err(syn::Error::new_spanned(
            key,
            "key must be a single path segment; use parent = ParentConfig for child sections",
        ));
    }
    if key.value().is_empty()
        || !key
            .value()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(syn::Error::new_spanned(
            key,
            "key must contain non-empty ASCII letters, digits, underscores or hyphens",
        ));
    }
    let field_names: Vec<_> = if let Data::Struct(data) = &input.data {
        data.fields
            .iter()
            .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
            .map(|(field, ident)| {
                let options = Options::parse(&field.attrs, Location::Field)?;
                Ok(external_name(ident, options.rename))
            })
            .collect::<syn::Result<_>>()?
    } else {
        Vec::new()
    };
    let full_key = if let Some(parent) = &args.parent {
        quote!({
            const LENGTH: usize = <#parent as #runtime::ConfigSection>::KEY.len() + 1 + #key.len();
            const BYTES: [u8; LENGTH] = #runtime::__private::child_key::<#parent, LENGTH>(#key);
            match ::core::str::from_utf8(&BYTES) {
                ::core::result::Result::Ok(key) => key,
                ::core::result::Result::Err(_) => ::core::panic!("invalid configuration key"),
            }
        })
    } else {
        quote!(#key)
    };
    let extensible = if matches!(&input.data, Data::Struct(data) if matches!(data.fields, Fields::Named(_)))
        && !args.deny_unknown_fields
    {
        quote!(impl #runtime::__private::ExtensibleSection for #name {})
    } else {
        TokenStream::new()
    };
    let spaces = args
        .spaces
        .clone()
        .unwrap_or_else(|| vec![LitStr::new("default", Span::call_site())]);
    let mut names = std::collections::HashSet::new();
    if spaces.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "spaces must contain at least one name",
        ));
    }
    for space in &spaces {
        let value = space.value();
        if value.is_empty()
            || value
                .chars()
                .any(|ch| ch.is_whitespace() || ch.is_control())
        {
            return Err(syn::Error::new_spanned(
                space,
                "space names must be non-empty and contain no whitespace or control characters",
            ));
        }
        if !names.insert(value) {
            return Err(syn::Error::new_spanned(space, "duplicate space name"));
        }
    }
    let defaults = if args.default.is_some() {
        quote!(::core::option::Option::Some(<#name as ::core::default::Default>::default()))
    } else {
        quote!(::core::option::Option::<#name>::None)
    };
    let validate = args.validate.as_ref().map(|check| quote!(
        ::core::option::Option::Some(
            (|value: &#name| #check(value).map_err(::core::convert::Into::into))
            as fn(&#name) -> ::core::result::Result<(), #runtime::__private::ValidationError>
        )
    )).unwrap_or_else(|| quote!(::core::option::Option::None));
    let replace = matches!(input.data, Data::Enum(_));
    Ok(quote! {
        impl #runtime::ConfigSection for #name {
            const KEY: &'static str = #full_key;
        }
        impl #runtime::__private::SectionFields for #name {
            const FIELDS: &'static [&'static str] = &[#(#field_names),*];
        }
        #extensible
        #[#runtime::__private::linkme::distributed_slice(#runtime::__private::DECLARATIONS)]
        #[linkme(crate = #runtime::__private::linkme)]
        static DECLARATION: #runtime::__private::Declaration =
            #runtime::__private::Declaration::new::<#name>(
                <#name as #runtime::ConfigSection>::KEY, &[#(#spaces),*], || {
                    #runtime::__private::prepare::<#name>(
                        <#name as #runtime::ConfigSection>::KEY,
                        #defaults, #validate, #replace
                    )
                }
            );
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_variant_bindings_use_shorthand_in_patterns_and_expressions() -> syn::Result<()> {
        use syn::parse::Parser;

        let variant: syn::Variant = syn::parse_quote!(Memory {
            capacity: u64,
            r#type: String
        });
        let tokens = variant_binding(&variant.fields, quote!(Self::Memory));
        let expression: syn::ExprStruct = syn::parse2(tokens.clone())?;
        assert!(
            expression
                .fields
                .iter()
                .all(|field| field.colon_token.is_none())
        );
        let pattern = syn::Pat::parse_single.parse2(tokens)?;
        let syn::Pat::Struct(pattern) = pattern else {
            return Err(syn::Error::new(
                Span::call_site(),
                "expected a struct pattern",
            ));
        };
        assert!(
            pattern
                .fields
                .iter()
                .all(|field| field.colon_token.is_none())
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_declarations() -> syn::Result<()> {
        for (tokens, message) in [
            (
                quote!(
                    #[config(key = "a", rename_all = "snake_case")]
                    struct A {}
                ),
                "fixed snake_case",
            ),
            (
                quote!(
                    #[config(key = "a")]
                    struct A {
                        first: u16,
                        #[config(rename = "first")]
                        second: u16,
                    }
                ),
                "duplicate external field name",
            ),
            (
                quote!(
                    #[config(key = "a")]
                    enum A {
                        LocalFile,
                        #[config(rename = "local_file")]
                        Other,
                    }
                ),
                "duplicate external variant name",
            ),
            (
                quote!(
                    struct A {}
                ),
                "requires",
            ),
            (
                quote!(
                    #[config(key = "a", tag = "kind")]
                    struct A {}
                ),
                "only on enums",
            ),
            (
                quote!(
                    #[config(key="a", default=1, default=f())]
                    struct A {}
                ),
                "duplicate default",
            ),
            (
                quote!(
                    #[config(key = "a")]
                    enum A {
                        X { value: u16 },
                    }
                ),
                "require",
            ),
            (
                quote!(
                    #[config(key = "a")]
                    enum A {
                        #[config(default)]
                        X,
                        #[config(default)]
                        Y,
                    }
                ),
                "select the default on the enum",
            ),
            (
                quote!(
                    #[config(key = "a", tag = "kind", default = Self::X)]
                    enum A {
                        X { value: u16 },
                    }
                ),
                "fields of a default",
            ),
            (
                quote!(
                    #[config(key = "a")]
                    struct A<T> {
                        value: T,
                    }
                ),
                "non-generic",
            ),
            (
                quote!(
                    #[config(key="a", spaces=[])]
                    struct A {}
                ),
                "at least one",
            ),
            (
                quote!(
                    #[config(key="a", spaces=["x", "x"])]
                    struct A {}
                ),
                "duplicate space",
            ),
            (
                quote!(
                    #[config(key = "a..b")]
                    struct A {}
                ),
                "single path segment",
            ),
            (
                quote!(
                    #[config(key = "a")]
                    struct A {
                        #[serde(default)]
                        value: u16,
                    }
                ),
                "instead of",
            ),
        ] {
            let result = expand(syn::parse2(tokens)?, true, quote!(::settings));
            assert!(
                result
                    .err()
                    .is_some_and(|error| error.to_string().contains(message)),
                "expected {message}"
            );
        }
        Ok(())
    }

    #[test]
    fn value_derives_do_not_register_sections() -> syn::Result<()> {
        let input = syn::parse_quote!(
            #[config(default = Self::MiB)]
            enum Unit {
                MiB,
            }
        );
        let output = expand(input, false, quote!(::settings))?.to_string();
        assert!(output.contains("Serialize"));
        assert!(output.contains("Deserialize"));
        assert!(output.contains("Default"));
        assert!(!output.contains("DECLARATIONS"));
        Ok(())
    }

    #[test]
    fn nested_values_cannot_override_the_naming_convention() -> syn::Result<()> {
        let input = syn::parse_quote!(
            #[config(rename_all = "camelCase")]
            struct Value {
                field_name: u16,
            }
        );
        let result = expand(input, false, quote!(::settings));
        assert!(
            result
                .err()
                .is_some_and(|error| error.to_string().contains("fixed snake_case"))
        );
        Ok(())
    }
}
