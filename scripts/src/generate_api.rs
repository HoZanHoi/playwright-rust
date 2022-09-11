use case::CaseExt;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens, TokenStreamExt};
use scripts::{api::*, utils};
use std::collections::{HashMap, VecDeque};

fn main() {
    let api: Api = serde_json::from_reader(std::io::stdin()).unwrap();
    let t = to_tokens(&api);
    println!("{}\n// vim: foldnestmax=0 ft=rust", t);
}

fn to_tokens(api: &Api) -> TokenStream {
    let mut tokens = TokenStream::default();
    tokens.append_all(api.0.iter().map(body));
    tokens
}

fn body(x: &Interface) -> TokenStream {
    let name = format_ident!("{}", utils::loud_to_camel(&x.name));
    let mod_name = format_ident!("{}", utils::loud_to_camel(&x.name).to_snake());
    let extends = x.extends.as_deref().map(|e| {
        let e = format!("Extends {}", e);
        quote! { #[doc=#e] }
    });
    // TODO: doc_comment
    let mut overload_targets: HashMap<&str, Vec<&Member>> = x
        .members
        .iter()
        .filter(|m| m.overload_index > 0)
        .filter(|m| matches!(m.kind, Kind::Property | Kind::Method))
        .fold(HashMap::new(), |mut a, b| {
            a.entry(&*b.alias)
                .and_modify(|xs| xs.push(b))
                .or_insert(vec![b]);
            a
        });
    let methods = x
        .members
        .iter()
        .filter(|m| matches!(m.kind, Kind::Property | Kind::Method))
        .filter(|m| m.overload_index == 0)
        .map(|m| {
            let overloads = overload_targets.remove(&*m.alias);
            method_tokens(m, overloads)
        });
    let events = {
        let xs = x
            .members
            .iter()
            .filter(|m| matches!(m.kind, Kind::Event))
            .collect::<Vec<_>>();
        if xs.is_empty() {
            quote! {}
        } else {
            event_tokens(xs)
        }
    };
    let sub = collect_types(x);
    quote! {
        mod #mod_name {
            #extends
            impl #name {
                #(#methods)*
            }
            #events
            #sub
        }
    }
}

fn event_tokens(member: Vec<&Member>) -> TokenStream {
    let variants = member.iter().map(|e| {
        assert_eq!(e.args, &[]);
        assert_eq!(e.deprecated, false);
        assert_eq!(e.is_async, false);
        assert_eq!(e.name, e.alias);
        assert_eq!(e.experimental, false);
        assert_eq!(e.overload_index, 0);
        assert_eq!(e.required, true);
        // TODO: spec
        let label = format_ident!("{}", utils::loud_to_camel(&e.name.to_camel()));
        if e.ty.name == "void" {
            quote! {
                #label
            }
        } else {
            let ty = use_ty("", &e.name, &e.ty, false); // TODO
            quote! {
                #label(#ty)
            }
        }
    });
    let labels = member
        .iter()
        .map(|e| format_ident!("{}", utils::loud_to_camel(&e.name.to_camel())));
    quote! {
        #[derive(Debug, Clone)]
        pub enum Event {
            #(#variants),*
        }
        #[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
        pub enum EventType {
            #(#labels),*
        }
    }
}
fn method_tokens(member: &Member, overloads: Option<Vec<&Member>>) -> TokenStream {
    let mut tokens: TokenStream = Default::default();
    let Member {
        kind: _,
        name,
        alias,
        experimental,
        since: _,
        overload_index: _,
        required,
        is_async,
        args,
        ty,
        deprecated,
        spec // TODO
    } = member;
    assert!(name == alias || name.starts_with(alias), "{}", name);
    let is_builder = needs_builder(member);
    let rety: Box<dyn ToTokens> = if is_builder {
        let name = format!("{}{}", &name.replace("#", ""), "Builder");
        // TODO: make type for builder
        Box::new(use_ty("", &name, ty, true))
    } else {
        Box::new(use_ty("", name, ty, false))
    };
    let arg_fields = args
        .iter()
        .filter(|a| a.required)
        .filter(|a| !types::is_action_csharp(&a))
        .map(|a| arg_field(alias, a, true));
    let fn_name = if is_builder {
        format_ident!(
            "{}_builder",
            utils::loud_to_camel(&name.replace("#", "")).to_snake()
        )
    } else {
        format_ident!("{}", utils::loud_to_snake(&name.replace("#", "")))
    };
    let mark_async = (!is_builder && *is_async)
        .then(|| quote!(async))
        .unwrap_or_default();
    let doc_unnecessary = (!required)
        .then(|| quote!(#[doc="unnecessary"]))
        .unwrap_or_default();
    let doc_experimental = experimental
        .then(|| quote!(#[doc="experimental"]))
        .unwrap_or_default();
    let mark_deprecated = deprecated
        .then(|| quote!(#[deprecated]))
        .unwrap_or_default();
    tokens.extend(quote! {
        #doc_unnecessary
        #doc_experimental
        #mark_deprecated
        #mark_async fn #fn_name(#(#arg_fields),*) -> #rety {
            todo!()
        }
    });
    tokens
}

/// has two or more optional values
fn needs_builder(member: &Member) -> bool {
    let args = &member.args;
    let mut xs = args.iter().filter(|a| !a.required).chain(
        args.iter()
            .filter(|a| a.name == "options" && !a.ty.properties.is_empty())
            .flat_map(|a| a.ty.properties.iter())
    );
    xs.next().and(xs.next()).is_some()
}

fn arg_field(scope: &str, a: &Arg, borrow: bool) -> TokenStream {
    let Arg {
        name,
        kind: _,
        alias,
        ty,
        since: _,
        overload_index,
        spec, // TODO
        required,
        deprecated,
        is_async,
        langs: _
    } = a;
    assert_eq!(*is_async, false);
    assert_eq!(*overload_index, 0);
    assert_eq!(alias, name);
    let field_name = format_ident!("{}", utils::loud_to_snake(name));
    let use_ty = {
        let t = use_ty(scope, name, ty, borrow);
        if *required {
            quote!(#t)
        } else {
            quote!(Option<#t>)
        }
    };
    quote! {
        #field_name: #use_ty
    }
}

fn collect_types(x: &Interface) -> TokenStream {
    let mut ret = TokenStream::default();
    for member in &x.members {
        for arg in &member.args {
            if types::is_action_csharp(arg) {
                continue;
            }
            ret.extend(declare_ty(&member.name, &arg.name, &arg.ty, true));
        }
        ret.extend(declare_ty(&member.name, "", &member.ty, false));
    }
    ret
}

// TODO
fn use_ty(scope: &str, name: &str, ty: &Type, borrow: bool) -> TokenStream {
    let opt = ty.name.ends_with("?");
    let s = ty.name.replace("?", "");
    if ty.union.is_empty() {
        match (ty.properties.is_empty(), ty.templates.is_empty()) {
            (true, true) => {
                let label = match &*s {
                    "binary" if borrow => quote!(&'a [u8]),
                    "binary" => quote!(Vec<u8>),
                    "number" => quote!(serde_json::Number),
                    "float" => quote!(f64),
                    "json" if borrow => quote!(&'a str),
                    "json" => quote!(String),
                    "string" if borrow => quote!(&'a str),
                    "string" => quote!(String),
                    "boolean" => quote!(bool),
                    "void" => quote!(()),
                    x => {
                        let ident = format_ident!("{}", utils::loud_to_camel(x));
                        quote!(#ident)
                    }
                };
                if opt {
                    quote!(Option<#label>)
                } else {
                    quote!(#label)
                }
            }
            (false, true) => {
                assert_eq!(ty.name, "Object");
                let ident = format_ident!(
                    "{}{}",
                    utils::loud_to_camel(&scope.to_camel()),
                    utils::loud_to_camel(&name.to_camel())
                );
                quote! {
                    #ident
                }
            }
            (true, false) if ty.name == "Func" => unreachable!(),
            (true, false) if ty.name == "Array" => {
                assert_eq!(ty.templates.len(), 1);
                let inner = use_ty(scope, name, &ty.templates[0], borrow);
                if borrow {
                    quote! {
                        &[#inner]
                    }
                } else {
                    quote! {
                        Vec<#inner>
                    }
                }
            }
            (true, false) => {
                // 148 Array
                // 12 Func
                //  1 IReadOnlyDictionary
                //  1 Map
                // 56 Object
                let ident = format_ident!(
                    "{}{}",
                    utils::loud_to_camel(&scope.to_camel()),
                    utils::loud_to_camel(&name.to_camel())
                );
                quote! {
                    #ident
                }
            }
            (false, false) => {
                assert_eq!(ty.name, "Object");
                todo!()
            }
        }
    } else {
        assert_eq!(ty.properties, &[]);
        assert_eq!(ty.templates, &[]);
        let variants = ty.union.iter().filter(|t| t.name != "null");
        let num_variants = variants.clone().count();
        let opt = ty.union.len() != num_variants;
        match num_variants {
            0 => unreachable!(),
            1 => {
                let mut vs = variants;
                let t = vs.next().unwrap();
                let t = use_ty(scope, name, t, borrow);
                if opt {
                    quote!(Opiton<#t>)
                } else {
                    quote!(#t)
                }
            }
            _ => {
                let ident = format_ident!(
                    "{}{}",
                    utils::loud_to_camel(&scope.to_camel()),
                    utils::loud_to_camel(&name.to_camel())
                );
                if opt {
                    quote!(Option<#ident>)
                } else {
                    quote!(#ident)
                }
            }
        }
    }
}

// TODO
fn has_reference(ty: &Type) -> bool { todo!() }

// TODO
fn declare_ty(scope: &str, name: &str, ty: &Type, borrow: bool) -> TokenStream {
    if ty.union.is_empty() {
        match (ty.properties.is_empty(), ty.templates.is_empty()) {
            (true, true) => {
                quote! {}
            }
            (false, true) => {
                let name = format!(
                    "{}{}",
                    utils::loud_to_camel(&scope.to_camel().replace("#", "")),
                    utils::loud_to_camel(&name.to_camel())
                );
                let ident = format_ident!("{}", &name);
                let fields = ty.properties.iter().map(|p| {
                    let field_name = format_ident!("{}", utils::loud_to_snake(&p.name));
                    let use_ty = {
                        let t = use_ty(&name, &p.name, ty, borrow);
                        if p.required {
                            quote!(#t)
                        } else {
                            quote!(Option<#t>)
                        }
                    };
                    quote! {
                        #field_name: #use_ty
                    }
                });
                quote! {
                    pub struct #ident {
                        #(#fields),*
                    }
                }
            }
            (true, false) if ty.name == "Func" => unreachable!(),
            (true, false) if ty.name == "Array" => {
                assert_eq!(ty.templates.len(), 1);
                let t = &ty.templates[0];
                declare_ty(scope, name, t, borrow)
            }
            (true, false) => {
                assert!(
                    ty.expression.as_deref()
                        == Some("[IReadOnlyDictionary<string, BrowserNewContextOptions>]")
                        || ty.expression.as_deref() == Some("[Map]<[string], [JSHandle]>")
                        || ty.expression.as_deref() == Some("[Object]<[string], [string]>"),
                    "{:?}",
                    &ty
                );
                let it = ty
                    .templates
                    .iter()
                    .map(|t| declare_ty(scope, name, t, borrow));
                quote! {
                    #(#it)*
                }
            }
            (true, false) => {
                let ident = format_ident!(
                    "{}{}",
                    utils::loud_to_camel(&scope.to_camel().replace("#", "")),
                    utils::loud_to_camel(&name.to_camel())
                );
                let vars = ty.templates.iter().enumerate().map(|(i, t)| {
                    let c = format_ident!("{}", char::from_u32('A' as u32 + i as u32).unwrap());
                    quote! {
                        #c
                    }
                });
                quote! {
                    struct #ident<#(#vars),*> {
                    }
                }
            }
            (false, false) => {
                assert_eq!(
                    ty.expression.as_deref(),
                    Some("[Object]<[string], [string]|[float]|[boolean]|[ReadStream]|[Object]>")
                );
                quote! {}
            }
        }
    } else {
        let variants = ty.union.iter().filter(|t| t.name != "null");
        let num_variants = variants.clone().count();
        match num_variants {
            0 => unreachable!(),
            1 => {
                let mut vs = variants;
                let t = vs.next().unwrap();
                declare_ty(scope, name, t, borrow)
            }
            _ => {
                let s = format!(
                    "{}{}",
                    utils::loud_to_camel(&scope.to_camel().replace("#", "")),
                    utils::loud_to_camel(&name.to_camel())
                );
                enum_tokens(&s, ty)
            }
        }
    }

    // let mut tokens = Default::default();
    // if ty.union.is_empty() {
    //    if ty.properties.is_empty() && ty.templates.is_empty() {
    //        return tokens;
    //    }
    //    let name = format_ident!("{}", prefix.replace("#", ""));
    //    match (ty.properties.is_empty(), ty.templates.is_empty()) {
    //        (true, true) => return tokens,
    //        (false, false) => {
    //            assert_eq!(ty.name, "Object");
    //        }
    //        (false, true) => {
    //            assert_eq!(ty.name, "Object");
    //            let properties = ty.properties.iter().map(|p| {
    //                let deprecated = p
    //                    .deprecated
    //                    .then(|| quote!(#[deprecated]))
    //                    .unwrap_or_default();
    //                let name = format_ident!("{}", utils::loud_to_snake(&p.name));
    //                let orig = &p.name;
    //                // TODO: doc_comment
    //                let use_ty = {
    //                    let a = use_ty("", "", &p.ty, borrow);
    //                    if p.required {
    //                        quote!(#a)
    //                    } else {
    //                        quote!(Option<#a>)
    //                    }
    //                };
    //                quote! {
    //                    #deprecated
    //                    #[serde(rename = #orig)]
    //                    #name: #use_ty
    //                }
    //            });
    //            tokens.extend(quote! {
    //                #[derive(Debug, Serialize, Deserialize)]
    //                struct #name {
    //                    #(#properties),*
    //                }
    //            });
    //        }
    //        (true, false) => {}
    //    }
    //};
}

fn enum_tokens(name: &str, ty: &Type) -> TokenStream {
    assert_eq!(ty.properties, &[]);
    assert_eq!(ty.templates, &[]);
    let enum_name = format_ident!("{}", name);
    let variants = ty.union.iter().filter(|t| t.name != "null").map(|t| {
        if t.name.contains("\"") {
            let name = t.name.replace("\"", "");
            let label = format_ident!("{}", utils::kebab_to_camel(&name));
            quote! {
                #[serde(rename = #name)]
                #label
            }
        } else {
            // TODO
            let name = &t.name;
            let label = format_ident!("{}", utils::kebab_to_camel(&name));
            let use_ty = use_ty(name, &t.name, t, false);
            quote! {
                    #[serde(rename = #name)]
                    #label(#use_ty)
            }
        }
    });
    quote! {
        #[derive(Debug)]
        pub enum #enum_name {
            #(#variants),*
        }
    }
}
