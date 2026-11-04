//! Compiler-syntax public API snapshot; bodies, docs and private fields are excluded.
use quote::ToTokens;
use std::{collections::BTreeMap, path::PathBuf};
use syn::{Fields, ImplItem, Item, Visibility};

fn public(value: &Visibility) -> bool {
    matches!(value, Visibility::Public(_))
}
fn text(value: &impl ToTokens) -> String {
    value.to_token_stream().to_string()
}
fn exports(tree: &syn::UseTree, prefix: String, surface: &mut BTreeMap<String, String>) {
    match tree {
        syn::UseTree::Path(path) => {
            exports(&path.tree, format!("{prefix}{}::", path.ident), surface)
        }
        syn::UseTree::Name(name) => {
            surface.insert(
                format!("export.{}", name.ident),
                format!("{prefix}{}", name.ident),
            );
        }
        syn::UseTree::Rename(name) => {
            surface.insert(
                format!("export.{}", name.rename),
                format!("{prefix}{}", name.ident),
            );
        }
        syn::UseTree::Group(group) => {
            for child in &group.items {
                exports(child, prefix.clone(), surface);
            }
        }
        syn::UseTree::Glob(_) => {
            surface.insert(format!("export.{prefix}*"), "glob".into());
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut surface = BTreeMap::<String, String>::new();
    for file in std::fs::read_dir(root.join("src"))? {
        let path = file?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let source = syn::parse_file(&std::fs::read_to_string(path)?)?;
        for item in source.items {
            match item {
                Item::Use(item) if public(&item.vis) => {
                    exports(&item.tree, String::new(), &mut surface)
                }
                Item::Const(item) if public(&item.vis) => {
                    surface.insert(item.ident.to_string(), text(&item.ty));
                }
                Item::Struct(item) if public(&item.vis) => {
                    let name = item.ident.to_string();
                    surface.insert(
                        name.clone(),
                        format!(
                            "struct {} {}",
                            text(&item.generics),
                            text(&item.generics.where_clause)
                        ),
                    );
                    match item.fields {
                        Fields::Named(fields) => {
                            for field in fields.named {
                                if public(&field.vis) {
                                    surface.insert(
                                        format!("{}.{}", name, field.ident.as_ref().unwrap()),
                                        text(&field.ty),
                                    );
                                }
                            }
                        }
                        Fields::Unnamed(fields) => {
                            for (index, field) in fields.unnamed.into_iter().enumerate() {
                                if public(&field.vis) {
                                    surface.insert(format!("{name}.{index}"), text(&field.ty));
                                }
                            }
                        }
                        Fields::Unit => {}
                    }
                }
                Item::Enum(mut item) if public(&item.vis) => {
                    item.attrs.clear();
                    for variant in &mut item.variants {
                        variant.attrs.clear();
                        for field in &mut variant.fields {
                            field.attrs.clear();
                        }
                    }
                    surface.insert(item.ident.to_string(), text(&item));
                }
                Item::Fn(item) if public(&item.vis) => {
                    surface.insert(item.sig.ident.to_string(), text(&item.sig));
                }
                Item::Type(item) if public(&item.vis) => {
                    surface.insert(item.ident.to_string(), text(&item.ty));
                }
                Item::Impl(item) => {
                    let owner = text(&item.self_ty);
                    if let Some((_, trait_path, _)) = item.trait_ {
                        surface.insert(
                            format!("{owner}: {}", text(&trait_path)),
                            format!(
                                "{} {}",
                                text(&item.generics),
                                text(&item.generics.where_clause)
                            ),
                        );
                    } else {
                        for method in item.items {
                            if let ImplItem::Fn(method) = method {
                                if public(&method.vis) {
                                    surface.insert(
                                        format!("{owner}::{}", method.sig.ident),
                                        text(&method.sig),
                                    );
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    println!("{}", serde_json::to_string(&surface)?);
    Ok(())
}
