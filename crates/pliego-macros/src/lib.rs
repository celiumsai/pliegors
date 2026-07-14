// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Procedural syntax for PliegoRS's typed `pliego_dom::View` tree.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote};
use rstml::node::{
    Infallible, KeyedAttributeValue, Node, NodeAttribute, NodeBlock, NodeElement, NodeName,
};
use syn::{FnArg, ItemFn, Pat, ReturnType, parse_macro_input};

/// Turn a Rust function into a typed PliegoRS component.
///
/// The first contract generates `<Name>Props`, keeps all declared properties
/// required, and always provides a `children: pliego_dom::View` field.
#[proc_macro_attribute]
pub fn component(_attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    match expand_component(function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Build a typed PliegoRS view from RSX.
#[proc_macro]
pub fn view(input: TokenStream) -> TokenStream {
    match expand_view(input.into()) {
        Ok(tokens) => quote! {{
            #[allow(unused_braces)]
            let __pliego_view = #tokens;
            __pliego_view
        }}
        .into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_view(input: TokenStream2) -> syn::Result<TokenStream2> {
    let nodes = rstml::parse2(input).map_err(|errors| {
        errors
            .into_iter()
            .reduce(|mut combined, error| {
                combined.combine(error);
                combined
            })
            .unwrap_or_else(|| syn::Error::new(proc_macro2::Span::call_site(), "invalid RSX"))
    })?;
    expand_nodes(&nodes)
}

fn expand_component(function: ItemFn) -> syn::Result<TokenStream2> {
    if !function.sig.generics.params.is_empty()
        || function.sig.asyncness.is_some()
        || function.sig.constness.is_some()
        || function.sig.abi.is_some()
    {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "the first #[component] contract requires a non-generic synchronous Rust function",
        ));
    }
    if matches!(function.sig.output, ReturnType::Default) {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "a PliegoRS component must declare its View return type",
        ));
    }
    let name = &function.sig.ident;
    if !name
        .to_string()
        .chars()
        .next()
        .is_some_and(char::is_uppercase)
    {
        return Err(syn::Error::new_spanned(
            name,
            "component functions must use PascalCase so they are distinct from HTML tags",
        ));
    }
    let props_name = format_ident!("{}Props", name);
    let visibility = &function.vis;
    let attributes = &function.attrs;
    let output = &function.sig.output;
    let body = &function.block;
    let mut fields = Vec::new();
    let mut bindings = Vec::new();
    let mut has_children = false;

    for argument in &function.sig.inputs {
        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "component methods with self are not supported",
            ));
        };
        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &argument.pat,
                "component properties must be simple named parameters",
            ));
        };
        let ident = &pattern.ident;
        let ty = &argument.ty;
        if ident == "children" {
            has_children = true;
        }
        fields.push(quote! { pub #ident: #ty });
        let mutability = &pattern.mutability;
        bindings.push(quote! { #mutability #ident });
    }
    if !has_children {
        fields.push(quote! { pub children: ::pliego_dom::View });
    }
    let children_binding = (!has_children).then(|| quote! { children: _ });

    Ok(quote! {
        #visibility struct #props_name {
            #(#fields,)*
        }

        #(#attributes)*
        #[allow(non_snake_case)]
        #visibility fn #name(props: #props_name) #output {
            let #props_name {
                #(#bindings,)*
                #children_binding
            } = props;
            #body
        }
    })
}

fn expand_nodes(nodes: &[Node]) -> syn::Result<TokenStream2> {
    let children = nodes
        .iter()
        .map(expand_node)
        .collect::<syn::Result<Vec<_>>>()?;
    if let [only] = children.as_slice() {
        Ok(quote! { ::pliego_dom::IntoView::into_view(#only) })
    } else {
        Ok(quote! {
            ::pliego_dom::View::Fragment(vec![
                #(::pliego_dom::IntoView::into_view(#children)),*
            ])
        })
    }
}

fn expand_node(node: &Node) -> syn::Result<TokenStream2> {
    match node {
        Node::Element(element) => expand_element(element),
        Node::Fragment(fragment) => expand_nodes(&fragment.children),
        Node::Text(text) => {
            let value = &text.value;
            Ok(quote! { ::pliego_dom::text(#value) })
        }
        Node::RawText(text) => {
            let value = text.to_string_best();
            Ok(quote! { ::pliego_dom::text(#value) })
        }
        Node::Block(NodeBlock::ValidBlock(block)) => Ok(quote! { #block }),
        Node::Block(NodeBlock::Invalid(block)) => Err(syn::Error::new_spanned(
            block,
            "PliegoRS requires a valid Rust expression block",
        )),
        unsupported => Err(syn::Error::new_spanned(
            unsupported,
            "this RSX node is not supported by the first PliegoRS view! contract",
        )),
    }
}

fn expand_element(element: &NodeElement<Infallible>) -> syn::Result<TokenStream2> {
    let tag = node_name(element.name())?;
    if tag.chars().next().is_some_and(char::is_uppercase) || tag.contains("::") {
        return expand_component_element(element);
    }

    let mut expression = quote! { ::pliego_dom::el(#tag) };
    for attribute in element.attributes() {
        let NodeAttribute::Attribute(attribute) = attribute else {
            return Err(syn::Error::new_spanned(
                attribute,
                "spread attributes are not enabled yet",
            ));
        };
        let name = node_name(&attribute.key)?;
        expression = match &attribute.possible_value {
            KeyedAttributeValue::Value(value) => {
                let value = match &value.value {
                    rstml::node::KVAttributeValue::Expr(value) => value,
                    rstml::node::KVAttributeValue::InvalidBraced(value) => {
                        return Err(syn::Error::new_spanned(
                            value,
                            "invalid attribute expression",
                        ));
                    }
                };
                quote! { #expression.attr(#name, (#value).to_string()) }
            }
            KeyedAttributeValue::None => quote! { #expression.attr(#name, "") },
            KeyedAttributeValue::Binding(binding) => {
                return Err(syn::Error::new_spanned(
                    binding,
                    "attribute bindings are not enabled yet",
                ));
            }
        };
    }
    for child in &element.children {
        let child = expand_node(child)?;
        expression = quote! { #expression.child(#child) };
    }
    Ok(expression)
}

fn expand_component_element(element: &NodeElement<Infallible>) -> syn::Result<TokenStream2> {
    let NodeName::Path(component_path) = element.name() else {
        return Err(syn::Error::new_spanned(
            element.name(),
            "component tags must be Rust paths",
        ));
    };
    let Some(component_segment) = component_path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            element.name(),
            "empty component path",
        ));
    };
    if component_path.path.segments.len() != 1 {
        return Err(syn::Error::new_spanned(
            component_path,
            "qualified component paths are not enabled yet; import the component first",
        ));
    }
    let component_name = &component_segment.ident;
    let props_name = format_ident!("{}Props", component_name);
    let mut fields = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for attribute in element.attributes() {
        let NodeAttribute::Attribute(attribute) = attribute else {
            return Err(syn::Error::new_spanned(
                attribute,
                "spread component properties are not enabled yet",
            ));
        };
        let NodeName::Path(path) = &attribute.key else {
            return Err(syn::Error::new_spanned(
                &attribute.key,
                "component property names must be Rust identifiers",
            ));
        };
        if path.path.segments.len() != 1 {
            return Err(syn::Error::new_spanned(
                path,
                "component property names cannot contain a path",
            ));
        }
        let field = &path.path.segments[0].ident;
        if field == "children" {
            return Err(syn::Error::new_spanned(
                field,
                "children are supplied between the component tags",
            ));
        }
        if !seen.insert(field.to_string()) {
            return Err(syn::Error::new_spanned(
                field,
                format!("duplicate component property: {field}"),
            ));
        }
        let KeyedAttributeValue::Value(value) = &attribute.possible_value else {
            return Err(syn::Error::new_spanned(
                attribute,
                "component properties require explicit values",
            ));
        };
        let rstml::node::KVAttributeValue::Expr(value) = &value.value else {
            return Err(syn::Error::new_spanned(
                value,
                "invalid component property expression",
            ));
        };
        fields.push(quote! { #field: ::core::convert::Into::into(#value) });
    }
    let children = expand_nodes(&element.children)?;
    Ok(quote! {
        #component_name(#props_name {
            #(#fields,)*
            children: #children,
        })
    })
}

fn node_name(name: &NodeName) -> syn::Result<String> {
    match name {
        NodeName::Path(path) => Ok(path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::")),
        NodeName::Punctuated(_) => Ok(name.to_token_stream().to_string().replace(' ', "")),
        NodeName::Block(block) => Err(syn::Error::new_spanned(
            block,
            "dynamic tag names are not supported",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_attribute_generates_typed_props_and_wrapper() {
        let function: ItemFn = syn::parse_quote! {
            pub fn Card(title: String, children: ::pliego_dom::View) -> ::pliego_dom::View {
                children
            }
        };
        let expanded = expand_component(function).unwrap().to_string();
        assert!(expanded.contains("struct CardProps"));
        assert!(expanded.contains("pub title : String"));
        assert!(expanded.contains("pub children : :: pliego_dom :: View"));
        assert!(expanded.contains("fn Card (props : CardProps)"));
    }

    #[test]
    fn component_tag_generates_props_and_children() {
        let expanded = expand_view(quote! {
            <Card title="Reference"><strong>"status"</strong></Card>
        })
        .unwrap()
        .to_string();
        assert!(expanded.contains("Card (CardProps"));
        assert!(expanded.contains("title : :: core :: convert :: Into :: into"));
        assert!(expanded.contains("children :"));
    }

    #[test]
    fn duplicate_component_props_fail_during_macro_expansion() {
        let error = expand_view(quote! { <Card title="a" title="b" /> }).unwrap_err();
        assert!(error.to_string().contains("duplicate component property"));
    }
}
