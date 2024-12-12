use bumpalo::collections::Vec;
use bumpalo::Bump;
use roc_parse::ast::AbilityImpls;
use roc_parse::ast::AbilityMember;
use roc_parse::ast::AssignedField;
use roc_parse::ast::Collection;
use roc_parse::ast::CommentOrNewline;
use roc_parse::ast::Defs;
use roc_parse::ast::Expr;
use roc_parse::ast::FullAst;
use roc_parse::ast::Header;
use roc_parse::ast::Implements;
use roc_parse::ast::ImplementsAbilities;
use roc_parse::ast::ImplementsAbility;
use roc_parse::ast::ImplementsClause;
use roc_parse::ast::ImportAlias;
use roc_parse::ast::ImportAsKeyword;
use roc_parse::ast::ImportExposingKeyword;
use roc_parse::ast::ImportedModuleName;
use roc_parse::ast::IngestedFileAnnotation;
use roc_parse::ast::IngestedFileImport;
use roc_parse::ast::ModuleImport;
use roc_parse::ast::ModuleImportParams;
use roc_parse::ast::Pattern;
use roc_parse::ast::PatternAs;
use roc_parse::ast::PrecedenceConflict;
use roc_parse::ast::Spaced;
use roc_parse::ast::Spaces;
use roc_parse::ast::SpacesBefore;
use roc_parse::ast::StrLiteral;
use roc_parse::ast::StrSegment;
use roc_parse::ast::Tag;
use roc_parse::ast::TypeAnnotation;
use roc_parse::ast::TypeDef;
use roc_parse::ast::TypeHeader;
use roc_parse::ast::ValueDef;
use roc_parse::ast::WhenBranch;
use roc_parse::ast::WhenPattern;
use roc_parse::header::parse_module_defs;
use roc_parse::header::AppHeader;
use roc_parse::header::ExposedName;
use roc_parse::header::ExposesKeyword;
use roc_parse::header::HostedHeader;
use roc_parse::header::ImportsEntry;
use roc_parse::header::ImportsKeyword;
use roc_parse::header::KeywordItem;
use roc_parse::header::ModuleHeader;
use roc_parse::header::ModuleName;
use roc_parse::header::ModuleParams;
use roc_parse::header::PackageEntry;
use roc_parse::header::PackageHeader;
use roc_parse::header::PackageKeyword;
use roc_parse::header::PackageName;
use roc_parse::header::PackagesKeyword;
use roc_parse::header::PlatformHeader;
use roc_parse::header::PlatformKeyword;
use roc_parse::header::PlatformRequires;
use roc_parse::header::ProvidesKeyword;
use roc_parse::header::ProvidesTo;
use roc_parse::header::RequiresKeyword;
use roc_parse::header::To;
use roc_parse::header::ToKeyword;
use roc_parse::header::TypedIdent;
use roc_parse::ident::UppercaseIdent;
use roc_parse::parser::Parser;
use roc_parse::parser::SyntaxError;
use roc_parse::state::State;
use roc_region::all::Loc;
use roc_region::all::Region;
use test_syntax::test_helpers::Output;

#[derive(Copy, Clone, Debug)]
enum Space<'a> {
    Newline,
    Comment(&'a str),
    DocComment(&'a str),
}

#[derive(Copy, Clone)]
enum Node<'a> {
    // If there's a string literal we hit when recursing, fall back to Text:
    Str(&'a str),

    // Debug-formatted numbers (u32, i128, f64, etc.)
    Number(&'a str),

    // Debug-formatted enums
    DebugFormatted(&'a str),

    // Bools!
    Bool(bool),

    Spaces(&'a [Space<'a>]),

    // Any time there's a Loc<Whatever>, that should become a Loc(loc.region, loc.value.nodify(arena)):
    Loc(Region, &'a Node<'a>),

    Region(Region),

    // For &[Whatever]
    Array(&'a [Node<'a>]),

    // For (A, B, C)
    Tuple(&'a [Node<'a>]),

    // For Foo(Baz, Qux)
    NamedTuple {
        name: &'static str,
        children: &'a [Node<'a>],
    },

    // For Foo { bar: Baz, qux: Qux }
    NamedStruct {
        name: &'static str,
        fields: &'a [(&'static str, Node<'a>)],
    },
    NamedEmpty {
        name: &'static str,
    },
}

impl<'a> std::fmt::Debug for Node<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Node::Str(s) => s.fmt(f),
            Node::Number(n) => n.fmt(f)
            Node::DebugFormatted(d) => write!(f, "{}", d),
            Node::Bool(b) => b.fmt(f),
            Node::Spaces(spaces) => spaces.fmt(f),
            Node::Loc(region, node) => Loc::at(*region, node).fmt(f),
            Node::Region(region) => region.fmt(f),
            Node::Array(nodes) => f.debug_list().entries(nodes.iter()).finish(),
            Node::Tuple(nodes) => {
                let mut d = f.debug_tuple("");
                for n in *nodes {
                    d.field(n);
                }
                d.finish()
            },
            Node::NamedTuple { name, children } => f.debug_struct(name).field("children", children).finish(),
            Node::NamedStruct { name, fields } => {
                let mut debug_struct = f.debug_struct(name);
                for (field_name, field_value) in *fields {
                    debug_struct.field(field_name, field_value);
                }
                debug_struct.finish()
            }
            Node::NamedEmpty { name } => f.debug_struct(name).finish(),
        }
    }
}

trait Nodify {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a>;
}

impl<T: Nodify> Nodify for &T {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        (*self).nodify(arena)
    }
}

// pub enum Expr<'a> {
//     // Number Literals
//     Num(&'a str),
//     NonBase10Int {
//         string: &'a str,
//         base: Base,
//         is_negative: bool,
//     },
//     Float(&'a str),

//     /// String Literals
//     Str(StrLiteral<'a>), // string without escapes in it
//     /// eg 'b'
//     SingleQuote(&'a str),

//     /// Look up exactly one field on a record, e.g. `x.foo`.
//     RecordAccess(&'a Expr<'a>, &'a str),

//     /// e.g. `.foo` or `.0`
//     AccessorFunction(Accessor<'a>),

//     /// Update the value of a field in a record, e.g. `&foo`
//     RecordUpdater(&'a str),

//     /// Look up exactly one field on a tuple, e.g. `(x, y).1`.
//     TupleAccess(&'a Expr<'a>, &'a str),

//     /// Early return on failures - e.g. the ! in `File.readUtf8! path`
//     TrySuffix {
//         target: TryTarget,
//         expr: &'a Expr<'a>,
//     },

//     // Collection Literals
//     List(Collection<'a, &'a Loc<Expr<'a>>>),

//     RecordUpdate {
//         update: &'a Loc<Expr<'a>>,
//         fields: Collection<'a, Loc<AssignedField<'a, Expr<'a>>>>,
//     },

//     Record(Collection<'a, Loc<AssignedField<'a, Expr<'a>>>>),

//     Tuple(Collection<'a, &'a Loc<Expr<'a>>>),

//     /// Mapper-based record builders, e.g.
//     /// { Task.parallel <-
//     ///     foo: Task.getData Foo,
//     ///     bar: Task.getData Bar,
//     /// }
//     RecordBuilder {
//         mapper: &'a Loc<Expr<'a>>,
//         fields: Collection<'a, Loc<AssignedField<'a, Expr<'a>>>>,
//     },

//     // Lookups
//     Var {
//         module_name: &'a str, // module_name will only be filled if the original Roc code stated something like `5 + SomeModule.myVar`, module_name will be blank if it was `5 + myVar`
//         ident: &'a str,
//     },

//     Underscore(&'a str),

//     // The "crash" keyword
//     Crash,

//     // Tags
//     Tag(&'a str),

//     // Reference to an opaque type, e.g. @Opaq
//     OpaqueRef(&'a str),

//     // Pattern Matching
//     Closure(&'a [Loc<Pattern<'a>>], &'a Loc<Expr<'a>>),
//     /// Multiple defs in a row
//     Defs(&'a Defs<'a>, &'a Loc<Expr<'a>>),

//     Backpassing(&'a [Loc<Pattern<'a>>], &'a Loc<Expr<'a>>, &'a Loc<Expr<'a>>),

//     Dbg,
//     DbgStmt {
//         first: &'a Loc<Expr<'a>>,
//         extra_args: &'a [&'a Loc<Expr<'a>>],
//         continuation: &'a Loc<Expr<'a>>,
//     },

//     /// The `try` keyword that performs early return on errors
//     Try,
//     // This form of try is a desugared Result unwrapper
//     LowLevelTry(&'a Loc<Expr<'a>>, ResultTryKind),

//     // This form of debug is a desugared call to roc_dbg
//     LowLevelDbg(&'a (&'a str, &'a str), &'a Loc<Expr<'a>>, &'a Loc<Expr<'a>>),

//     // Application
//     /// To apply by name, do Apply(Var(...), ...)
//     /// To apply a tag by name, do Apply(Tag(...), ...)
//     Apply(&'a Loc<Expr<'a>>, &'a [&'a Loc<Expr<'a>>], CalledVia),
//     BinOps(&'a [(Loc<Expr<'a>>, Loc<BinOp>)], &'a Loc<Expr<'a>>),
//     UnaryOp(&'a Loc<Expr<'a>>, Loc<UnaryOp>),

//     // Conditionals
//     If {
//         if_thens: &'a [(Loc<Expr<'a>>, Loc<Expr<'a>>)],
//         final_else: &'a Loc<Expr<'a>>,
//         indented_else: bool,
//     },
//     When(
//         /// The condition
//         &'a Loc<Expr<'a>>,
//         /// A | B if bool -> expression
//         /// <Pattern 1> | <Pattern 2> if <Guard> -> <Expr>
//         /// Vec, because there may be many patterns, and the guard
//         /// is Option<Expr> because each branch may be preceded by
//         /// a guard (".. if ..").
//         &'a [&'a WhenBranch<'a>],
//     ),

//     Return(
//         /// The return value
//         &'a Loc<Expr<'a>>,
//         /// The unused code after the return statement
//         Option<&'a Loc<Expr<'a>>>,
//     ),

//     // Blank Space (e.g. comments, spaces, newlines) before or after an expression.
//     // We preserve this for the formatter; canonicalization ignores it.
//     SpaceBefore(&'a Expr<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a Expr<'a>, &'a [CommentOrNewline<'a>]),
//     ParensAround(&'a Expr<'a>),

//     // Problems
//     MalformedIdent(&'a str, crate::ident::BadIdent),
//     MalformedSuffixed(&'a Loc<Expr<'a>>),
//     // Both operators were non-associative, e.g. (True == False == False).
//     // We should tell the author to disambiguate by grouping them with parens.
//     PrecedenceConflict(&'a PrecedenceConflict<'a>),
//     EmptyRecordBuilder(&'a Loc<Expr<'a>>),
//     SingleFieldRecordBuilder(&'a Loc<Expr<'a>>),
//     OptionalFieldInRecordBuilder(&'a Loc<&'a str>, &'a Loc<Expr<'a>>),
// }

impl<'b> Nodify for &'b str {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::Str(arena.alloc_str(self))
    }
}

impl<T: Nodify> Nodify for Loc<T> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::Loc(self.region, arena.alloc(self.value.nodify(arena)))
    }
}

impl Nodify for Region {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::Region(*self)
    }
}

impl<'b, T: Nodify> Nodify for AssignedField<'b, T> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            AssignedField::RequiredValue(loc_name, sp, val) => Node::NamedTuple {
                name: "RequiredValue",
                children: arena.alloc_slice_copy(&[
                    loc_name.nodify(arena),
                    nodify_spaces(arena, sp),
                    val.nodify(arena),
                ]),
            },
            AssignedField::OptionalValue(loc_name, sp, val) => Node::NamedTuple {
                name: "OptionalValue",
                children: arena.alloc_slice_copy(&[
                    loc_name.nodify(arena),
                    nodify_spaces(arena, sp),
                    val.nodify(arena),
                ]),
            },
            AssignedField::IgnoredValue(loc_name, sp, val) => Node::NamedTuple {
                name: "IgnoredValue",
                children: arena.alloc_slice_copy(&[
                    loc_name.nodify(arena),
                    nodify_spaces(arena, sp),
                    val.nodify(arena),
                ]),
            },
            AssignedField::LabelOnly(loc_name) => Node::NamedTuple {
                name: "LabelOnly",
                children: arena.alloc_slice_copy(&[loc_name.nodify(arena)]),
            },
            AssignedField::SpaceBefore(inner, sp) => Node::NamedTuple {
                name: "SpaceBefore",
                children: arena.alloc_slice_copy(&[inner.nodify(arena), nodify_spaces(arena, sp)]),
            },
            AssignedField::SpaceAfter(inner, sp) => Node::NamedTuple {
                name: "SpaceAfter",
                children: arena.alloc_slice_copy(&[inner.nodify(arena), nodify_spaces(arena, sp)]),
            },
        }
    }
}

fn nodify_spaces<'a>(arena: &'a Bump, sp: &[CommentOrNewline<'_>]) -> Node<'a> {
    Node::Spaces(arena.alloc_slice_fill_iter(sp.iter().map(|sp| nodify_space(arena, sp))))
}

fn nodify_space<'a>(arena: &'a Bump, sp: &CommentOrNewline<'_>) -> Space<'a> {
    match sp {
        CommentOrNewline::Newline => Space::Newline,
        CommentOrNewline::LineComment(text) => Space::Comment(arena.alloc_str(text)),
        CommentOrNewline::DocComment(text) => Space::DocComment(arena.alloc_str(text)),
    }
}

// Implement Nodify for Expr (above). Note that Node is intended to look a lot like the
// Rust "debug format" syntax.
impl<'b> Nodify for Expr<'b> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            Expr::Num(content) => Node::NamedTuple {
                name: "Num",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Expr::NonBase10Int {
                string,
                base,
                is_negative,
            } => Node::NamedTuple {
                name: "NonBase10Int",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(string)),
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", base))),
                    Node::Bool(*is_negative),
                ]),
            },
            Expr::Float(content) => Node::NamedTuple {
                name: "Float",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Expr::Str(literal) => Node::NamedTuple {
                name: "Str",
                children: arena.alloc_slice_copy(&[literal.nodify(arena)]),
            },
            Expr::SingleQuote(content) => Node::NamedTuple {
                name: "SingleQuote",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Expr::RecordAccess(expr, field) => Node::NamedTuple {
                name: "RecordAccess",
                children: arena
                    .alloc_slice_copy(&[expr.nodify(arena), Node::Str(arena.alloc_str(field))]),
            },
            Expr::AccessorFunction(accessor) => Node::NamedTuple {
                name: "AccessorFunction",
                children: arena.alloc_slice_copy(&[Node::DebugFormatted(
                    arena.alloc_str(&format!("{:?}", accessor)),
                )]),
            },
            Expr::RecordUpdater(field) => Node::NamedTuple {
                name: "RecordUpdater",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(field))]),
            },
            Expr::TupleAccess(expr, field) => Node::NamedTuple {
                name: "TupleAccess",
                children: arena
                    .alloc_slice_copy(&[expr.nodify(arena), Node::Str(arena.alloc_str(field))]),
            },
            Expr::TrySuffix { target, expr } => Node::NamedTuple {
                name: "TrySuffix",
                children: arena.alloc_slice_copy(&[
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", target))),
                    expr.nodify(arena),
                ]),
            },
            Expr::List(collection) => Node::NamedTuple {
                name: "List",
                children: arena.alloc_slice_copy(&[collection.nodify(arena)]),
            },
            Expr::RecordUpdate { update, fields } => Node::NamedTuple {
                name: "RecordUpdate",
                children: arena.alloc_slice_copy(&[update.nodify(arena), fields.nodify(arena)]),
            },
            Expr::Record(fields) => Node::NamedTuple {
                name: "Record",
                children: arena.alloc_slice_copy(&[fields.nodify(arena)]),
            },
            Expr::Tuple(collection) => Node::NamedTuple {
                name: "Tuple",
                children: arena.alloc_slice_copy(&[collection.nodify(arena)]),
            },
            Expr::RecordBuilder { mapper, fields } => Node::NamedTuple {
                name: "RecordBuilder",
                children: arena.alloc_slice_copy(&[mapper.nodify(arena), fields.nodify(arena)]),
            },
            Expr::Var { module_name, ident } => Node::NamedTuple {
                name: "Var",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(module_name)),
                    Node::Str(arena.alloc_str(ident)),
                ]),
            },
            Expr::Underscore(content) => Node::NamedTuple {
                name: "Underscore",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Expr::Crash => Node::NamedTuple {
                name: "Crash",
                children: &[],
            },
            Expr::Tag(content) => Node::NamedTuple {
                name: "Tag",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Expr::OpaqueRef(content) => Node::NamedTuple {
                name: "OpaqueRef",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Expr::Closure(patterns, expr) => Node::NamedTuple {
                name: "Closure",
                children: arena.alloc_slice_copy(&[
                    Node::Array(arena.alloc_slice_fill_iter(
                        patterns.iter().map(|pattern| pattern.nodify(arena)),
                    )),
                    expr.nodify(arena),
                ]),
            },
            Expr::Defs(defs, expr) => Node::NamedTuple {
                name: "Defs",
                children: arena.alloc_slice_copy(&[defs.nodify(arena), expr.nodify(arena)]),
            },
            Expr::Backpassing(patterns, expr1, expr2) => Node::NamedTuple {
                name: "Backpassing",
                children: arena.alloc_slice_copy(&[
                    Node::Array(arena.alloc_slice_fill_iter(
                        patterns.iter().map(|pattern| pattern.nodify(arena)),
                    )),
                    expr1.nodify(arena),
                    expr2.nodify(arena),
                ]),
            },
            Expr::Dbg => Node::NamedTuple {
                name: "Dbg",
                children: &[],
            },
            Expr::DbgStmt {
                first,
                extra_args,
                continuation,
            } => Node::NamedTuple {
                name: "DbgStmt",
                children: arena.alloc_slice_copy(&[
                    first.nodify(arena),
                    Node::Array(
                        arena.alloc_slice_fill_iter(extra_args.iter().map(|arg| arg.nodify(arena))),
                    ),
                    continuation.nodify(arena),
                ]),
            },
            Expr::Try => Node::NamedTuple {
                name: "Try",
                children: &[],
            },
            Expr::LowLevelTry(expr, kind) => Node::NamedTuple {
                name: "LowLevelTry",
                children: arena.alloc_slice_copy(&[
                    expr.nodify(arena),
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", kind))),
                ]),
            },
            Expr::LowLevelDbg((str1, str2), expr1, expr2) => Node::NamedTuple {
                name: "LowLevelDbg",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(str1)),
                    Node::Str(arena.alloc_str(str2)),
                    expr1.nodify(arena),
                    expr2.nodify(arena),
                ]),
            },
            Expr::Apply(expr, args, via) => Node::NamedTuple {
                name: "Apply",
                children: arena.alloc_slice_copy(&[
                    expr.nodify(arena),
                    Node::Array(
                        arena.alloc_slice_fill_iter(args.iter().map(|arg| arg.nodify(arena))),
                    ),
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", via))),
                ]),
            },
            Expr::BinOps(ops, expr) => Node::NamedTuple {
                name: "BinOps",
                children: arena.alloc_slice_copy(&[
                    Node::Array(arena.alloc_slice_fill_iter(ops.iter().map(|(op_expr, op)| {
                        Node::Tuple(arena.alloc_slice_copy(&[
                            op_expr.nodify(arena),
                            Node::DebugFormatted(arena.alloc_str(&format!("{:?}", op))),
                        ]))
                    }))),
                    expr.nodify(arena),
                ]),
            },
            Expr::UnaryOp(expr, op) => Node::NamedTuple {
                name: "UnaryOp",
                children: arena.alloc_slice_copy(&[
                    expr.nodify(arena),
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", op))),
                ]),
            },
            Expr::If {
                if_thens,
                final_else,
                indented_else,
            } => Node::NamedTuple {
                name: "If",
                children: arena.alloc_slice_copy(&[
                    Node::Array(arena.alloc_slice_fill_iter(if_thens.iter().map(
                        |(cond, body)| {
                            Node::Tuple(
                                arena.alloc_slice_copy(&[cond.nodify(arena), body.nodify(arena)]),
                            )
                        },
                    ))),
                    final_else.nodify(arena),
                    Node::Bool(*indented_else),
                ]),
            },
            Expr::When(cond, branches) => {
                Node::NamedTuple {
                    name: "When",
                    children: arena.alloc_slice_copy(&[
                        cond.nodify(arena),
                        Node::Array(arena.alloc_slice_fill_iter(
                            branches.iter().map(|branch| branch.nodify(arena)),
                        )),
                    ]),
                }
            }
            Expr::Return(expr, unused) => Node::NamedTuple {
                name: "Return",
                children: arena.alloc_slice_copy(&[
                    expr.nodify(arena),
                    match unused {
                        Some(unused_expr) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[unused_expr.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ]),
            },
            Expr::SpaceBefore(expr, sp) => Node::NamedTuple {
                name: "SpaceBefore",
                children: arena.alloc_slice_copy(&[expr.nodify(arena), nodify_spaces(arena, sp)]),
            },
            Expr::SpaceAfter(expr, sp) => Node::NamedTuple {
                name: "SpaceAfter",
                children: arena.alloc_slice_copy(&[expr.nodify(arena), nodify_spaces(arena, sp)]),
            },
            Expr::ParensAround(expr) => Node::NamedTuple {
                name: "ParensAround",
                children: arena.alloc_slice_copy(&[expr.nodify(arena)]),
            },
            Expr::MalformedIdent(content, bad_ident) => Node::NamedTuple {
                name: "MalformedIdent",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(content)),
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", bad_ident))),
                ]),
            },
            Expr::MalformedSuffixed(expr) => Node::NamedTuple {
                name: "MalformedSuffixed",
                children: arena.alloc_slice_copy(&[expr.nodify(arena)]),
            },
            Expr::PrecedenceConflict(conflict) => Node::NamedTuple {
                name: "PrecedenceConflict",
                children: arena.alloc_slice_copy(&[(*conflict).nodify(arena)]),
            },
            Expr::EmptyRecordBuilder(expr) => Node::NamedTuple {
                name: "EmptyRecordBuilder",
                children: arena.alloc_slice_copy(&[expr.nodify(arena)]),
            },
            Expr::SingleFieldRecordBuilder(expr) => Node::NamedTuple {
                name: "SingleFieldRecordBuilder",
                children: arena.alloc_slice_copy(&[expr.nodify(arena)]),
            },
            Expr::OptionalFieldInRecordBuilder(loc, expr) => Node::NamedTuple {
                name: "OptionalFieldInRecordBuilder",
                children: arena.alloc_slice_copy(&[loc.nodify(arena), expr.nodify(arena)]),
            },
        }
    }
}

// pub struct PrecedenceConflict<'a> {
//     pub whole_region: Region,
//     pub binop1_position: Position,
//     pub binop2_position: Position,
//     pub binop1: BinOp,
//     pub binop2: BinOp,
//     pub expr: &'a Loc<Expr<'a>>,
// }

impl<'b> Nodify for PrecedenceConflict<'b> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "PrecedenceConflict",
            fields: arena.alloc_slice_copy(&[
                ("whole_region", self.whole_region.nodify(arena)),
                (
                    "binop1_position",
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", self.binop1_position))),
                ),
                (
                    "binop2_position",
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", self.binop2_position))),
                ),
                (
                    "binop1",
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", self.binop1))),
                ),
                (
                    "binop2",
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", self.binop2))),
                ),
                ("expr", self.expr.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Clone, Copy, Debug, PartialEq)]
// pub enum StrSegment<'a> {
//     Plaintext(&'a str),              // e.g. "foo"
//     Unicode(Loc<&'a str>),           // e.g. "00A0" in "\u(00A0)"
//     EscapedChar(EscapedChar),        // e.g. '\n' in "Hello!\n"
//     Interpolated(Loc<&'a Expr<'a>>), // e.g. "$(expr)"
// }

// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum SingleQuoteSegment<'a> {
//     Plaintext(&'a str),    // e.g. 'f'
//     Unicode(Loc<&'a str>), // e.g. '00A0' in '\u(00A0)'
//     EscapedChar(EscapedChar), // e.g. '\n'
//                            // No interpolated expressions in single-quoted strings
// }

// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum EscapedChar {
//     Newline,        // \n
//     Tab,            // \t
//     DoubleQuote,    // \"
//     SingleQuote,    // \'
//     Backslash,      // \\
//     CarriageReturn, // \r
//     Dollar,         // \$
// }

// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum SingleQuoteLiteral<'a> {
//     /// The most common case: a plain character with no escapes
//     PlainLine(&'a str),
//     Line(&'a [SingleQuoteSegment<'a>]),
// }

// #[derive(Clone, Copy, Debug, PartialEq)]
// pub enum StrLiteral<'a> {
//     /// The most common case: a plain string with no escapes or interpolations
//     PlainLine(&'a str),
//     Line(&'a [StrSegment<'a>]),
//     Block(&'a [&'a [StrSegment<'a>]]),
// }

impl<'b> Nodify for StrSegment<'b> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            StrSegment::Plaintext(content) => Node::NamedTuple {
                name: "Plaintext",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            StrSegment::Unicode(loc) => Node::NamedTuple {
                name: "Unicode",
                children: arena.alloc_slice_copy(&[loc.nodify(arena)]),
            },
            StrSegment::EscapedChar(escaped_char) => Node::NamedTuple {
                name: "EscapedChar",
                children: arena.alloc_slice_copy(&[Node::DebugFormatted(
                    arena.alloc_str(&format!("{:?}", escaped_char)),
                )]),
            },
            StrSegment::Interpolated(loc) => Node::NamedTuple {
                name: "Interpolated",
                children: arena.alloc_slice_copy(&[loc.nodify(arena)]),
            },
        }
    }
}

impl<'b> Nodify for StrLiteral<'b> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            StrLiteral::PlainLine(content) => Node::NamedTuple {
                name: "PlainLine",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            StrLiteral::Line(segments) => {
                Node::NamedTuple {
                    name: "Line",
                    children: arena.alloc_slice_copy(&[Node::Array(arena.alloc_slice_fill_iter(
                        segments.iter().map(|segment| segment.nodify(arena)),
                    ))]),
                }
            }
            StrLiteral::Block(lines) => Node::NamedTuple {
                name: "Block",
                children: arena.alloc_slice_copy(&[Node::Array(arena.alloc_slice_fill_iter(
                    lines.iter().map(|line| {
                        Node::Array(arena.alloc_slice_fill_iter(
                            line.iter().map(|segment| segment.nodify(arena)),
                        ))
                    }),
                ))]),
            },
        }
    }
}

// #[derive(Copy, Clone)]
// pub struct Collection<'a, T> {
//     pub items: &'a [T],
//     // Use a pointer to a slice (rather than just a slice), in order to avoid bloating
//     // Ast variants. The final_comments field is rarely accessed in the hot path, so
//     // this shouldn't matter much for perf.
//     // Use an Option, so it's possible to initialize without allocating.
//     final_comments: Option<&'a &'a [CommentOrNewline<'a>]>,
// }

impl<T: Nodify> Nodify for Collection<'_, T> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "Collection",
            fields: arena.alloc_slice_copy(&[
                (
                    "items",
                    Node::Array(
                        arena.alloc_slice_fill_iter(
                            self.items.iter().map(|item| item.nodify(arena)),
                        ),
                    ),
                ),
                (
                    "final_comments",
                    nodify_spaces(arena, self.final_comments()),
                ),
            ]),
        }
    }
}

// #[derive(Clone, Copy, Debug, PartialEq)]
// pub struct WhenBranch<'a> {
//     pub patterns: &'a [Loc<Pattern<'a>>],
//     pub value: Loc<Expr<'a>>,
//     pub guard: Option<Loc<Expr<'a>>>,
// }

impl Nodify for WhenBranch<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "WhenBranch",
            fields: arena.alloc_slice_copy(&[
                (
                    "patterns",
                    Node::Array(arena.alloc_slice_fill_iter(
                        self.patterns.iter().map(|pattern| pattern.nodify(arena)),
                    )),
                ),
                ("value", self.value.nodify(arena)),
                (
                    "guard",
                    match self.guard {
                        Some(guard) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[guard.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
            ]),
        }
    }
}

// #[derive(Clone, Copy, Debug, PartialEq)]
// pub struct WhenPattern<'a> {
//     pub pattern: Loc<Pattern<'a>>,
//     pub guard: Option<Loc<Expr<'a>>>,
// }

impl Nodify for WhenPattern<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "WhenPattern",
            fields: arena.alloc_slice_copy(&[
                ("pattern", self.pattern.nodify(arena)),
                (
                    "guard",
                    match self.guard {
                        Some(guard) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[guard.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
            ]),
        }
    }
}

// pub enum Pattern<'a> {
//     // Identifier
//     Identifier {
//         ident: &'a str,
//     },
//     QualifiedIdentifier {
//         module_name: &'a str,
//         ident: &'a str,
//     },

//     Tag(&'a str),

//     OpaqueRef(&'a str),

//     Apply(&'a Loc<Pattern<'a>>, &'a [Loc<Pattern<'a>>]),

//     /// This is Located<Pattern> rather than Located<str> so we can record comments
//     /// around the destructured names, e.g. { x ### x does stuff ###, y }
//     /// In practice, these patterns will always be Identifier
//     RecordDestructure(Collection<'a, Loc<Pattern<'a>>>),

//     /// A required field pattern, e.g. { x: Just 0 } -> ...
//     /// Can only occur inside of a RecordDestructure
//     RequiredField(&'a str, &'a Loc<Pattern<'a>>),

//     /// An optional field pattern, e.g. { x ? Just 0 } -> ...
//     /// Can only occur inside of a RecordDestructure
//     OptionalField(&'a str, &'a Loc<Expr<'a>>),

//     // Literal
//     NumLiteral(&'a str),
//     NonBase10Literal {
//         string: &'a str,
//         base: Base,
//         is_negative: bool,
//     },
//     FloatLiteral(&'a str),
//     StrLiteral(StrLiteral<'a>),

//     /// Underscore pattern
//     /// Contains the name of underscore pattern (e.g. "a" is for "_a" in code)
//     /// Empty string is unnamed pattern ("" is for "_" in code)
//     Underscore(&'a str),
//     SingleQuote(&'a str),

//     /// A tuple pattern, e.g. (Just x, 1)
//     Tuple(Collection<'a, Loc<Pattern<'a>>>),

//     /// A list pattern like [_, x, ..]
//     List(Collection<'a, Loc<Pattern<'a>>>),

//     /// A list-rest pattern ".."
//     /// Can only occur inside of a [Pattern::List]
//     ListRest(Option<(&'a [CommentOrNewline<'a>], PatternAs<'a>)>),

//     As(&'a Loc<Pattern<'a>>, PatternAs<'a>),

//     // Space
//     SpaceBefore(&'a Pattern<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a Pattern<'a>, &'a [CommentOrNewline<'a>]),

//     // Malformed
//     Malformed(&'a str),
//     MalformedIdent(&'a str, crate::ident::BadIdent),
// }

impl Nodify for Pattern<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            Pattern::Identifier { ident } => Node::NamedTuple {
                name: "Identifier",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(ident))]),
            },
            Pattern::QualifiedIdentifier { module_name, ident } => Node::NamedTuple {
                name: "QualifiedIdentifier",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(module_name)),
                    Node::Str(arena.alloc_str(ident)),
                ]),
            },
            Pattern::Tag(content) => Node::NamedTuple {
                name: "Tag",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Pattern::OpaqueRef(content) => Node::NamedTuple {
                name: "OpaqueRef",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Pattern::Apply(pattern, args) => Node::NamedTuple {
                name: "Apply",
                children: arena.alloc_slice_copy(&[
                    pattern.nodify(arena),
                    Node::Array(
                        arena.alloc_slice_fill_iter(args.iter().map(|arg| arg.nodify(arena))),
                    ),
                ]),
            },
            Pattern::RecordDestructure(patterns) => Node::NamedTuple {
                name: "RecordDestructure",
                children: arena.alloc_slice_copy(&[patterns.nodify(arena)]),
            },
            Pattern::RequiredField(field, pattern) => Node::NamedTuple {
                name: "RequiredField",
                children: arena
                    .alloc_slice_copy(&[Node::Str(arena.alloc_str(field)), pattern.nodify(arena)]),
            },
            Pattern::OptionalField(field, expr) => Node::NamedTuple {
                name: "OptionalField",
                children: arena
                    .alloc_slice_copy(&[Node::Str(arena.alloc_str(field)), expr.nodify(arena)]),
            },
            Pattern::NumLiteral(content) => Node::NamedTuple {
                name: "NumLiteral",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Pattern::NonBase10Literal {
                string,
                base,
                is_negative,
            } => Node::NamedTuple {
                name: "NonBase10Literal",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(string)),
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", base))),
                    Node::Bool(*is_negative),
                ]),
            },
            Pattern::FloatLiteral(content) => Node::NamedTuple {
                name: "FloatLiteral",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Pattern::StrLiteral(literal) => Node::NamedTuple {
                name: "StrLiteral",
                children: arena.alloc_slice_copy(&[literal.nodify(arena)]),
            },
            Pattern::Underscore(content) => Node::NamedTuple {
                name: "Underscore",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Pattern::SingleQuote(content) => Node::NamedTuple {
                name: "SingleQuote",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Pattern::Tuple(collection) => Node::NamedTuple {
                name: "Tuple",
                children: arena.alloc_slice_copy(&[collection.nodify(arena)]),
            },
            Pattern::List(collection) => Node::NamedTuple {
                name: "List",
                children: arena.alloc_slice_copy(&[collection.nodify(arena)]),
            },
            Pattern::ListRest(rest) => Node::NamedTuple {
                name: "ListRest",
                children: arena.alloc_slice_copy(&[match rest {
                    Some((comments, pattern_as)) => Node::NamedTuple {
                        name: "Some",
                        children: arena.alloc_slice_copy(&[
                            nodify_spaces(arena, comments),
                            pattern_as.nodify(arena),
                        ]),
                    },
                    None => Node::NamedTuple {
                        name: "None",
                        children: &[],
                    },
                }]),
            },
            Pattern::As(pattern, pattern_as) => Node::NamedTuple {
                name: "As",
                children: arena
                    .alloc_slice_copy(&[pattern.nodify(arena), pattern_as.nodify(arena)]),
            },
            Pattern::SpaceBefore(pattern, sp) => Node::NamedTuple {
                name: "SpaceBefore",
                children: arena
                    .alloc_slice_copy(&[pattern.nodify(arena), nodify_spaces(arena, sp)]),
            },
            Pattern::SpaceAfter(pattern, sp) => Node::NamedTuple {
                name: "SpaceAfter",
                children: arena
                    .alloc_slice_copy(&[pattern.nodify(arena), nodify_spaces(arena, sp)]),
            },
            Pattern::Malformed(content) => Node::NamedTuple {
                name: "Malformed",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            Pattern::MalformedIdent(content, bad_ident) => Node::NamedTuple {
                name: "MalformedIdent",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(content)),
                    Node::DebugFormatted(arena.alloc_str(&format!("{:?}", bad_ident))),
                ]),
            },
        }
    }
}

// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub struct PatternAs<'a> {
//     pub spaces_before: &'a [CommentOrNewline<'a>],
//     pub identifier: Loc<&'a str>,
// }

impl Nodify for PatternAs<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "PatternAs",
            fields: arena.alloc_slice_copy(&[
                ("spaces_before", nodify_spaces(arena, self.spaces_before)),
                ("identifier", self.identifier.nodify(arena)),
            ]),
        }
    }
}

impl Nodify for Defs<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        let mut items = Vec::new_in(arena);
        for (i, tag) in self.tags.iter().enumerate() {
            let before = &self.spaces[self.space_before[i].indices()];
            let after = &self.spaces[self.space_before[i].indices()];

            let item = match tag.split() {
                Ok(td) => {
                    let td = self.type_defs[td.index()];
                    if before.is_empty() && after.is_empty() {
                        td.nodify(arena)
                    } else {
                        Node::NamedTuple {
                            name: "Spaces",
                            children: arena.alloc_slice_copy(&[
                                nodify_spaces(arena, before),
                                td.nodify(arena),
                                nodify_spaces(arena, after),
                            ]),
                        }
                    }
                }
                Err(vd) => {
                    let vd = self.value_defs[vd.index()];
                    if before.is_empty() && after.is_empty() {
                        vd.nodify(arena)
                    } else {
                        Node::NamedTuple {
                            name: "Spaces",
                            children: arena.alloc_slice_copy(&[
                                nodify_spaces(arena, before),
                                vd.nodify(arena),
                                nodify_spaces(arena, after),
                            ]),
                        }
                    }
                }
            };

            items.push(item);
        }

        Node::NamedTuple {
            name: "Defs",
            children: arena.alloc_slice_copy(&[Node::Array(items.into_bump_slice())]),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub enum TypeDef<'a> {
//     /// A type alias. This is like a standalone annotation, except the pattern
//     /// must be a capitalized Identifier, e.g.
//     ///
//     /// Foo : Bar Baz
//     Alias {
//         header: TypeHeader<'a>,
//         ann: Loc<TypeAnnotation<'a>>,
//     },

//     /// An opaque type, wrapping its inner type. E.g. Age := U64.
//     Opaque {
//         header: TypeHeader<'a>,
//         typ: Loc<TypeAnnotation<'a>>,
//         derived: Option<Loc<ImplementsAbilities<'a>>>,
//     },

//     /// An ability definition. E.g.
//     ///   Hash implements
//     ///     hash : a -> U64 where a implements Hash
//     Ability {
//         header: TypeHeader<'a>,
//         loc_implements: Loc<Implements<'a>>,
//         members: &'a [AbilityMember<'a>],
//     },
// }

impl Nodify for TypeDef<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            TypeDef::Alias { header, ann } => Node::NamedStruct {
                name: "Alias",
                fields: arena.alloc_slice_copy(&[
                    ("header", header.nodify(arena)),
                    ("ann", ann.nodify(arena)),
                ]),
            },
            TypeDef::Opaque {
                header,
                typ,
                derived,
            } => Node::NamedStruct {
                name: "Opaque",
                fields: arena.alloc_slice_copy(&[
                    ("header", header.nodify(arena)),
                    ("typ", typ.nodify(arena)),
                    (
                        "derived",
                        match derived {
                            Some(derived) => Node::NamedTuple {
                                name: "Some",
                                children: arena.alloc_slice_copy(&[derived.nodify(arena)]),
                            },
                            None => Node::NamedTuple {
                                name: "None",
                                children: &[],
                            },
                        },
                    ),
                ]),
            },
            TypeDef::Ability {
                header,
                loc_implements,
                members,
            } => Node::NamedStruct {
                name: "Ability",
                fields: arena.alloc_slice_copy(&[
                    ("header", header.nodify(arena)),
                    ("loc_implements", loc_implements.nodify(arena)),
                    (
                        "members",
                        Node::Array(arena.alloc_slice_fill_iter(
                            members.iter().map(|member| member.nodify(arena)),
                        )),
                    ),
                ]),
            },
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub enum ValueDef<'a> {
//     // TODO in canonicalization, validate the pattern; only certain patterns
//     // are allowed in annotations.
//     Annotation(Loc<Pattern<'a>>, Loc<TypeAnnotation<'a>>),

//     // TODO in canonicalization, check to see if there are any newlines after the
//     // annotation; if not, and if it's followed by a Body, then the annotation
//     // applies to that expr! (TODO: verify that the pattern for both annotation and body match.)
//     // No need to track that relationship in any data structure.
//     Body(&'a Loc<Pattern<'a>>, &'a Loc<Expr<'a>>),

//     AnnotatedBody {
//         ann_pattern: &'a Loc<Pattern<'a>>,
//         ann_type: &'a Loc<TypeAnnotation<'a>>,
//         lines_between: &'a [CommentOrNewline<'a>],
//         body_pattern: &'a Loc<Pattern<'a>>,
//         body_expr: &'a Loc<Expr<'a>>,
//     },

//     Dbg {
//         condition: &'a Loc<Expr<'a>>,
//         preceding_comment: Region,
//     },

//     Expect {
//         condition: &'a Loc<Expr<'a>>,
//         preceding_comment: Region,
//     },

//     /// e.g. `import InternalHttp as Http exposing [Req]`.
//     ModuleImport(ModuleImport<'a>),

//     /// e.g. `import "path/to/my/file.txt" as myFile : Str`
//     IngestedFileImport(IngestedFileImport<'a>),

//     Stmt(&'a Loc<Expr<'a>>),

//     StmtAfterExpr,
// }

impl Nodify for ValueDef<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            ValueDef::Annotation(pattern, ann) => Node::NamedStruct {
                name: "Annotation",
                fields: arena.alloc_slice_copy(&[
                    ("pattern", pattern.nodify(arena)),
                    ("ann", ann.nodify(arena)),
                ]),
            },
            ValueDef::Body(pattern, expr) => Node::NamedStruct {
                name: "Body",
                fields: arena.alloc_slice_copy(&[
                    ("pattern", pattern.nodify(arena)),
                    ("expr", expr.nodify(arena)),
                ]),
            },
            ValueDef::AnnotatedBody {
                ann_pattern,
                ann_type,
                lines_between,
                body_pattern,
                body_expr,
            } => Node::NamedStruct {
                name: "AnnotatedBody",
                fields: arena.alloc_slice_copy(&[
                    ("ann_pattern", ann_pattern.nodify(arena)),
                    ("ann_type", ann_type.nodify(arena)),
                    ("lines_between", nodify_spaces(arena, lines_between)),
                    ("body_pattern", body_pattern.nodify(arena)),
                    ("body_expr", body_expr.nodify(arena)),
                ]),
            },
            ValueDef::Dbg {
                condition,
                preceding_comment,
            } => Node::NamedStruct {
                name: "Dbg",
                fields: arena.alloc_slice_copy(&[
                    ("condition", condition.nodify(arena)),
                    ("preceding_comment", preceding_comment.nodify(arena)),
                ]),
            },
            ValueDef::Expect {
                condition,
                preceding_comment,
            } => Node::NamedStruct {
                name: "Expect",
                fields: arena.alloc_slice_copy(&[
                    ("condition", condition.nodify(arena)),
                    ("preceding_comment", preceding_comment.nodify(arena)),
                ]),
            },
            ValueDef::ModuleImport(import) => Node::NamedTuple {
                name: "ModuleImport",
                children: arena.alloc_slice_copy(&[import.nodify(arena)]),
            },
            ValueDef::IngestedFileImport(import) => Node::NamedTuple {
                name: "IngestedFileImport",
                children: arena.alloc_slice_copy(&[import.nodify(arena)]),
            },
            ValueDef::Stmt(expr) => Node::NamedStruct {
                name: "Stmt",
                fields: arena.alloc_slice_copy(&[("expr", expr.nodify(arena))]),
            },
            ValueDef::StmtAfterExpr => Node::NamedStruct {
                name: "StmtAfterExpr",
                fields: &[],
            },
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct TypeHeader<'a> {
//     pub name: Loc<&'a str>,
//     pub vars: &'a [Loc<Pattern<'a>>],
// }

impl Nodify for TypeHeader<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "TypeHeader",
            fields: arena.alloc_slice_copy(&[
                ("name", self.name.nodify(arena)),
                (
                    "vars",
                    Node::Array(
                        arena.alloc_slice_fill_iter(self.vars.iter().map(|var| var.nodify(arena))),
                    ),
                ),
            ]),
        }
    }
}

// /// The `implements` keyword associated with ability definitions.
// #[derive(Debug, Clone, Copy, PartialEq)]
// pub enum Implements<'a> {
//     Implements,
//     SpaceBefore(&'a Implements<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a Implements<'a>, &'a [CommentOrNewline<'a>]),
// }

impl Nodify for Implements<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            Implements::Implements => Node::NamedEmpty { name: "Implements" },
            Implements::SpaceBefore(implements, sp) => Node::NamedTuple {
                name: "SpaceBefore",
                children: arena
                    .alloc_slice_copy(&[implements.nodify(arena), nodify_spaces(arena, sp)]),
            },
            Implements::SpaceAfter(implements, sp) => Node::NamedTuple {
                name: "SpaceAfter",
                children: arena
                    .alloc_slice_copy(&[implements.nodify(arena), nodify_spaces(arena, sp)]),
            },
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct AbilityMember<'a> {
//     pub name: Loc<Spaced<'a, &'a str>>,
//     pub typ: Loc<TypeAnnotation<'a>>,
// }

impl Nodify for AbilityMember<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "AbilityMember",
            fields: arena.alloc_slice_copy(&[
                ("name", self.name.nodify(arena)),
                ("typ", self.typ.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct ModuleImport<'a> {
//     pub before_name: &'a [CommentOrNewline<'a>],
//     pub name: Loc<ImportedModuleName<'a>>,
//     pub params: Option<ModuleImportParams<'a>>,
//     pub alias: Option<header::KeywordItem<'a, ImportAsKeyword, Loc<ImportAlias<'a>>>>,
//     pub exposed: Option<
//         header::KeywordItem<
//             'a,
//             ImportExposingKeyword,
//             Collection<'a, Loc<Spaced<'a, header::ExposedName<'a>>>>,
//         >,
//     >,
// }

impl Nodify for ModuleImport<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "ModuleImport",
            fields: arena.alloc_slice_copy(&[
                ("before_name", nodify_spaces(arena, self.before_name)),
                ("name", self.name.nodify(arena)),
                (
                    "params",
                    match &self.params {
                        Some(params) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[params.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
                (
                    "alias",
                    match &self.alias {
                        Some(alias) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[alias.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
                (
                    "exposed",
                    match &self.exposed {
                        Some(exposed) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[exposed.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
            ]),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct ImportedModuleName<'a> {
//     pub package: Option<&'a str>,
//     pub name: ModuleName<'a>,
// }

impl Nodify for ImportedModuleName<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "ImportedModuleName",
            fields: arena.alloc_slice_copy(&[
                (
                    "package",
                    match &self.package {
                        Some(package) => Node::NamedTuple {
                            name: "Some",
                            children: arena
                                .alloc_slice_copy(&[Node::Str(arena.alloc_str(package))]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
                ("name", self.name.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct ModuleImportParams<'a> {
//     pub before: &'a [CommentOrNewline<'a>],
//     pub params: Loc<Collection<'a, Loc<AssignedField<'a, Expr<'a>>>>>,
// }

impl Nodify for ModuleImportParams<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "ModuleImportParams",
            fields: arena.alloc_slice_copy(&[
                ("before", nodify_spaces(arena, self.before)),
                ("params", self.params.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Copy, Clone, Debug, PartialEq, Eq)]
// pub struct KeywordItem<'a, K, V> {
//     pub keyword: Spaces<'a, K>,
//     pub item: V,
// }

impl<K: Nodify, V: Nodify> Nodify for KeywordItem<'_, K, V> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "KeywordItem",
            fields: arena.alloc_slice_copy(&[
                ("keyword", self.keyword.nodify(arena)),
                ("item", self.item.nodify(arena)),
            ]),
        }
    }
}

impl Nodify for ExposedName<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedTuple {
            name: "ExposedName",
            children: arena.alloc_slice_copy(&[self.as_str().nodify(arena)]),
        }
    }
}

impl Nodify for ModuleName<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedTuple {
            name: "ModuleName",
            children: arena.alloc_slice_copy(&[self.as_str().nodify(arena)]),
        }
    }
}

impl Nodify for PackageName<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedTuple {
            name: "PackageName",
            children: arena.alloc_slice_copy(&[self.as_str().nodify(arena)]),
        }
    }
}

impl Nodify for ImportAlias<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedTuple {
            name: "ImportAlias",
            children: arena.alloc_slice_copy(&[self.as_str().nodify(arena)]),
        }
    }
}

impl Nodify for UppercaseIdent<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        let text: &str = self.into();
        Node::NamedTuple {
            name: "UppercaseIdent",
            children: arena.alloc_slice_copy(&[text.nodify(arena)]),
        }
    }
}

impl Nodify for ImportAsKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "ImportAsKeyword",
        }
    }
}

impl Nodify for ImportExposingKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "ImportExposingKeyword",
        }
    }
}

impl Nodify for ExposesKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "ExposesKeyword",
        }
    }
}
impl Nodify for PackageKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "PackageKeyword",
        }
    }
}

impl Nodify for PackagesKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "PackagesKeyword",
        }
    }
}

impl Nodify for RequiresKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "RequiresKeyword",
        }
    }
}

impl Nodify for ProvidesKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "ProvidesKeyword",
        }
    }
}

impl Nodify for ToKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty { name: "ToKeyword" }
    }
}

impl Nodify for PlatformKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "PlatformKeyword",
        }
    }
}

impl Nodify for ImportsKeyword {
    fn nodify<'a>(&self, _arena: &'a Bump) -> Node<'a> {
        Node::NamedEmpty {
            name: "ImportsKeyword",
        }
    }
}

// #[derive(Debug, Copy, Clone, PartialEq, Eq)]
// pub struct Spaces<'a, T> {
//     pub before: &'a [CommentOrNewline<'a>],
//     pub item: T,
//     pub after: &'a [CommentOrNewline<'a>],
// }

impl<T: Nodify> Nodify for Spaces<'_, T> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "Spaces",
            fields: arena.alloc_slice_copy(&[
                ("before", nodify_spaces(arena, self.before)),
                ("item", self.item.nodify(arena)),
                ("after", nodify_spaces(arena, self.after)),
            ]),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct IngestedFileImport<'a> {
//     pub before_path: &'a [CommentOrNewline<'a>],
//     pub path: Loc<StrLiteral<'a>>,
//     pub name: header::KeywordItem<'a, ImportAsKeyword, Loc<&'a str>>,
//     pub annotation: Option<IngestedFileAnnotation<'a>>,
// }

impl Nodify for IngestedFileImport<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "IngestedFileImport",
            fields: arena.alloc_slice_copy(&[
                ("before_path", nodify_spaces(arena, self.before_path)),
                ("path", self.path.nodify(arena)),
                ("name", self.name.nodify(arena)),
                (
                    "annotation",
                    match &self.annotation {
                        Some(annotation) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[annotation.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
            ]),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct IngestedFileAnnotation<'a> {
//     pub before_colon: &'a [CommentOrNewline<'a>],
//     pub annotation: Loc<TypeAnnotation<'a>>,
// }

impl Nodify for IngestedFileAnnotation<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "IngestedFileAnnotation",
            fields: arena.alloc_slice_copy(&[
                ("before_colon", nodify_spaces(arena, self.before_colon)),
                ("annotation", self.annotation.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Debug, Copy, Clone, PartialEq)]
// pub enum TypeAnnotation<'a> {
//     /// A function. The types of its arguments, the type of arrow used, then the type of its return value.
//     Function(
//         &'a [Loc<TypeAnnotation<'a>>],
//         FunctionArrow,
//         &'a Loc<TypeAnnotation<'a>>,
//     ),

//     /// Applying a type to some arguments (e.g. Map.Map String Int)
//     Apply(&'a str, &'a str, &'a [Loc<TypeAnnotation<'a>>]),

//     /// A bound type variable, e.g. `a` in `(a -> a)`
//     BoundVariable(&'a str),

//     /// Inline type alias, e.g. `as List a` in `[Cons a (List a), Nil] as List a`
//     As(
//         &'a Loc<TypeAnnotation<'a>>,
//         &'a [CommentOrNewline<'a>],
//         TypeHeader<'a>,
//     ),

//     Record {
//         fields: Collection<'a, Loc<AssignedField<'a, TypeAnnotation<'a>>>>,
//         /// The row type variable in an open record, e.g. the `r` in `{ name: Str }r`.
//         /// This is None if it's a closed record annotation like `{ name: Str }`.
//         ext: Option<&'a Loc<TypeAnnotation<'a>>>,
//     },

//     Tuple {
//         elems: Collection<'a, Loc<TypeAnnotation<'a>>>,
//         /// The row type variable in an open tuple, e.g. the `r` in `( Str, Str )r`.
//         /// This is None if it's a closed tuple annotation like `( Str, Str )`.
//         ext: Option<&'a Loc<TypeAnnotation<'a>>>,
//     },

//     /// A tag union, e.g. `[
//     TagUnion {
//         /// The row type variable in an open tag union, e.g. the `a` in `[Foo, Bar]a`.
//         /// This is None if it's a closed tag union like `[Foo, Bar]`.
//         ext: Option<&'a Loc<TypeAnnotation<'a>>>,
//         tags: Collection<'a, Loc<Tag<'a>>>,
//     },

//     /// '_', indicating the compiler should infer the type
//     Inferred,

//     /// The `*` type variable, e.g. in (List *)
//     Wildcard,

//     /// A "where" clause demanding abilities designated by a `where`, e.g. `a -> U64 where a implements Hash`
//     Where(&'a Loc<TypeAnnotation<'a>>, &'a [Loc<ImplementsClause<'a>>]),

//     // We preserve this for the formatter; canonicalization ignores it.
//     SpaceBefore(&'a TypeAnnotation<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a TypeAnnotation<'a>, &'a [CommentOrNewline<'a>]),

//     /// A malformed type annotation, which will code gen to a runtime error
//     Malformed(&'a str),
// }

impl Nodify for TypeAnnotation<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            TypeAnnotation::Function(args, arrow, ret) => Node::NamedStruct {
                name: "Function",
                fields: arena.alloc_slice_copy(&[
                    (
                        "args",
                        Node::Array(
                            arena.alloc_slice_fill_iter(args.iter().map(|arg| arg.nodify(arena))),
                        ),
                    ),
                    (
                        "arrow",
                        Node::DebugFormatted(arena.alloc_str(&format!("{:?}", arrow))),
                    ),
                    ("ret", ret.nodify(arena)),
                ]),
            },
            TypeAnnotation::Apply(name, module, args) => Node::NamedTuple {
                name: "Apply",
                children: arena.alloc_slice_copy(&[
                    Node::Str(arena.alloc_str(name)),
                    Node::Str(arena.alloc_str(module)),
                    Node::Array(
                        arena.alloc_slice_fill_iter(args.iter().map(|arg| arg.nodify(arena))),
                    ),
                ]),
            },
            TypeAnnotation::BoundVariable(content) => Node::NamedTuple {
                name: "BoundVariable",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
            TypeAnnotation::As(ann, spaces, header) => Node::NamedStruct {
                name: "As",
                fields: arena.alloc_slice_copy(&[
                    ("ann", ann.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                    ("header", header.nodify(arena)),
                ]),
            },
            TypeAnnotation::Record { fields, ext } => Node::NamedStruct {
                name: "Record",
                fields: arena.alloc_slice_copy(&[
                    ("fields", fields.nodify(arena)),
                    (
                        "ext",
                        match ext {
                            Some(ext) => Node::NamedTuple {
                                name: "Some",
                                children: arena.alloc_slice_copy(&[ext.nodify(arena)]),
                            },
                            None => Node::NamedTuple {
                                name: "None",
                                children: &[],
                            },
                        },
                    ),
                ]),
            },
            TypeAnnotation::Tuple { elems, ext } => Node::NamedStruct {
                name: "Tuple",
                fields: arena.alloc_slice_copy(&[
                    ("elems", elems.nodify(arena)),
                    (
                        "ext",
                        match ext {
                            Some(ext) => Node::NamedTuple {
                                name: "Some",
                                children: arena.alloc_slice_copy(&[ext.nodify(arena)]),
                            },
                            None => Node::NamedTuple {
                                name: "None",
                                children: &[],
                            },
                        },
                    ),
                ]),
            },
            TypeAnnotation::TagUnion { ext, tags } => Node::NamedStruct {
                name: "TagUnion",
                fields: arena.alloc_slice_copy(&[
                    (
                        "ext",
                        match ext {
                            Some(ext) => Node::NamedTuple {
                                name: "Some",
                                children: arena.alloc_slice_copy(&[ext.nodify(arena)]),
                            },
                            None => Node::NamedTuple {
                                name: "None",
                                children: &[],
                            },
                        },
                    ),
                    ("tags", tags.nodify(arena)),
                ]),
            },
            TypeAnnotation::Inferred => Node::NamedTuple {
                name: "Inferred",
                children: &[],
            },
            TypeAnnotation::Wildcard => Node::NamedTuple {
                name: "Wildcard",
                children: &[],
            },
            TypeAnnotation::Where(ann, clauses) => Node::NamedStruct {
                name: "Where",
                fields: arena.alloc_slice_copy(&[
                    ("ann", ann.nodify(arena)),
                    (
                        "clauses",
                        Node::Array(arena.alloc_slice_fill_iter(
                            clauses.iter().map(|clause| clause.nodify(arena)),
                        )),
                    ),
                ]),
            },
            TypeAnnotation::SpaceBefore(ann, spaces) => Node::NamedStruct {
                name: "SpaceBefore",
                fields: arena.alloc_slice_copy(&[
                    ("ann", ann.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
            TypeAnnotation::SpaceAfter(ann, spaces) => Node::NamedStruct {
                name: "SpaceAfter",
                fields: arena.alloc_slice_copy(&[
                    ("ann", ann.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
            TypeAnnotation::Malformed(content) => Node::NamedTuple {
                name: "Malformed",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(content))]),
            },
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq)]
// pub enum Tag<'a> {
//     Apply {
//         name: Loc<&'a str>,
//         args: &'a [Loc<TypeAnnotation<'a>>],
//     },

//     // We preserve this for the formatter; canonicalization ignores it.
//     SpaceBefore(&'a Tag<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a Tag<'a>, &'a [CommentOrNewline<'a>]),
// }

impl Nodify for Tag<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            Tag::Apply { name, args } => Node::NamedStruct {
                name: "Apply",
                fields: arena.alloc_slice_copy(&[
                    ("name", name.nodify(arena)),
                    (
                        "args",
                        Node::Array(
                            arena.alloc_slice_fill_iter(args.iter().map(|arg| arg.nodify(arena))),
                        ),
                    ),
                ]),
            },
            Tag::SpaceBefore(tag, spaces) => Node::NamedStruct {
                name: "SpaceBefore",
                fields: arena.alloc_slice_copy(&[
                    ("tag", tag.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
            Tag::SpaceAfter(tag, spaces) => Node::NamedStruct {
                name: "SpaceAfter",
                fields: arena.alloc_slice_copy(&[
                    ("tag", tag.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
        }
    }
}

// #[derive(Debug, Copy, Clone, PartialEq)]
// pub struct ImplementsClause<'a> {
//     pub var: Loc<Spaced<'a, &'a str>>,
//     pub abilities: &'a [AbilityName<'a>],
// }

impl Nodify for ImplementsClause<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "ImplementsClause",
            fields: arena.alloc_slice_copy(&[
                ("var", self.var.nodify(arena)),
                (
                    "abilities",
                    Node::Array(arena.alloc_slice_fill_iter(
                        self.abilities.iter().map(|ability| ability.nodify(arena)),
                    )),
                ),
            ]),
        }
    }
}

// #[derive(Debug, Copy, Clone, PartialEq)]
// pub enum ImplementsAbilities<'a> {
//     /// `implements [Eq { eq: myEq }, Hash]`
//     Implements(Collection<'a, Loc<ImplementsAbility<'a>>>),

//     // We preserve this for the formatter; canonicalization ignores it.
//     SpaceBefore(&'a ImplementsAbilities<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a ImplementsAbilities<'a>, &'a [CommentOrNewline<'a>]),
// }

impl Nodify for ImplementsAbilities<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            ImplementsAbilities::Implements(abilities) => Node::NamedStruct {
                name: "Implements",
                fields: arena.alloc_slice_copy(&[("abilities", abilities.nodify(arena))]),
            },
            ImplementsAbilities::SpaceBefore(impls, spaces) => Node::NamedStruct {
                name: "SpaceBefore",
                fields: arena.alloc_slice_copy(&[
                    ("impls", impls.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
            ImplementsAbilities::SpaceAfter(impls, spaces) => Node::NamedStruct {
                name: "SpaceAfter",
                fields: arena.alloc_slice_copy(&[
                    ("impls", impls.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
        }
    }
}

// /// `Eq` or `Eq { eq: myEq }`
// #[derive(Debug, Copy, Clone, PartialEq)]
// pub enum ImplementsAbility<'a> {
//     ImplementsAbility {
//         /// Should be a zero-argument `Apply` or an error; we'll check this in canonicalization
//         ability: Loc<TypeAnnotation<'a>>,
//         impls: Option<Loc<AbilityImpls<'a>>>,
//     },

//     // We preserve this for the formatter; canonicalization ignores it.
//     SpaceBefore(&'a ImplementsAbility<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a ImplementsAbility<'a>, &'a [CommentOrNewline<'a>]),
// }

impl Nodify for ImplementsAbility<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            ImplementsAbility::ImplementsAbility { ability, impls } => Node::NamedStruct {
                name: "ImplementsAbility",
                fields: arena.alloc_slice_copy(&[
                    ("ability", ability.nodify(arena)),
                    (
                        "impls",
                        match impls {
                            Some(impls) => Node::NamedTuple {
                                name: "Some",
                                children: arena.alloc_slice_copy(&[impls.nodify(arena)]),
                            },
                            None => Node::NamedTuple {
                                name: "None",
                                children: &[],
                            },
                        },
                    ),
                ]),
            },
            ImplementsAbility::SpaceBefore(impls_ability, spaces) => Node::NamedStruct {
                name: "SpaceBefore",
                fields: arena.alloc_slice_copy(&[
                    ("impls_ability", impls_ability.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
            ImplementsAbility::SpaceAfter(impls_ability, spaces) => Node::NamedStruct {
                name: "SpaceAfter",
                fields: arena.alloc_slice_copy(&[
                    ("impls_ability", impls_ability.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
        }
    }
}

// #[derive(Debug, Copy, Clone, PartialEq)]
// pub enum AbilityImpls<'a> {
//     // `{ eq: myEq }`
//     AbilityImpls(Collection<'a, Loc<AssignedField<'a, Expr<'a>>>>),

//     // We preserve this for the formatter; canonicalization ignores it.
//     SpaceBefore(&'a AbilityImpls<'a>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a AbilityImpls<'a>, &'a [CommentOrNewline<'a>]),
// }

impl Nodify for AbilityImpls<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            AbilityImpls::AbilityImpls(collection) => Node::NamedStruct {
                name: "AbilityImpls",
                fields: arena.alloc_slice_copy(&[("collection", collection.nodify(arena))]),
            },
            AbilityImpls::SpaceBefore(impls, spaces) => Node::NamedStruct {
                name: "SpaceBefore",
                fields: arena.alloc_slice_copy(&[
                    ("impls", impls.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
            AbilityImpls::SpaceAfter(impls, spaces) => Node::NamedStruct {
                name: "SpaceAfter",
                fields: arena.alloc_slice_copy(&[
                    ("impls", impls.nodify(arena)),
                    ("spaces", nodify_spaces(arena, spaces)),
                ]),
            },
        }
    }
}

// #[derive(Copy, Clone, PartialEq)]
// pub enum Spaced<'a, T> {
//     Item(T),

//     // Spaces
//     SpaceBefore(&'a Spaced<'a, T>, &'a [CommentOrNewline<'a>]),
//     SpaceAfter(&'a Spaced<'a, T>, &'a [CommentOrNewline<'a>]),
// }

impl<T: Nodify> Nodify for Spaced<'_, T> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            Spaced::Item(item) => item.nodify(arena),
            Spaced::SpaceBefore(spaced, spaces) => Node::NamedTuple {
                name: "SpaceBefore",
                children: arena
                    .alloc_slice_copy(&[spaced.nodify(arena), nodify_spaces(arena, spaces)]),
            },
            Spaced::SpaceAfter(spaced, spaces) => Node::NamedTuple {
                name: "SpaceAfter",
                children: arena
                    .alloc_slice_copy(&[spaced.nodify(arena), nodify_spaces(arena, spaces)]),
            },
        }
    }
}

impl Nodify for FullAst<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "FullAst",
            fields: arena.alloc_slice_copy(&[
                ("header", self.header.nodify(arena)),
                ("defs", self.defs.nodify(arena)),
            ]),
        }
    }
}

impl<T: Nodify> Nodify for SpacesBefore<'_, T> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "SpacesBefore",
            fields: arena.alloc_slice_copy(&[
                ("before", nodify_spaces(arena, self.before)),
                ("item", self.item.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub enum Header<'a> {
//     Module(ModuleHeader<'a>),
//     App(AppHeader<'a>),
//     Package(PackageHeader<'a>),
//     Platform(PlatformHeader<'a>),
//     Hosted(HostedHeader<'a>),
// }

impl Nodify for Header<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            Header::Module(header) => Node::NamedStruct {
                name: "Module",
                fields: arena.alloc_slice_copy(&[("header", header.nodify(arena))]),
            },
            Header::App(header) => Node::NamedStruct {
                name: "App",
                fields: arena.alloc_slice_copy(&[("header", header.nodify(arena))]),
            },
            Header::Package(header) => Node::NamedStruct {
                name: "Package",
                fields: arena.alloc_slice_copy(&[("header", header.nodify(arena))]),
            },
            Header::Platform(header) => Node::NamedStruct {
                name: "Platform",
                fields: arena.alloc_slice_copy(&[("header", header.nodify(arena))]),
            },
            Header::Hosted(header) => Node::NamedStruct {
                name: "Hosted",
                fields: arena.alloc_slice_copy(&[("header", header.nodify(arena))]),
            },
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct ModuleHeader<'a> {
//     pub after_keyword: &'a [CommentOrNewline<'a>],
//     pub params: Option<ModuleParams<'a>>,
//     pub exposes: Collection<'a, Loc<Spaced<'a, ExposedName<'a>>>>,

//     // Keeping this so we can format old interface header into module headers
//     pub interface_imports: Option<KeywordItem<'a, ImportsKeyword, ImportsCollection<'a>>>,
// }

impl Nodify for ModuleHeader<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "ModuleHeader",
            fields: arena.alloc_slice_copy(&[
                ("after_keyword", nodify_spaces(arena, self.after_keyword)),
                (
                    "params",
                    match &self.params {
                        Some(params) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[params.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
                ("exposes", self.exposes.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct ModuleParams<'a> {
//     pub pattern: Loc<Collection<'a, Loc<Pattern<'a>>>>,
//     pub before_arrow: &'a [CommentOrNewline<'a>],
//     pub after_arrow: &'a [CommentOrNewline<'a>],
// }

impl Nodify for ModuleParams<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "ModuleParams",
            fields: arena.alloc_slice_copy(&[
                ("pattern", self.pattern.nodify(arena)),
                ("before_arrow", nodify_spaces(arena, self.before_arrow)),
                ("after_arrow", nodify_spaces(arena, self.after_arrow)),
            ]),
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct HostedHeader<'a> {
//     pub before_name: &'a [CommentOrNewline<'a>],
//     pub name: Loc<ModuleName<'a>>,
//     pub exposes: KeywordItem<'a, ExposesKeyword, Collection<'a, Loc<Spaced<'a, ExposedName<'a>>>>>,

//     pub imports: KeywordItem<'a, ImportsKeyword, Collection<'a, Loc<Spaced<'a, ImportsEntry<'a>>>>>,
// }

impl Nodify for HostedHeader<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "HostedHeader",
            fields: arena.alloc_slice_copy(&[
                ("before_name", nodify_spaces(arena, self.before_name)),
                ("name", self.name.nodify(arena)),
                ("exposes", self.exposes.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Copy, Clone, Debug, PartialEq, Eq)]
// pub enum To<'a> {
//     ExistingPackage(&'a str),
//     NewPackage(PackageName<'a>),
// }

impl Nodify for To<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            To::ExistingPackage(name) => Node::NamedTuple {
                name: "ExistingPackage",
                children: arena.alloc_slice_copy(&[Node::Str(arena.alloc_str(name))]),
            },
            To::NewPackage(name) => Node::NamedStruct {
                name: "NewPackage",
                fields: arena.alloc_slice_copy(&[("name", name.nodify(arena))]),
            },
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct AppHeader<'a> {
//     pub before_provides: &'a [CommentOrNewline<'a>],
//     pub provides: Collection<'a, Loc<Spaced<'a, ExposedName<'a>>>>,
//     pub before_packages: &'a [CommentOrNewline<'a>],
//     pub packages: Loc<Collection<'a, Loc<Spaced<'a, PackageEntry<'a>>>>>,
//     // Old header pieces
//     pub old_imports: Option<KeywordItem<'a, ImportsKeyword, ImportsCollection<'a>>>,
//     pub old_provides_to_new_package: Option<PackageName<'a>>,
// }

impl Nodify for AppHeader<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "AppHeader",
            fields: arena.alloc_slice_copy(&[
                (
                    "before_provides",
                    nodify_spaces(arena, self.before_provides),
                ),
                ("provides", self.provides.nodify(arena)),
                (
                    "before_packages",
                    nodify_spaces(arena, self.before_packages),
                ),
                ("packages", self.packages.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Copy, Clone, Debug, PartialEq, Eq)]
// pub struct PackageEntry<'a> {
//     pub shorthand: &'a str,
//     pub spaces_after_shorthand: &'a [CommentOrNewline<'a>],
//     pub platform_marker: Option<&'a [CommentOrNewline<'a>]>,
//     pub package_name: Loc<PackageName<'a>>,
// }

impl Nodify for PackageEntry<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "PackageEntry",
            fields: arena.alloc_slice_copy(&[
                ("shorthand", Node::Str(arena.alloc_str(self.shorthand))),
                (
                    "spaces_after_shorthand",
                    nodify_spaces(arena, self.spaces_after_shorthand),
                ),
                (
                    "platform_marker",
                    match &self.platform_marker {
                        Some(marker) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[nodify_spaces(arena, marker)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
                ("package_name", self.package_name.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct ProvidesTo<'a> {
//     pub provides_keyword: Spaces<'a, ProvidesKeyword>,
//     pub entries: Collection<'a, Loc<Spaced<'a, ExposedName<'a>>>>,
//     pub types: Option<Collection<'a, Loc<Spaced<'a, UppercaseIdent<'a>>>>>,

//     pub to_keyword: Spaces<'a, ToKeyword>,
//     pub to: Loc<To<'a>>,
// }

impl Nodify for ProvidesTo<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "ProvidesTo",
            fields: arena.alloc_slice_copy(&[
                ("provides_keyword", self.provides_keyword.nodify(arena)),
                ("entries", self.entries.nodify(arena)),
                (
                    "types",
                    match &self.types {
                        Some(types) => Node::NamedTuple {
                            name: "Some",
                            children: arena.alloc_slice_copy(&[types.nodify(arena)]),
                        },
                        None => Node::NamedTuple {
                            name: "None",
                            children: &[],
                        },
                    },
                ),
                ("to_keyword", self.to_keyword.nodify(arena)),
                ("to", self.to.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct PackageHeader<'a> {
//     pub before_exposes: &'a [CommentOrNewline<'a>],
//     pub exposes: Collection<'a, Loc<Spaced<'a, ModuleName<'a>>>>,
//     pub before_packages: &'a [CommentOrNewline<'a>],
//     pub packages: Loc<Collection<'a, Loc<Spaced<'a, PackageEntry<'a>>>>>,
// }

impl Nodify for PackageHeader<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "PackageHeader",
            fields: arena.alloc_slice_copy(&[
                ("before_exposes", nodify_spaces(arena, self.before_exposes)),
                ("exposes", self.exposes.nodify(arena)),
                (
                    "before_packages",
                    nodify_spaces(arena, self.before_packages),
                ),
                ("packages", self.packages.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct PlatformRequires<'a> {
//     pub rigids: Collection<'a, Loc<Spaced<'a, UppercaseIdent<'a>>>>,
//     pub signatures: Collection<'a, Loc<Spaced<'a, TypedIdent<'a>>>>,
// }

impl Nodify for PlatformRequires<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "PlatformRequires",
            fields: arena.alloc_slice_copy(&[
                ("rigids", self.rigids.nodify(arena)),
                ("signatures", self.signatures.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Copy, Clone, Debug, PartialEq)]
// pub struct TypedIdent<'a> {
//     pub ident: Loc<&'a str>,
//     pub spaces_before_colon: &'a [CommentOrNewline<'a>],
//     pub ann: Loc<TypeAnnotation<'a>>,
// }

impl Nodify for TypedIdent<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "TypedIdent",
            fields: arena.alloc_slice_copy(&[
                ("ident", self.ident.nodify(arena)),
                (
                    "spaces_before_colon",
                    nodify_spaces(arena, self.spaces_before_colon),
                ),
                ("ann", self.ann.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Clone, Debug, PartialEq)]
// pub struct PlatformHeader<'a> {
//     pub before_name: &'a [CommentOrNewline<'a>],
//     pub name: Loc<PackageName<'a>>,

//     pub requires: KeywordItem<'a, RequiresKeyword, PlatformRequires<'a>>,
//     pub exposes: KeywordItem<'a, ExposesKeyword, Collection<'a, Loc<Spaced<'a, ModuleName<'a>>>>>,
//     pub packages:
//         KeywordItem<'a, PackagesKeyword, Collection<'a, Loc<Spaced<'a, PackageEntry<'a>>>>>,
//     pub imports: KeywordItem<'a, ImportsKeyword, Collection<'a, Loc<Spaced<'a, ImportsEntry<'a>>>>>,
//     pub provides:
//         KeywordItem<'a, ProvidesKeyword, Collection<'a, Loc<Spaced<'a, ExposedName<'a>>>>>,
// }

impl Nodify for PlatformHeader<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        Node::NamedStruct {
            name: "PlatformHeader",
            fields: arena.alloc_slice_copy(&[
                ("before_name", nodify_spaces(arena, self.before_name)),
                ("name", self.name.nodify(arena)),
                ("requires", self.requires.nodify(arena)),
                ("exposes", self.exposes.nodify(arena)),
                ("packages", self.packages.nodify(arena)),
                ("imports", self.imports.nodify(arena)),
                ("provides", self.provides.nodify(arena)),
            ]),
        }
    }
}

// #[derive(Copy, Clone, Debug, PartialEq)]
// pub enum ImportsEntry<'a> {
//     /// e.g. `Hello` or `Hello exposing [hello]` see roc-lang.org/examples/MultipleRocFiles/README.html
//     Module(
//         ModuleName<'a>,
//         Collection<'a, Loc<Spaced<'a, ExposedName<'a>>>>,
//     ),

//     /// e.g. `pf.Stdout` or `pf.Stdout exposing [line]`
//     Package(
//         &'a str,
//         ModuleName<'a>,
//         Collection<'a, Loc<Spaced<'a, ExposedName<'a>>>>,
//     ),

//     /// e.g "path/to/my/file.txt" as myFile : Str
//     IngestedFile(StrLiteral<'a>, Spaced<'a, TypedIdent<'a>>),
// }

impl Nodify for ImportsEntry<'_> {
    fn nodify<'a>(&self, arena: &'a Bump) -> Node<'a> {
        match self {
            ImportsEntry::Module(name, exposes) => Node::NamedStruct {
                name: "Module",
                fields: arena.alloc_slice_copy(&[
                    ("name", name.nodify(arena)),
                    ("exposes", exposes.nodify(arena)),
                ]),
            },
            ImportsEntry::Package(shorthand, name, exposes) => Node::NamedStruct {
                name: "Package",
                fields: arena.alloc_slice_copy(&[
                    ("shorthand", Node::Str(arena.alloc_str(shorthand))),
                    ("name", name.nodify(arena)),
                    ("exposes", exposes.nodify(arena)),
                ]),
            },
            ImportsEntry::IngestedFile(path, name) => Node::NamedStruct {
                name: "IngestedFile",
                fields: arena.alloc_slice_copy(&[
                    ("path", path.nodify(arena)),
                    ("name", name.nodify(arena)),
                ]),
            },
        }
    }
}

fn parse_full<'a>(input: &'a str, arena: &'a Bump) -> Result<Output<'a>, String> {
    let state = State::new(input.as_bytes());
    let min_indent = 0;
    let (_, header, state) = roc_parse::header::header()
        .parse(arena, state.clone(), min_indent)
        .map_err(|(_, fail)| format!("{:?}", SyntaxError::Header(fail)))?;

    let defs = Defs::default();
    let defs = parse_module_defs(arena, state, defs).map_err(|e| format!("{:?}", e))?;

    Ok(Output::Full(FullAst { header, defs }))
}

fn main() {}
