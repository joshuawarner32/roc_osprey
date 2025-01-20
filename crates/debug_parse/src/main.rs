use bumpalo::collections::Vec as BumpVec;
use bumpalo::Bump;
use std::str;

#[derive(Debug)]
pub struct ParseError<E> {
    pub kind: E,
    pub offset: usize,
}

pub struct Parser<'a> {
    text: &'a [u8],
    offset: usize,
    bump: &'a Bump,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str, bump: &'a Bump) -> Self {
        Parser {
            text: input.as_bytes(),
            offset: 0,
            bump,
        }
    }

    pub fn bump(&self) -> &Bump {
        self.bump
    }

    pub fn check_u8(&mut self, next: u8) -> bool {
        if self.offset < self.text.len() && self.text[self.offset] == next {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    pub fn expect_u8<E>(&mut self, next: u8, err: E) -> Result<(), ParseError<E>> {
        if self.check_u8(next) {
            Ok(())
        } else {
            Err(ParseError {
                kind: err,
                offset: self.offset,
            })
        }
    }

    pub fn peek_u8(&self, next: u8) -> bool {
        self.offset < self.text.len() && self.text[self.offset] == next
    }

    pub fn check_str(&mut self, next: &str) -> bool {
        if self.text[self.offset..].starts_with(next.as_bytes()) {
            self.offset += next.len();
            true
        } else {
            false
        }
    }

    pub fn expect_str<E>(&mut self, next: &str, err: E) -> Result<(), ParseError<E>> {
        if self.check_str(next) {
            Ok(())
        } else {
            Err(ParseError {
                kind: err,
                offset: self.offset,
            })
        }
    }

    pub fn check_ident(&mut self) -> Option<&'a str> {
        let mut offset = self.offset;
        while offset < self.text.len() {
            let c = self.text[offset];
            if c.is_ascii_alphanumeric() || c == b'_' {
                offset += 1;
            } else {
                break;
            }
        }

        if offset > self.offset {
            let res = if cfg!(debug_assertions) {
                Some(str::from_utf8(&self.text[self.offset..offset]).unwrap())
            } else {
                Some(unsafe { str::from_utf8_unchecked(&self.text[self.offset..offset]) })
            };
            self.offset = offset;
            res
        } else {
            None
        }
    }

    pub fn at_terminator(&self) -> bool {
        self.peek_u8(b',')
            || self.peek_u8(b')')
            || self.peek_u8(b']')
            || self.peek_u8(b'}')
            || self.is_eof()
    }

    pub fn expect_ident<E>(&mut self, err: E) -> Result<&'a str, ParseError<E>> {
        self.check_ident().ok_or_else(|| ParseError {
            kind: err,
            offset: self.offset,
        })
    }

    pub fn check_path_ident(&mut self) -> Option<&'a str> {
        // Parse a path identifier, e.g. std::collections::HashMap
        let mut offset = self.offset;
        while offset < self.text.len() {
            let c = self.text[offset];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b':' {
                offset += 1;
            } else {
                break;
            }
        }

        if offset > self.offset {
            let res = if cfg!(debug_assertions) {
                Some(str::from_utf8(&self.text[self.offset..offset]).unwrap())
            } else {
                Some(unsafe { str::from_utf8_unchecked(&self.text[self.offset..offset]) })
            };
            self.offset = offset;
            res
        } else {
            None
        }
    }

    pub fn expect_path_ident<E>(&mut self, err: E) -> Result<&'a str, ParseError<E>> {
        self.check_path_ident().ok_or_else(|| ParseError {
            kind: err,
            offset: self.offset,
        })
    }

    pub fn check_int(&mut self) -> Option<&'a str> {
        let mut offset = self.offset;
        while offset < self.text.len() {
            let c = self.text[offset];
            if c.is_ascii_digit() {
                offset += 1;
            } else {
                break;
            }
        }

        if offset > self.offset {
            let res = if cfg!(debug_assertions) {
                Some(str::from_utf8(&self.text[self.offset..offset]).unwrap())
            } else {
                Some(unsafe { str::from_utf8_unchecked(&self.text[self.offset..offset]) })
            };
            self.offset = offset;
            res
        } else {
            None
        }
    }

    pub fn expect_int<E>(&mut self, err: E) -> Result<&'a str, ParseError<E>> {
        self.check_int().ok_or_else(|| ParseError {
            kind: err,
            offset: self.offset,
        })
    }

    pub fn check_delimited_seq<T, E>(
        &mut self,
        begin: u8,
        end: u8,
        err: E,
        func: impl Fn(&mut Self) -> Result<T, ParseError<E>>,
    ) -> Result<Option<BumpVec<'a, T>>, ParseError<E>> {
        if self.check_u8(begin) {
            // Parse a tuple
            let mut elements = BumpVec::new_in(self.bump);
            self.consume_ws();
            while !self.check_u8(end) {
                elements.push(func(self)?);
                self.consume_ws();
                if self.check_u8(b',') {
                    self.consume_ws();
                    if self.check_u8(end) {
                        break;
                    } else {
                        continue;
                    }
                } else if !self.peek_u8(end) {
                    return Err(ParseError {
                        kind: err,
                        offset: self.offset,
                    });
                }
            }
            Ok(Some(elements))
        } else {
            Ok(None)
        }
    }

    pub fn consume_ws(&mut self) {
        while self.offset < self.text.len() {
            if self.text[self.offset].is_ascii_whitespace() {
                self.offset += 1;
            } else {
                break;
            }
        }
    }

    pub fn current_offset(&self) -> usize {
        self.offset
    }

    pub fn is_eof(&self) -> bool {
        self.offset >= self.text.len()
    }
}

#[derive(Debug)]
pub struct Generics<'a> {
    pub params: &'a [&'a str],
}

#[derive(Debug)]
pub enum Fields<'a> {
    Unit,
    Tuple(&'a [DebugNode<'a>]),
    Struct(&'a [(&'a str, DebugNode<'a>)]),
}

#[derive(Debug)]
pub enum DebugNode<'a> {
    Ellipsis,
    Int(&'a str),
    Str(&'a str),
    Tuple(&'a [DebugNode<'a>]),
    List(&'a [DebugNode<'a>]),
    Struct(&'a str, Option<&'a Generics<'a>>, Fields<'a>),
    Region(usize, usize),
    Position(usize),
    Loc(usize, usize, &'a DebugNode<'a>),
}

impl<'a> Parser<'a> {
    pub fn parse_debug_node(&mut self) -> Result<DebugNode<'a>, ParseError<&'static str>> {
        self.consume_ws();

        if self.check_u8(b'"') {
            // Parse a string literal
            let start = self.offset;
            while self.offset < self.text.len() {
                if self.text[self.offset] == b'\\' {
                    if self.offset + 1 < self.text.len() {
                        if self.text[self.offset + 1] == b'"' {
                            self.offset += 2; // Skip the escaped quote
                        } else if self.text[self.offset + 1] == b'\\' {
                            self.offset += 2; // Skip the escaped backslash
                        } else {
                            self.offset += 1; // Skip the backslash
                        }
                    } else {
                        return Err(ParseError {
                            kind: "Unterminated string escape",
                            offset: self.offset,
                        });
                    }
                } else if self.text[self.offset] == b'"' {
                    let result =
                        unsafe { str::from_utf8_unchecked(&self.text[start..self.offset]) };
                    self.offset += 1;
                    return Ok(DebugNode::Str(result));
                } else {
                    self.offset += 1;
                }
            }
            Err(ParseError {
                kind: "Unterminated string literal",
                offset: self.offset,
            })
        } else if self.check_str("…") {
            Ok(DebugNode::Ellipsis)
        } else if self.peek_u8(b'-') {
            let start = self.offset;
            self.offset += 1;
            let _ = self.expect_int("Expected integer")?;
            Ok(DebugNode::Int(unsafe {
                str::from_utf8_unchecked(&self.text[start..self.offset])
            }))
        } else if self.check_u8(b'@') {
            // Parse a Loc - @start-end <node>
            let start = self.check_int().ok_or_else(|| ParseError {
                kind: "Expected integer",
                offset: self.offset,
            })?;
            if self.check_u8(b'-') {
                let end = self.check_int().ok_or_else(|| ParseError {
                    kind: "Expected integer",
                    offset: self.offset,
                })?;
                self.consume_ws();
                if self.at_terminator() {
                    Ok(DebugNode::Region(
                        start.parse().unwrap(),
                        end.parse().unwrap(),
                    ))
                } else {
                    let node = self.parse_debug_node()?;
                    Ok(DebugNode::Loc(
                        start.parse().unwrap(),
                        end.parse().unwrap(),
                        self.bump.alloc(node),
                    ))
                }
            } else {
                Ok(DebugNode::Position(start.parse().unwrap()))
            }
        } else if let Some(val) = self.check_int() {
            Ok(DebugNode::Int(val))
        } else if let Some(items) =
            self.check_delimited_seq(b'(', b')', "Expected ',' or ')'", |p| p.parse_debug_node())?
        {
            Ok(DebugNode::Tuple(items.into_bump_slice()))
        } else if let Some(items) =
            self.check_delimited_seq(b'[', b']', "Expected ',' or ']'", |p| p.parse_debug_node())?
        {
            Ok(DebugNode::List(items.into_bump_slice()))
        } else if let Some(name) = self.check_ident() {
            self.consume_ws();

            let generics = if let Some(items) =
                self.check_delimited_seq(b'<', b'>', "Expected ',' or '>'", |p| {
                    p.expect_path_ident("Expected identifier")
                })? {
                self.consume_ws();
                Some(&*self.bump.alloc(Generics {
                    params: items.into_bump_slice(),
                }))
            } else {
                None
            };

            let fields = if let Some(elements) =
                self.check_delimited_seq(b'(', b')', "Expected ',' or ')'", |p| {
                    p.parse_debug_node()
                })? {
                Fields::Tuple(elements.into_bump_slice())
            } else if let Some(fields) =
                self.check_delimited_seq(b'{', b'}', "Expected ',' or '}'", |p| {
                    let key = p.expect_ident("Expected identifier")?;
                    p.expect_u8(b':', "Expected ':'")?;
                    let value = p.parse_debug_node()?;
                    Ok((key, value))
                })?
            {
                Fields::Struct(fields.into_bump_slice())
            } else {
                Fields::Unit
            };
            Ok(DebugNode::Struct(name, generics, fields))
        } else {
            Err(ParseError {
                kind: "Expected string, integer, tuple, tuple struct, or struct",
                offset: self.offset,
            })
        }
    }
}

fn main() {
    use annotate_snippets::{Level, Renderer, Snippet};

    use std::fs;
    use std::path::Path;

    let dir_path =
        "/Users/joshw/src/github.com/roc-lang/roc/crates/compiler/test_syntax/tests/snapshots";
    let extension = "result-ast";

    for subddir in &["pass", "fail", "malformed"] {
        let mut dir_path = Path::new(dir_path).to_path_buf();
        dir_path.push(subddir);
        if let Ok(entries) = fs::read_dir(dir_path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
                        if let Ok(content) = fs::read_to_string(&path) {
                            println!("Loaded file: {:?}", path);
                            println!("Content: {}", content);

                            let bump = Bump::new();
                            let input = &content;
                            let mut parser = Parser::new(input, &bump);

                            match parser.parse_debug_node() {
                                Ok(node) => println!("Parsed debug node: {:?}", node),
                                Err(err) => {
                                    let mut source = input.to_string();
                                    if err.offset == input.len() {
                                        source.push(' ');
                                    }
                                    let source = &source;
                                    let path = path.to_string_lossy();
                                    let message = annotate_snippets::Level::Error
                                        .title("Parse error")
                                        .snippet(
                                            annotate_snippets::Snippet::source(source)
                                                .line_start(1)
                                                .origin(path.as_ref())
                                                .fold(true)
                                                .annotation(
                                                    annotate_snippets::Level::Error
                                                        .span(err.offset..err.offset + 1)
                                                        .label(err.kind),
                                                ),
                                        );

                                    let renderer = annotate_snippets::Renderer::styled();
                                    println!("{}", renderer.render(message));
                                    std::process::exit(1);
                                }
                            }
                        } else {
                            eprintln!("Failed to read file: {:?}", path);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
    }
}
