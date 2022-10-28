use proc_macro::TokenStream;

fn get_features() -> Vec<&'static str> {
    vec!["printer", "stats", "sqlite"]
}

#[proc_macro]
pub fn invoke_static_modules(stream: TokenStream) -> TokenStream {
    get_features()
        .iter()
        .map(|f| format!("#[cfg(feature = \"{}\")]\n{}_module::{}\n", f, f, stream))
        .fold("".to_string(), |a, b| a + "\n" + &b)
        .parse()
        .expect("Failed to parse generated TokenStream.")
}
