use bumpalo::Bump;
use roc_parse::module::parse_module_defs;
use roc_parse::parser::Parser;
use roc_parse::state::State;
use roc_parse::{
    ast::{Defs, Expr},
    test_helpers::parse_loc_with,
};
use rusqlite::{Connection, Result};
use scraper::{Html, Selector};

#[allow(dead_code)]
#[derive(Debug)]
struct Module<'a> {
    header: roc_parse::ast::Module<'a>,
    module_defs: Defs<'a>,
}

fn parse_module<'a>(input: &'a str, arena: &'a Bump) -> Result<Module<'a>, String> {
    let state = State::new(input.as_bytes());
    let min_indent = 0;
    let (_, header, state) = roc_parse::module::header()
        .parse(arena, state.clone(), min_indent)
        .map_err(|e| format!("{:?}", e))?;

    let (header, defs) = header.upgrade_header_imports(arena);
    let module_defs = parse_module_defs(arena, state, defs).map_err(|e| format!("{:?}", e))?;

    Ok(Module {
        header,
        module_defs,
    })
}

fn parse_expr<'a>(input: &'a str, arena: &'a Bump) -> Result<Expr<'a>, String> {
    let state = State::new(input.as_bytes());
    let min_indent = 0;
    parse_loc_with(arena, input)
        .map(|loc_expr| loc_expr.value)
        .map_err(|e| format!("{:?}", e.problem))
}

fn parse_roc_code(text: &str) -> Result<(), String> {
    // try to parse the text as a module or an expr
    let arena = Bump::new();
    match parse_module(text, &arena) {
        Ok(_) => Ok(()),
        Err(_) => match parse_expr(text, &arena) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        },
    }
}

// Table schema:
//     conn.execute('''
//     CREATE TABLE IF NOT EXISTS messages (
//         channel TEXT,
//         message_id INTEGER PRIMARY KEY,
//         content TEXT
//     )
// ''')

// Read the zulip_code_blocks.db file and iterate over the messages
fn main() -> Result<()> {
    let conn = Connection::open("../zulip_code_blocks.db")?;
    let mut stmt = conn.prepare("SELECT * FROM messages")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i32>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let code_selector = Selector::parse("code, pre").unwrap();

    for row in rows {
        let (_, _, content) = row?;
        let document = Html::parse_document(&content);
        for code_block in document.select(&code_selector) {
            let text = code_block.text().collect::<Vec<_>>().join("");
            let parsed_code = match parse_roc_code(&text) {
                Ok(()) => println!("{}", text),
                Err(e) => {}
            };
        }
    }
    Ok(())
}
