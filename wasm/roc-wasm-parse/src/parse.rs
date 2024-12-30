use bumpalo::Bump;
use roc_parse::ast::SpacesBefore;
use roc_parse::parser::SyntaxError;
use roc_parse::{
    ast::{Defs, Header},
    header::parse_module_defs,
    parser::Parser,
    state::State,
};

#[derive(Debug)]
#[allow(dead_code)]
pub struct Full<'a> {
    pub header: SpacesBefore<'a, Header<'a>>,
    pub defs: Defs<'a>,
}

pub fn parse_module<'a>(input: &'a str, arena: &'a Bump) -> Result<Full<'a>, SyntaxError<'a>> {
    let state = State::new(input.as_bytes());

    let min_indent = 0;
    let (_, header, state) = roc_parse::header::header()
        .parse(arena, state.clone(), min_indent)
        .map_err(|(_, fail)| SyntaxError::Header(fail))?;

    let (new_header, defs) = header.item.upgrade_header_imports(arena);
    let header = SpacesBefore {
        before: header.before,
        item: new_header,
    };

    let defs = parse_module_defs(arena, state, defs)?;

    Ok(Full { header, defs })
}
