mod parse;

use wasm_bindgen::prelude::*;

use bumpalo::Bump;

#[wasm_bindgen]
pub fn parse_and_debug(input: &str) -> String {
    let arena = Bump::new();
    match parse::parse_module(input, &arena) {
        Ok(ast) => format!("{:#?}", ast),
        Err(e) => format!("{:?}", e),
    }
}
