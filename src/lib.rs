use proc_macro::TokenStream;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

#[proc_macro]
pub fn embed(input: TokenStream) -> TokenStream {
    if !input.is_empty() {
        return syn::Error::new(
            Span::call_site(),
            "embed!() does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    match expand() {
        Ok(tokens) => tokens.into(),
        Err(error) => syn::Error::new(
            Span::call_site(),
            error,
        )
        .to_compile_error()
        .into(),
    }
}

fn expand() -> Result<TokenStream2, String> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| "CARGO_MANIFEST_DIR is not set".to_string())?;

    let manifest_path = find_manifest(Path::new(&manifest_dir))?;

    let source = fs::read_to_string(&manifest_path)
        .map_err(|e| {
            format!(
                "failed to read {}: {e}",
                manifest_path.display()
            )
        })?;

    generate(&source)
}

pub fn generate(source: &str) -> Result<TokenStream2, String> {
    let document: toml::Table = source.parse().map_err(|e| {
        format!("failed to parse Cargo.toml: {e}")
    })?;

    let mut output = TokenStream2::new();

    for (key, value) in document {
        let ident = make_ident(key);

        let tokens = generate_value(&ident, value)?;

        output.extend(tokens);
    }

    Ok(quote! {
        #output
    })
}

fn find_manifest(start: &Path) -> Result<PathBuf, String> {
    let mut current = start.to_path_buf();

    loop {
        let candidate = current.join("Cargo.toml");

        if candidate.exists() {
            return Ok(candidate);
        }

        if !current.pop() {
            break;
        }
    }

    Err(format!(
        "could not find Cargo.toml starting from {}",
        start.display()
    ))
}

fn generate_value(
    ident: &Ident,
    value: &toml::Value,
) -> Result<TokenStream2, String> {
    match value {
        toml::Value::String(value) => {
            Ok(quote! {
                pub const #ident: &str = #value;
            })
        }

        toml::Value::Integer(value) => {
            let value = *value;

            Ok(quote! {
                pub const #ident: i64 = #value;
            })
        }

        toml::Value::Float(value) => {
            let value = *value;

            Ok(quote! {
                pub const #ident: f64 = #value;
            })
        }

        toml::Value::Boolean(value) => {
            let value = *value;

            Ok(quote! {
                pub const #ident: bool = #value;
            })
        }

        toml::Value::Datetime(value) => {
            let value = value.to_string();

            Ok(quote! {
                pub const #ident: &str = #value;
            })
        }

        toml::Value::Array(values) => {
            generate_array(ident, values)
        }

        toml::Value::Table(table) => {
            generate_table(ident, table)
        }
    }
}

fn generate_table(
    ident: &Ident,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<TokenStream2, String> {
    let mut output = TokenStream2::new();

    let mut items = Vec::new();

    for (key, value) in table {
        let child_ident = make_ident(key);

        match value {
            toml::Value::Table(child_table) => {
                let child = generate_table(
                    &child_ident,
                    child_table,
                )?;

                output.extend(child);
            }

            _ => {
                let child = generate_value(
                    &child_ident,
                    value,
                )?;

                items.push(child);
            }
        }
    }

    output.extend(items);

    Ok(quote! {
        pub mod #ident {
            #output
        }
    })
}

fn generate_array(
    ident: &Ident,
    values: &[toml::Value],
) -> Result<TokenStream2, String> {
    if values.is_empty() {
        return Ok(quote! {
            pub const #ident: &[()] = &[];
        });
    }

    if values.iter().all(|v| matches!(v, toml::Value::String(_))) {
        let values = values
            .iter()
            .map(|value| {
                let value = value
                    .as_str()
                    .expect("checked above");

                quote! { #value }
            });

        return Ok(quote! {
            pub const #ident: &[&str] = &[
                #(#values),*
            ];
        });
    }

    if values.iter().all(|v| matches!(v, toml::Value::Integer(_))) {
        let values = values
            .iter()
            .map(|value| {
                let value = value
                    .as_integer()
                    .expect("checked above");

                quote! { #value }
            });

        return Ok(quote! {
            pub const #ident: &[i64] = &[
                #(#values),*
            ];
        });
    }

    if values.iter().all(|v| matches!(v, toml::Value::Boolean(_))) {
        let values = values
            .iter()
            .map(|value| {
                let value = value
                    .as_bool()
                    .expect("checked above");

                quote! { #value }
            });

        return Ok(quote! {
            pub const #ident: &[bool] = &[
                #(#values),*
            ];
        });
    }

    if values.iter().all(|v| matches!(v, toml::Value::Float(_))) {
        let values = values
            .iter()
            .map(|value| {
                let value = value
                    .as_float()
                    .expect("checked above");

                quote! { #value }
            });

        return Ok(quote! {
            pub const #ident: &[f64] = &[
                #(#values),*
            ];
        });
    }

    Err(format!(
        "unsupported mixed TOML array for `{ident}`"
    ))
}

fn make_ident(name: &str) -> Ident {
    let mut result = String::with_capacity(name.len());

    for (index, character) in name.chars().enumerate() {
        if index == 0 {
            if character.is_ascii_digit() {
                result.push('_');
            }
        }

        if character.is_ascii_alphanumeric() || character == '_' {
            result.push(character.to_ascii_uppercase());
        } else {
            result.push('_');
        }
    }

    if result.is_empty() {
        result.push('_');
    }

    // Rust keywords müssen escaped werden.
    if is_keyword(&result.to_ascii_lowercase()) {
        Ident::new_raw(
            &result.to_ascii_lowercase(),
            Span::call_site(),
        )
    } else {
        Ident::new(
            &result,
            Span::call_site(),
        )
    }
}

fn is_keyword(value: &str) -> bool {
    matches!(
        value,
        "as"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_generated_rust() {
        let cargo_toml = r#"[package]
name = "hello-world"
version = "1.2.3"
edition = "2024"
description = "Test application"

[package.metadata.my_app]
foo = "bar"
enabled = true
retries = 3

[features]
default = ["foo"]
foo = ["serde"]
bar = []

[dependencies]
serde = "1"
tokio = { version = "1", features = ["full"] }"#;

        let tokens = generate(cargo_toml.trim()).unwrap();

        println!("\n========== GENERATED ==========\n");
        println!("{}", tokens);
        println!("\n===============================\n");
    }
}
