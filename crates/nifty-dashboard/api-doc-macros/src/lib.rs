extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    parse_macro_input, parse_str, AttributeArgs, Ident, ItemFn, Lit, Meta, MetaNameValue,
    NestedMeta, Path, Type,
};

//
// #[api_doc(...)] attribute macro
//
#[proc_macro_attribute]
pub fn api_doc(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as AttributeArgs);

    let mut id: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut ok_ty_str: Option<String> = None;
    let mut err_ty_str: Option<String> = None;

    for arg in args {
        if let NestedMeta::Meta(Meta::NameValue(MetaNameValue { path, lit, .. })) = arg {
            if let Some(ident) = path.get_ident() {
                match (ident.to_string().as_str(), &lit) {
                    ("id", Lit::Str(s)) => id = Some(s.value()),
                    ("tag", Lit::Str(s)) => tag = Some(s.value()),
                    ("ok", Lit::Str(s)) => ok_ty_str = Some(s.value()),
                    ("err", Lit::Str(s)) => err_ty_str = Some(s.value()),
                    _ => {}
                }
            }
        }
    }

    let func = parse_macro_input!(item as ItemFn);
    let fn_name = func.sig.ident.clone();
    let vis = func.vis.clone();

    // doc comments -> summary + description
    let mut doc_lines: Vec<String> = Vec::new();
    for attr in &func.attrs {
        if attr.path.is_ident("doc") {
            if let Ok(Meta::NameValue(MetaNameValue {
                lit: Lit::Str(s), ..
            })) = attr.parse_meta()
            {
                doc_lines.push(s.value());
            }
        }
    }

    let mut summary: Option<String> = None;
    let mut description: Option<String> = None;

    if !doc_lines.is_empty() {
        summary = Some(doc_lines[0].trim().to_string());
        if doc_lines.len() > 1 {
            let rest = doc_lines[1..].join("\n");
            if !rest.trim().is_empty() {
                description = Some(rest.trim().to_string());
            }
        }
    }

    // Default id to function name if not provided
    let id_value = id.unwrap_or_else(|| fn_name.to_string());

    // Generated docs function name: foo -> foo_docs
    let docs_fn_name = Ident::new(&(fn_name.to_string() + "_docs"), Span::mixed_site());

    // Optional summary / description / tag
    let summary_ts = if let Some(s) = summary {
        quote! { op = op.summary(#s); }
    } else {
        quote! {}
    };

    let description_ts = if let Some(d) = description {
        quote! { op = op.description(#d); }
    } else {
        quote! {}
    };

    let tag_ts = if let Some(t) = tag {
        quote! { op = op.tag(#t); }
    } else {
        quote! {}
    };

    // Optional 200 OK response
    let ok_ts = if let Some(s) = ok_ty_str {
        let ty: Type = parse_str(&s).expect("failed to parse ok type in #[api_doc]");
        quote! {
            op = op.response_with::<200, #ty, _>(|res| {
                res.description("OK")
            });
        }
    } else {
        quote! {}
    };

    // Optional 500 Error response
    let err_ts = if let Some(s) = err_ty_str {
        let ty: Type = parse_str(&s).expect("failed to parse err type in #[api_doc]");
        quote! {
            op = op.response_with::<500, #ty, _>(|res| {
                res.description("Error")
            });
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #func

        #vis fn #docs_fn_name(
            mut op: ::aide::transform::TransformOperation,
        ) -> ::aide::transform::TransformOperation {
            op = op.id(#id_value);
            #tag_ts
            #summary_ts
            #description_ts
            #ok_ts
            #err_ts
            op
        }
    };

    TokenStream::from(expanded)
}

fn expand_with_docs(input: TokenStream, method: &str) -> TokenStream {
    let handler_path = parse_macro_input!(input as Path);

    // Build docs path: foo::bar -> foo::bar_docs
    let mut docs_path = handler_path.clone();
    if let Some(last) = docs_path.segments.last_mut() {
        let fn_ident = &last.ident;
        let docs_ident = Ident::new(&format!("{}_docs", fn_ident), fn_ident.span());
        last.ident = docs_ident;
    } else {
        return syn::Error::new_spanned(
            handler_path,
            "*_with_docs! expects a non-empty path, e.g. healthz::healthz",
        )
        .to_compile_error()
        .into();
    }

    let method_ident = Ident::new(method, Span::call_site());

    let expanded = quote! {
        ::aide::axum::routing::#method_ident(#handler_path, #docs_path)
    };

    TokenStream::from(expanded)
}

#[proc_macro]
pub fn get_with_docs(input: TokenStream) -> TokenStream {
    // expands to ::aide::axum::routing::get_with(handler, handler_docs)
    expand_with_docs(input, "get_with")
}

#[proc_macro]
pub fn post_with_docs(input: TokenStream) -> TokenStream {
    expand_with_docs(input, "post_with")
}

#[proc_macro]
pub fn put_with_docs(input: TokenStream) -> TokenStream {
    expand_with_docs(input, "put_with")
}

#[proc_macro]
pub fn delete_with_docs(input: TokenStream) -> TokenStream {
    expand_with_docs(input, "delete_with")
}

#[proc_macro]
pub fn patch_with_docs(input: TokenStream) -> TokenStream {
    expand_with_docs(input, "patch_with")
}

#[proc_macro]
pub fn options_with_docs(input: TokenStream) -> TokenStream {
    expand_with_docs(input, "options_with")
}

#[proc_macro]
pub fn head_with_docs(input: TokenStream) -> TokenStream {
    expand_with_docs(input, "head_with")
}
