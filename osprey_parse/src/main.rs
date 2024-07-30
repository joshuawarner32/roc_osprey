use bumpalo::Bump;
use roc_fmt::annotation::Formattable;
use roc_fmt::module::fmt_module;
use roc_fmt::Buf;
use roc_parse::{
    ast::Defs, module::parse_module_defs, parser::Parser, remove_spaces::RemoveSpaces, state::State,
};
use rusqlite::OptionalExtension;
use rusqlite::{params, Connection, Result};

struct ParseData {
    output: Option<String>,
    error: Option<String>,

    fmt_output: Option<String>,

    reparse_output: Option<String>,
    reparse_error: Option<String>,

    normalized_output: Option<String>,
    normalized_reparse_output: Option<String>,

    double_fmt_output: Option<String>,

    fmt_changed: Option<String>,
    fmt_changed_syntax: Option<bool>,
    fmt_idempotent: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug)]
struct Output<'a> {
    header: roc_parse::ast::Module<'a>,
    module_defs: Defs<'a>,
}

impl<'a> RemoveSpaces<'a> for Output<'a> {
    fn remove_spaces(&self, arena: &'a Bump) -> Self {
        Output {
            header: self.header.remove_spaces(arena),
            module_defs: self.module_defs.remove_spaces(arena),
        }
    }
}

fn parse_module<'a>(input: &'a str, arena: &'a Bump) -> Result<Output<'a>, String> {
    let state = State::new(input.as_bytes());
    let min_indent = 0;
    let (_, header, state) = roc_parse::module::header()
        .parse(arena, state.clone(), min_indent)
        .map_err(|e| format!("{:?}", e))?;

    let (header, defs) = header.upgrade_header_imports(arena);
    let module_defs = parse_module_defs(arena, state, defs).map_err(|e| format!("{:?}", e))?;

    Ok(Output {
        header,
        module_defs,
    })
}

fn format_module(output: &Output) -> String {
    let arena = Bump::new();
    let mut buf = Buf::new_in(&arena);
    fmt_module(&mut buf, &output.header);
    output.module_defs.format(&mut buf, 0);
    buf.fmt_end_of_file();
    buf.as_str().to_string()
}

fn parse_one(input: &str) -> ParseData {
    let mut result = ParseData {
        output: None,
        error: None,
        fmt_output: None,
        reparse_output: None,
        reparse_error: None,

        normalized_output: None,
        normalized_reparse_output: None,

        double_fmt_output: None,

        fmt_changed: None,
        fmt_changed_syntax: None,
        fmt_idempotent: None,
    };

    let arena = bumpalo::Bump::new();
    let output = match parse_module(input, &arena) {
        Ok(o) => o,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };

    result.output = Some(format!("{:#?}", output));

    let formatted = format_module(&output);

    result.fmt_output = Some(formatted.clone());
    result.fmt_changed = Some(format!("{:#?}", formatted != input));

    let reparsed_output = match parse_module(formatted.as_str(), &arena) {
        Ok(o) => o,
        Err(e) => {
            result.reparse_error = Some(e);
            return result;
        }
    };

    result.reparse_output = Some(format!("{:#?}", reparsed_output));

    let output_normalized = output.remove_spaces(&arena);
    let reparsed_output_normalized = reparsed_output.remove_spaces(&arena);

    result.normalized_output = Some(format!("{:#?}", output_normalized));
    result.normalized_reparse_output = Some(format!("{:#?}", reparsed_output_normalized));

    result.fmt_changed_syntax = Some(result.normalized_output != result.normalized_reparse_output);

    let double_formatted = format_module(&reparsed_output);

    result.fmt_idempotent = Some(formatted == double_formatted);
    result.double_fmt_output = Some(double_formatted);

    result
}

use structopt::StructOpt;
#[derive(StructOpt, Debug)]
#[structopt(name = "roc_parser")]
enum Opt {
    #[structopt(name = "parse")]
    Parse {
        #[structopt(short, long)]
        corpus_db: String,
        #[structopt(short, long)]
        results_db: String,
    },
    #[structopt(name = "diff")]
    Diff {
        #[structopt(short, long)]
        corpus_db: String,
        #[structopt(short = "a", long)]
        results_db_a: String,
        #[structopt(short = "b", long)]
        results_db_b: String,
    },
}

fn main() -> Result<()> {
    let opt = Opt::from_args();

    match opt {
        Opt::Parse {
            corpus_db,
            results_db,
        } => {
            let conn_corpus = Connection::open(corpus_db)?;
            let mut conn_results = Connection::open(results_db)?;

            // Ensure the output table exists
            conn_results.execute(
                "CREATE TABLE IF NOT EXISTS roc_parse_results (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    contents TEXT,
                    output TEXT,
                    error TEXT,
                    fmt_output TEXT,
                    reparse_output TEXT,
                    reparse_error TEXT,
                    normalized_output TEXT,
                    normalized_reparse_output TEXT,
                    double_fmt_output TEXT,
                    fmt_changed TEXT,
                    fmt_changed_syntax BOOL,
                    fmt_idempotent BOOL
                )",
                [],
            )?;

            let mut stmt = conn_corpus.prepare(
                "SELECT repo_url, file_path, file_contents FROM roc_files where repo_url not like '%/roc'",
            )?;
            let file_contents_iter = stmt.query_map([], |row| {
                let repo_url = row.get::<_, String>(0)?;
                let file_path = row.get::<_, String>(1)?;
                let file_contents = row.get::<_, String>(2)?;

                Ok((repo_url, file_path, file_contents))
            })?;

            let transaction = conn_results.transaction()?;

            for row in file_contents_iter {
                let (repo_url, file_path, file_content) = row?;
                println!("Parsing file: {} {}", repo_url, file_path);
                let result: ParseData = parse_one(&file_content);

                transaction.execute(
                    "INSERT INTO roc_parse_results (
                        contents, output, error, fmt_output, reparse_output, reparse_error,
                        normalized_output, normalized_reparse_output, double_fmt_output,
                        fmt_changed, fmt_changed_syntax, fmt_idempotent
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        file_content,
                        result.output,
                        result.error,
                        result.fmt_output,
                        result.reparse_output,
                        result.reparse_error,
                        result.normalized_output,
                        result.normalized_reparse_output,
                        result.double_fmt_output,
                        result.fmt_changed,
                        result.fmt_changed_syntax,
                        result.fmt_idempotent
                    ],
                )?;
            }

            transaction.commit()?;
        }
        Opt::Diff {
            corpus_db,
            results_db_a,
            results_db_b,
        } => {
            let conn_corpus = Connection::open(corpus_db)?;
            let conn_results_a = Connection::open(results_db_a)?;
            let conn_results_b = Connection::open(results_db_b)?;

            let mut stmt = conn_corpus.prepare(
                "SELECT repo_url, file_path FROM roc_files where repo_url not like '%/roc'",
            )?;
            let file_paths_iter = stmt.query_map([], |row| {
                let repo_url = row.get::<_, String>(0)?;
                let file_path = row.get::<_, String>(1)?;

                Ok((repo_url, file_path))
            })?;

            for row in file_paths_iter {
                let (repo_url, file_path) = row?;
                println!("Diffing file: {} {}", repo_url, file_path);

                let result_a: Option<ParseData> = conn_results_a
                    .query_row(
                        "SELECT contents, output, error, fmt_output, reparse_output, reparse_error,
                        normalized_output, normalized_reparse_output, double_fmt_output,
                        fmt_changed, fmt_changed_syntax, fmt_idempotent
                     FROM roc_parse_results
                     WHERE contents = ?1",
                        params![repo_url.clone() + &file_path],
                        |row| {
                            Ok(ParseData {
                                output: row.get(1)?,
                                error: row.get(2)?,
                                fmt_output: row.get(3)?,
                                reparse_output: row.get(4)?,
                                reparse_error: row.get(5)?,
                                normalized_output: row.get(6)?,
                                normalized_reparse_output: row.get(7)?,
                                double_fmt_output: row.get(8)?,
                                fmt_changed: row.get(9)?,
                                fmt_changed_syntax: row.get(10)?,
                                fmt_idempotent: row.get(11)?,
                            })
                        },
                    )
                    .optional()?;

                let result_b: Option<ParseData> = conn_results_b
                    .query_row(
                        "SELECT contents, output, error, fmt_output, reparse_output, reparse_error,
                        normalized_output, normalized_reparse_output, double_fmt_output,
                        fmt_changed, fmt_changed_syntax, fmt_idempotent
                     FROM roc_parse_results
                     WHERE contents = ?1",
                        params![repo_url.clone() + &file_path],
                        |row| {
                            Ok(ParseData {
                                output: row.get(1)?,
                                error: row.get(2)?,
                                fmt_output: row.get(3)?,
                                reparse_output: row.get(4)?,
                                reparse_error: row.get(5)?,
                                normalized_output: row.get(6)?,
                                normalized_reparse_output: row.get(7)?,
                                double_fmt_output: row.get(8)?,
                                fmt_changed: row.get(9)?,
                                fmt_changed_syntax: row.get(10)?,
                                fmt_idempotent: row.get(11)?,
                            })
                        },
                    )
                    .optional()?;

                match (result_a, result_b) {
                    (Some(a), Some(b)) => {
                        if a.output != b.output
                            || a.error != b.error
                            || a.fmt_output != b.fmt_output
                            || a.reparse_output != b.reparse_output
                            || a.reparse_error != b.reparse_error
                            || a.normalized_output != b.normalized_output
                            || a.normalized_reparse_output != b.normalized_reparse_output
                            || a.double_fmt_output != b.double_fmt_output
                            || a.fmt_changed != b.fmt_changed
                            || a.fmt_changed_syntax != b.fmt_changed_syntax
                            || a.fmt_idempotent != b.fmt_idempotent
                        {
                            println!("Differences found for file: {} {}", repo_url, file_path);
                        }
                    }
                    _ => println!("No results found for file: {} {}", repo_url, file_path),
                }
            }
        }
    }

    Ok(())
}
