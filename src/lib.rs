use proc_macro::TokenStream;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[proc_macro]
pub fn embed(input: TokenStream) -> TokenStream {
    if !input.is_empty() {
        return syn::Error::new(Span::call_site(), "embed!() does not accept arguments")
            .to_compile_error()
            .into();
    }

    match expand() {
        Ok(tokens) => tokens.into(),
        Err(error) => syn::Error::new(Span::call_site(), error)
            .to_compile_error()
            .into(),
    }
}

fn expand() -> Result<TokenStream2, String> {
    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").map_err(|_| "CARGO_MANIFEST_DIR is not set".to_string())?;

    let manifest_path = find_manifest(Path::new(&manifest_dir))?;

    let source = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("failed to read {}: {e}", manifest_path.display()))?;

    generate(&source)
}

fn generate(source: &str) -> Result<TokenStream2, String> {
    let document: toml::Table = source
        .parse()
        .map_err(|e| format!("failed to parse Cargo.toml: {e}"))?;

    let mut output = TokenStream2::new();

    for (key, value) in document {
        let ident = make_ident(&key);

        let tokens = generate_value(&ident, &value)?;

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

fn generate_value(ident: &Ident, value: &toml::Value) -> Result<TokenStream2, String> {
    match value {
        toml::Value::String(value) => Ok(quote! {
            pub const #ident: &str = #value;
        }),

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

        toml::Value::Array(values) => generate_array(ident, values),

        toml::Value::Table(table) => generate_table(ident, table),
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
                let child = generate_table(&child_ident, child_table)?;

                output.extend(child);
            }

            _ => {
                let child = generate_value(&child_ident, value)?;

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

fn generate_array(ident: &Ident, values: &[toml::Value]) -> Result<TokenStream2, String> {
    if values.is_empty() {
        return Ok(quote! {
            pub const #ident: &[()] = &[];
        });
    }

    // Array<String>
    if values.iter().all(|v| matches!(v, toml::Value::String(_))) {
        let values = values.iter().map(|value| {
            let value = value.as_str().expect("checked above");

            quote! { #value }
        });

        return Ok(quote! {
            pub const #ident: &[&str] = &[
                #(#values),*
            ];
        });
    }

    // Array<Integer>
    if values.iter().all(|v| matches!(v, toml::Value::Integer(_))) {
        let values = values.iter().map(|value| {
            let value = value.as_integer().expect("checked above");

            quote! { #value }
        });

        return Ok(quote! {
            pub const #ident: &[i64] = &[
                #(#values),*
            ];
        });
    }

    // Array<Float>
    if values.iter().all(|v| matches!(v, toml::Value::Float(_))) {
        let values = values.iter().map(|value| {
            let value = value.as_float().expect("checked above");

            quote! { #value }
        });

        return Ok(quote! {
            pub const #ident: &[f64] = &[
                #(#values),*
            ];
        });
    }

    // Array<Boolean>
    if values.iter().all(|v| matches!(v, toml::Value::Boolean(_))) {
        let values = values.iter().map(|value| {
            let value = value.as_bool().expect("checked above");

            quote! { #value }
        });

        return Ok(quote! {
            pub const #ident: &[bool] = &[
                #(#values),*
            ];
        });
    }

    // Array<Datetime>
    if values.iter().all(|v| matches!(v, toml::Value::Datetime(_))) {
        let values = values.iter().map(|value| {
            let value = value.as_datetime().expect("checked above").to_string();

            quote! { #value }
        });

        return Ok(quote! {
            pub const #ident: &[&str] = &[
                #(#values),*
            ];
        });
    }

    // Array<Array>
    if values.iter().all(|v| matches!(v, toml::Value::Array(_))) {
        return generate_nested_array(ident, values);
    }

    // Array<Table>
    if values.iter().all(|v| matches!(v, toml::Value::Table(_))) {
        return generate_table_array(ident, values);
    }

    Err(format!("unsupported TOML array for `{ident}`"))
}

fn generate_table_array(ident: &Ident, values: &[toml::Value]) -> Result<TokenStream2, String> {
    if values.is_empty() {
        return Ok(quote! {
            pub const #ident: &[()] = &[];
        });
    }

    let tables = values
        .iter()
        .map(|value| {
            value
                .as_table()
                .ok_or_else(|| format!("expected TOML table in `{ident}`"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let type_name = Ident::new(&format!("{ident}Item"), ident.span());

    // Felder anhand der ersten Tabelle bestimmen.
    let mut fields = Vec::new();

    for (key, value) in tables[0] {
        let field_ident = make_ident(key);
        let field_type = rust_type(value)?;

        fields.push((key.clone(), field_ident, field_type));
    }

    // Prüfen, dass alle Tabellen dieselben Keys besitzen.
    for table in &tables[1..] {
        for (key, _, _) in &fields {
            if !table.contains_key(key) {
                return Err(format!("missing field `{key}` in table array `{ident}`"));
            }
        }

        if table.len() != fields.len() {
            return Err(format!(
                "all tables in array `{ident}` must have the same fields"
            ));
        }
    }

    let field_defs = fields.iter().map(|(_, field, ty)| {
        quote! {
            pub #field: #ty
        }
    });

    let mut items = Vec::new();

    for table in &tables {
        let mut field_values = Vec::new();

        for (key, field, _) in &fields {
            let value = table
                .get(key)
                .ok_or_else(|| format!("missing field `{key}` in table array `{ident}`"))?;

            let tokens = rust_value_tokens(value)?;

            field_values.push(quote! {
                #field: #tokens
            });
        }

        items.push(quote! {
            #type_name {
                #(#field_values),*
            }
        });
    }

    Ok(quote! {
        #[derive(Debug, Clone, Copy)]
        pub struct #type_name {
            #(#field_defs),*
        }

        pub const #ident: &[#type_name] = &[
            #(#items),*
        ];
    })
}

fn rust_type(value: &toml::Value) -> Result<TokenStream2, String> {
    match value {
        toml::Value::String(_) => Ok(quote! { &'static str }),

        toml::Value::Integer(_) => Ok(quote! { i64 }),

        toml::Value::Float(_) => Ok(quote! { f64 }),

        toml::Value::Boolean(_) => Ok(quote! { bool }),

        toml::Value::Datetime(_) => Ok(quote! { &'static str }),

        toml::Value::Array(_) => {
            Err("arrays inside table arrays are not supported yet".to_string())
        }

        toml::Value::Table(_) => {
            Err("nested tables inside table arrays are not supported yet".to_string())
        }
    }
}

fn rust_value_tokens(value: &toml::Value) -> Result<TokenStream2, String> {
    match value {
        toml::Value::String(value) => Ok(quote! { #value }),

        toml::Value::Integer(value) => {
            let value = *value;
            Ok(quote! { #value })
        }

        toml::Value::Float(value) => {
            let value = *value;
            Ok(quote! { #value })
        }

        toml::Value::Boolean(value) => {
            let value = *value;
            Ok(quote! { #value })
        }

        toml::Value::Datetime(value) => {
            let value = value.to_string();
            Ok(quote! { #value })
        }

        toml::Value::Array(_) | toml::Value::Table(_) => {
            Err("unsupported value in table array".to_string())
        }
    }
}

fn generate_nested_array(ident: &Ident, values: &[toml::Value]) -> Result<TokenStream2, String> {
    let arrays = values
        .iter()
        .map(|value| {
            let values = value.as_array().expect("checked above");

            generate_array(ident, values)
        })
        .collect::<Result<Vec<_>, _>>()?;

    if arrays.is_empty() {
        return Ok(quote! {
            pub const #ident: &[()] = &[];
        });
    }

    // Wir brauchen hier den Elementtyp.
    // Für die typischen primitiven Fälle können wir ihn direkt
    // aus dem ersten Unterarray bestimmen.
    let first = values[0].as_array().expect("checked above");

    if first.is_empty() {
        return Ok(quote! {
            pub const #ident: &[&[()]] = &[];
        });
    }

    let element_type = rust_array_type(&first[0])?;

    let rows = values
        .iter()
        .map(|value| {
            let array = value.as_array().expect("checked above");

            let elements = array.iter().map(|value| primitive_tokens(value));

            Ok::<_, String>(quote! {
                &[#(#elements),*]
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(quote! {
        pub const #ident: &[&[#element_type]] = &[
            #(#rows),*
        ];
    })
}

fn rust_array_type(value: &toml::Value) -> Result<TokenStream2, String> {
    match value {
        toml::Value::String(_) => Ok(quote! { &str }),
        toml::Value::Integer(_) => Ok(quote! { i64 }),
        toml::Value::Float(_) => Ok(quote! { f64 }),
        toml::Value::Boolean(_) => Ok(quote! { bool }),
        toml::Value::Datetime(_) => Ok(quote! { &str }),

        _ => Err("nested arrays currently only support primitive values".to_string()),
    }
}

fn primitive_tokens(value: &toml::Value) -> TokenStream2 {
    match value {
        toml::Value::String(value) => {
            quote! { #value }
        }

        toml::Value::Integer(value) => {
            let value = *value;
            quote! { #value }
        }

        toml::Value::Float(value) => {
            let value = *value;
            quote! { #value }
        }

        toml::Value::Boolean(value) => {
            let value = *value;
            quote! { #value }
        }

        toml::Value::Datetime(value) => {
            let value = value.to_string();
            quote! { #value }
        }

        _ => unreachable!("primitive_tokens called for non-primitive"),
    }
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
        Ident::new_raw(&result.to_ascii_lowercase(), Span::call_site())
    } else {
        Ident::new(&result, Span::call_site())
    }
}

fn is_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "break"
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

        let tokens = generate(cargo_toml).unwrap();

        println!("\n========== GENERATED ==========\n");
        println!("{}", tokens);
        println!("\n===============================\n");
    }
}
