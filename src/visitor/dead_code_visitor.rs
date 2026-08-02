//! Port of `deadcode/visitor/dead_code_visitor.py`.
//!
//! Structural differences from the Python original, all deliberate (see
//! `RUST_PORT_PLAN.md`):
//! - No generic `ast.iter_fields`-style reflection: an exhaustive `match`
//!   over rustpython_ast's `Stmt`/`Expr` enums, monomorphized, no dynamic
//!   dispatch.
//! - `unreachable_code` (DC09) tracking is NOT implemented: tracing
//!   `get_unused_code_items()` in the Python source shows that collection is
//!   computed but explicitly excluded from the output
//!   (`# TODO: removal of unreachable_code has a lot of edge cases`), and no
//!   test suppresses or asserts on it being reported. It has zero effect on
//!   any observable behavior, so porting it would just be dead weight.
//! - `ignore_decorators` (the `--ignore-*-if-decorated-with` flags) is a
//!   confirmed no-op in the Python original (hardcoded to an empty list in
//!   `__init__`, see `FOLLOWUP.local.md`) — preserved as a no-op here too,
//!   not "fixed", for exact parity.
//! - `NestedScope` only tracks classes (see `utils/nested_scopes.rs` for why
//!   that's provably behavior-preserving), so there's no `mark_as_used`/
//!   `number_of_uses` machinery here at all.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rustpython_ast::{
    Alias, Arguments, Constant, Expr, ExprContext, Identifier, Keyword, Operator, Pattern, Ranged,
    Stmt,
};
use rustpython_parser::{ast, Parse};

use crate::actions::parse_tach_config::TachIndex;
use crate::constants::UnusedCodeType;
use crate::data_types::{Args, Part};
use crate::utils::fnmatch;
use crate::utils::line_index::LineIndex;
use crate::utils::nested_scopes::NestedScope;
use crate::visitor::code_item::{path_as_posix, CodeItem};
use crate::visitor::ignore;
use crate::visitor::noqa;
use crate::visitor::utils::get_decorator_name;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Class,
    Function,
}

#[derive(Clone, Copy)]
enum DefinedKind {
    Attribute,
    Class,
    Function,
    Import,
    Method,
    Property,
    Variable,
}

impl DefinedKind {
    fn unused_code_type(self) -> UnusedCodeType {
        match self {
            DefinedKind::Attribute => UnusedCodeType::Attribute,
            DefinedKind::Class => UnusedCodeType::Class,
            DefinedKind::Function => UnusedCodeType::Function,
            DefinedKind::Import => UnusedCodeType::Import,
            DefinedKind::Method => UnusedCodeType::Method,
            DefinedKind::Property => UnusedCodeType::Property,
            DefinedKind::Variable => UnusedCodeType::Variable,
        }
    }
}

pub struct DeadCodeVisitor<'a> {
    args: &'a Args,
    tach_index: &'a TachIndex,

    defined_attrs: Vec<CodeItem>,
    defined_classes: Vec<CodeItem>,
    defined_funcs: Vec<CodeItem>,
    defined_imports: Vec<CodeItem>,
    defined_methods: Vec<CodeItem>,
    defined_props: Vec<CodeItem>,
    defined_vars: Vec<CodeItem>,
    pub unused_file: Vec<CodeItem>,

    used_names: HashSet<String>,

    filename: PathBuf,
    scope_parts: Vec<String>,
    scope_kinds: Vec<ScopeKind>,
    should_ignore_new_definitions: bool,

    noqa_lines: std::collections::HashMap<String, HashSet<u32>>,
    scopes: NestedScope,
    line_index: LineIndex,
}

/// Extracts a `str` constant value (`ast.Str`-equivalent under modern
/// `ast.Constant`), mirroring the several `isinstance(x, ast.Str)` checks in
/// the Python original.
fn as_str_constant(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Constant(c) => match &c.value {
            Constant::Str(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn is_locals_call(node: &Expr) -> bool {
    if let Expr::Call(c) = node {
        if let Expr::Name(n) = c.func.as_ref() {
            return n.id.as_str() == "locals" && c.args.is_empty() && c.keywords.is_empty();
        }
    }
    false
}

/// Extracts `{field_name}`-style placeholders from a `str.format()` template,
/// approximating `string.Formatter().parse()` closely enough for identifier
/// extraction (handles `{{`/`}}` escapes and strips `!conversion`/
/// `:format_spec` suffixes). Not exercised by any test in the current suite —
/// lower-confidence port, documented as such.
fn parse_format_fields(s: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => i += 2,
            '}' if chars.get(i + 1) == Some(&'}') => i += 2,
            '{' => {
                let start = i + 1;
                let mut depth = 1;
                let mut j = start;
                while j < chars.len() && depth > 0 {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        j += 1;
                    }
                }
                let field_full: String = chars[start..j.min(chars.len())].iter().collect();
                let field_name = field_full.split(['!', ':']).next().unwrap_or("");
                if !field_name.is_empty() {
                    fields.push(field_name.to_string());
                }
                i = j + 1;
            }
            _ => i += 1,
        }
    }
    fields
}

fn is_identifier_like(name: &str) -> bool {
    // Python: `bool(re.match(r'[a-zA-Z_][a-zA-Z0-9_]*', name))` — `re.match`
    // only anchors at the start, so this is really just "starts with a
    // letter/underscore" (the `*` quantifier always succeeds for the rest).
    matches!(name.chars().next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
}

impl<'a> DeadCodeVisitor<'a> {
    pub fn new(args: &'a Args, tach_index: &'a TachIndex) -> Self {
        DeadCodeVisitor {
            args,
            tach_index,
            defined_attrs: Vec::new(),
            defined_classes: Vec::new(),
            defined_funcs: Vec::new(),
            defined_imports: Vec::new(),
            defined_methods: Vec::new(),
            defined_props: Vec::new(),
            defined_vars: Vec::new(),
            unused_file: Vec::new(),
            used_names: HashSet::new(),
            filename: PathBuf::new(),
            scope_parts: Vec::new(),
            scope_kinds: Vec::new(),
            should_ignore_new_definitions: false,
            noqa_lines: std::collections::HashMap::new(),
            scopes: NestedScope::new(),
            line_index: LineIndex::new(""),
        }
    }

    pub fn visit_files(&mut self, filenames: &[String]) {
        for file_path in filenames {
            let Ok(file_content) = std::fs::read(file_path) else {
                continue;
            };
            let filename = Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path)
                .to_string();
            let module_name = filename
                .rsplit_once('.')
                .map(|(base, _)| base)
                .unwrap_or(&filename);
            self.scope_parts = vec![module_name.to_string()];
            self.scope_kinds = Vec::new();

            let is_dunder_file = filename.starts_with("__") && filename.ends_with("__.py");
            let content_str = String::from_utf8_lossy(&file_content).into_owned();
            let has_real_content = !content_str.trim().is_empty();

            if has_real_content || is_dunder_file {
                self.noqa_lines = noqa::parse_noqa(&file_content);
                self.filename = PathBuf::from(file_path);
                self.line_index = LineIndex::new(&content_str);

                match ast::Suite::parse(&content_str, file_path) {
                    Ok(module) => {
                        for stmt in &module {
                            self.walk_stmt(stmt);
                        }
                    }
                    Err(_) => {
                        if !self.args.count && !self.args.quiet {
                            eprintln!("Error: Failed to parse {file_path} file, ignoring it.");
                        }
                    }
                }
            } else {
                self.unused_file.push(CodeItem::new(
                    filename,
                    UnusedCodeType::UnusedFile,
                    PathBuf::from(file_path),
                    vec![],
                    None,
                    None,
                    None,
                    None,
                    "Empty file".to_string(),
                ));
            }
        }
    }

    pub fn get_unused_code_items(&self) -> Vec<CodeItem> {
        let mut unused: Vec<CodeItem> = Vec::new();
        unused.extend(unused_of(&self.defined_attrs, &self.used_names));
        unused.extend(unused_of(&self.defined_classes, &self.used_names));
        unused.extend(unused_of(&self.defined_funcs, &self.used_names));
        unused.extend(unused_of(&self.defined_imports, &self.used_names));
        unused.extend(unused_of(&self.defined_methods, &self.used_names));
        unused.extend(unused_of(&self.defined_props, &self.used_names));
        unused.extend(unused_of(&self.defined_vars, &self.used_names));
        // NOT self.unreachable_code -- see module doc comment.
        unused.extend(self.unused_file.iter().cloned());

        unused.sort_by(|a, b| {
            (&a.filename, a.name_line.unwrap_or(0)).cmp(&(&b.filename, b.name_line.unwrap_or(0)))
        });
        unused
    }

    fn add_used_name(&mut self, name: &str) {
        self.used_names.insert(name.to_string());
    }

    fn is_directly_in_class_body(&self) -> bool {
        self.scope_kinds.last() == Some(&ScopeKind::Class)
    }

    /// Port of `get_inherits_from`, restricted (like the caller already
    /// restricts it) to `ClassDef` nodes: returns `None` if `bases` is
    /// empty (matching Python's falsy-list check on the *raw* bases list,
    /// before filtering to `Name`-only bases).
    fn compute_inherits_from(&self, bases: &[Expr]) -> Option<Vec<String>> {
        if bases.is_empty() {
            return None;
        }
        let base_names: Vec<String> = bases
            .iter()
            .filter_map(|b| match b {
                Expr::Name(n) => Some(n.id.as_str().to_string()),
                _ => None,
            })
            .collect();
        let mut inherits_from = base_names.clone();
        for base in &base_names {
            if let Some(chain) = self.scopes.get_inherits_from(&self.scope_parts, base) {
                inherits_from.extend(chain.iter().cloned());
            }
        }
        Some(inherits_from)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_definition(
        &mut self,
        kind: DefinedKind,
        name: &str,
        start: rustpython_parser::text_size::TextSize,
        end: rustpython_parser::text_size::TextSize,
        decorator_list: &[Expr],
        type_specific_ignored: bool,
        inherits_from: Option<Vec<String>>,
    ) {
        let type_ = kind.unused_code_type();
        let error_code = type_.error_code();

        let first_line = match decorator_list.first() {
            Some(d) => self.line_index.line_col(d.range().start()).0 as u32,
            None => self.line_index.line_col(start).0 as u32,
        };
        let (name_line, name_col) = self.line_index.line_col(start);
        let (last_line, _) = self.line_index.line_col(end);
        let (_, end_col) = self.line_index.line_col(end);

        let ignored = type_specific_ignored
            || fnmatch::match_any(name, &self.args.ignore_names, true)
            || fnmatch::match_any(
                &path_as_posix(&self.filename),
                &self.args.ignore_names_in_files,
                true,
            )
            || self.should_ignore_new_definitions
            || noqa::ignore_line(&self.noqa_lines, name_line as u32, error_code)
            || self.tach_index.is_exposed(&self.filename, name);

        // Registered in the scope tree unconditionally (even if `ignored`) —
        // matches Python's `self.scopes.add(code_item)` running before the
        // ignored-check, so a later subclass can still resolve an ignored
        // base class's inherits-from chain.
        if matches!(kind, DefinedKind::Class) {
            if let Some(chain) = &inherits_from {
                self.scopes
                    .add_class(&self.scope_parts, name, chain.clone());
            }
        }

        if ignored {
            return;
        }

        let scope_string = if self.scope_parts.is_empty() {
            None
        } else {
            Some(self.scope_parts.join("."))
        };

        let code_item = CodeItem::new(
            name.to_string(),
            type_,
            self.filename.clone(),
            vec![Part::new(
                first_line,
                last_line as u32,
                name_col as u32,
                end_col as u32,
            )],
            scope_string,
            inherits_from,
            Some(name_line as u32),
            Some(name_col as u32),
            String::new(),
        );

        match kind {
            DefinedKind::Attribute => self.defined_attrs.push(code_item),
            DefinedKind::Class => self.defined_classes.push(code_item),
            DefinedKind::Function => self.defined_funcs.push(code_item),
            DefinedKind::Import => self.defined_imports.push(code_item),
            DefinedKind::Method => self.defined_methods.push(code_item),
            DefinedKind::Property => self.defined_props.push(code_item),
            DefinedKind::Variable => self.defined_vars.push(code_item),
        }
    }

    fn define_variable(
        &mut self,
        name: &str,
        start: rustpython_parser::text_size::TextSize,
        end: rustpython_parser::text_size::TextSize,
    ) {
        if self.args.ignore_class_attributes && self.is_directly_in_class_body() {
            return;
        }
        let ignored = ignore::ignore_variable(name);
        self.push_definition(DefinedKind::Variable, name, start, end, &[], ignored, None);
    }

    fn track_usefixtures_mark(&mut self, decorator: &Expr) {
        let name = get_decorator_name(decorator);
        if !ignore::PYTEST_USEFIXTURES_DECORATOR_NAMES.contains(&name.as_str()) {
            return;
        }
        if let Expr::Call(call) = decorator {
            for arg in &call.args {
                if let Some(s) = as_str_constant(arg) {
                    self.add_used_name(s);
                }
            }
        }
    }

    fn add_aliases(&mut self, names: &[Alias]) {
        for alias in names {
            let full_name = alias.name.as_str();
            let name = full_name.split('.').next().unwrap_or(full_name);
            let effective_name = alias.asname.as_deref().unwrap_or(name);
            let ignored = ignore::ignore_import(&self.filename, name);
            self.push_definition(
                DefinedKind::Import,
                effective_name,
                alias.range().start(),
                alias.range().end(),
                &[],
                ignored,
                None,
            );
            if alias.asname.is_some() {
                self.add_used_name(full_name);
            }
        }
    }

    fn handle_name(
        &mut self,
        id: &Identifier,
        ctx: ExprContext,
        start: rustpython_parser::text_size::TextSize,
        end: rustpython_parser::text_size::TextSize,
    ) {
        match ctx {
            ExprContext::Load | ExprContext::Del => {
                if !ignore::IGNORED_VARIABLE_NAMES.contains(&id.as_str()) {
                    self.add_used_name(id.as_str());
                }
            }
            ExprContext::Store => {
                self.define_variable(id.as_str(), start, end);
            }
        }
    }

    fn handle_attribute(
        &mut self,
        value: &Expr,
        attr: &Identifier,
        ctx: ExprContext,
        start: rustpython_parser::text_size::TextSize,
        end: rustpython_parser::text_size::TextSize,
    ) {
        match ctx {
            ExprContext::Store => {
                if self.args.ignore_non_self_attributes && !ignore::is_self_attribute_target(value)
                {
                    return;
                }
                self.push_definition(
                    DefinedKind::Attribute,
                    attr.as_str(),
                    start,
                    end,
                    &[],
                    false,
                    None,
                );
            }
            ExprContext::Load => {
                self.add_used_name(attr.as_str());
            }
            ExprContext::Del => {}
        }
    }

    fn handle_call(&mut self, func: &Expr, args: &[Expr], keywords: &[Keyword]) {
        if let Expr::Name(func_name) = func {
            let id = func_name.id.as_str();
            let matches_getattr = id == "getattr" && (2..=3).contains(&args.len());
            let matches_hasattr = id == "hasattr" && args.len() == 2;
            if matches_getattr || matches_hasattr {
                if let Some(s) = args.get(1).and_then(|a| as_str_constant(a)) {
                    self.add_used_name(s);
                }
            }
        }

        if let Expr::Attribute(attr) = func {
            if attr.attr.as_str() == "getfixturevalue" && args.len() == 1 {
                if let Some(s) = args.first().and_then(|a| as_str_constant(a)) {
                    self.add_used_name(s);
                }
            }

            if attr.attr.as_str() == "format" && as_str_constant(&attr.value).is_some() {
                let has_locals_kwarg = keywords
                    .iter()
                    .any(|kw| kw.arg.is_none() && is_locals_call(&kw.value));
                if has_locals_kwarg {
                    let s = as_str_constant(&attr.value).unwrap().to_string();
                    self.handle_new_format_string(&s);
                }
            }
        }
    }

    fn handle_new_format_string(&mut self, s: &str) {
        let bracket_re = regex::Regex::new(r"\[\w*\]").unwrap();
        for field_name in parse_format_fields(s) {
            let cleaned = bracket_re.replace_all(&field_name, "");
            for var in cleaned.split('.') {
                if is_identifier_like(var) {
                    self.add_used_name(var);
                }
            }
        }
    }

    fn handle_binop(&mut self, left: &Expr, op: &Operator, right: &Expr) {
        if let (Some(s), Operator::Mod) = (as_str_constant(left), op) {
            if is_locals_call(right) {
                static PERCENT_RE: once_cell::sync::Lazy<regex::Regex> =
                    once_cell::sync::Lazy::new(|| regex::Regex::new(r"%\((\w+)\)").unwrap());
                let names: Vec<String> = PERCENT_RE
                    .captures_iter(s)
                    .map(|c| c[1].to_string())
                    .collect();
                for name in names {
                    self.add_used_name(&name);
                }
            }
        }
    }

    fn walk_function_like(
        &mut self,
        name: &Identifier,
        args: &Arguments,
        decorator_list: &[Expr],
        start: rustpython_parser::text_size::TextSize,
        end: rustpython_parser::text_size::TextSize,
    ) {
        let decorator_names: Vec<String> = decorator_list.iter().map(get_decorator_name).collect();
        for decorator in decorator_list {
            self.track_usefixtures_mark(decorator);
        }

        let first_arg = args.args.first().map(|a| a.def.arg.as_str());

        let is_property = decorator_names.iter().any(|d| d == "@property");
        let is_method_by_decorator = decorator_names
            .iter()
            .any(|d| d == "@staticmethod" || d == "@classmethod");
        let is_method = is_method_by_decorator || first_arg == Some("self");

        let name_str = name.as_str();
        let filename = self.filename.clone();

        if ignore::ignore_pytest_fixture(&filename, &decorator_names) {
            // Suppressed entirely: not even scope-registered (matches Python
            // logging-only branch with no `_define` call).
        } else if ignore::ignore_override(&decorator_names) {
            // Suppressed entirely.
        } else if is_property {
            self.push_definition(
                DefinedKind::Property,
                name_str,
                start,
                end,
                decorator_list,
                false,
                None,
            );
        } else if is_method {
            let ignored = ignore::ignore_method(&filename, name_str);
            self.push_definition(
                DefinedKind::Method,
                name_str,
                start,
                end,
                decorator_list,
                ignored,
                None,
            );
        } else {
            let ignored = ignore::ignore_function(&filename, name_str);
            self.push_definition(
                DefinedKind::Function,
                name_str,
                start,
                end,
                decorator_list,
                ignored,
                None,
            );
        }
    }

    fn handle_match_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::MatchValue(p) => self.walk_expr(&p.value),
            Pattern::MatchSingleton(_) => {}
            Pattern::MatchSequence(p) => {
                for pat in &p.patterns {
                    self.handle_match_pattern(pat);
                }
            }
            Pattern::MatchMapping(p) => {
                for key in &p.keys {
                    self.walk_expr(key);
                }
                for pat in &p.patterns {
                    self.handle_match_pattern(pat);
                }
            }
            Pattern::MatchClass(p) => {
                for kwd_attr in &p.kwd_attrs {
                    self.add_used_name(kwd_attr.as_str());
                }
                self.walk_expr(&p.cls);
                for pat in &p.patterns {
                    self.handle_match_pattern(pat);
                }
                for pat in &p.kwd_patterns {
                    self.handle_match_pattern(pat);
                }
            }
            Pattern::MatchStar(_) => {}
            Pattern::MatchAs(p) => {
                if let Some(inner) = &p.pattern {
                    self.handle_match_pattern(inner);
                }
            }
            Pattern::MatchOr(p) => {
                for pat in &p.patterns {
                    self.handle_match_pattern(pat);
                }
            }
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::ClassDef(c) => {
                let inherits_from = self.compute_inherits_from(&c.bases);

                let mut should_turn_off = false;
                let matches_ignore_defs =
                    fnmatch::match_any(c.name.as_str(), &self.args.ignore_definitions, true)
                        || inherits_from.as_deref().is_some_and(|chain| {
                            fnmatch::match_many(
                                chain,
                                &self.args.ignore_definitions_if_inherits_from,
                                true,
                            )
                        });
                if matches_ignore_defs && !self.should_ignore_new_definitions {
                    self.should_ignore_new_definitions = true;
                    should_turn_off = true;
                }

                for decorator in &c.decorator_list {
                    self.track_usefixtures_mark(decorator);
                }
                self.push_definition(
                    DefinedKind::Class,
                    c.name.as_str(),
                    c.range().start(),
                    c.range().end(),
                    &c.decorator_list,
                    ignore::ignore_class(&self.filename, c.name.as_str()),
                    inherits_from.clone(),
                );

                self.scope_parts.push(c.name.as_str().to_string());
                self.scope_kinds.push(ScopeKind::Class);

                if !self.should_ignore_new_definitions {
                    if let Some(chain) = &inherits_from {
                        if fnmatch::match_many(
                            chain,
                            &self.args.ignore_bodies_if_inherits_from,
                            true,
                        ) {
                            self.should_ignore_new_definitions = true;
                            should_turn_off = true;
                        }
                    }
                }

                for decorator in &c.decorator_list {
                    self.walk_expr(decorator);
                }
                for base in &c.bases {
                    self.walk_expr(base);
                }
                for kw in &c.keywords {
                    self.walk_expr(&kw.value);
                }
                for s in &c.body {
                    self.walk_stmt(s);
                }

                if should_turn_off {
                    self.should_ignore_new_definitions = false;
                }
                self.scope_parts.pop();
                self.scope_kinds.pop();
            }
            Stmt::FunctionDef(f) => {
                self.walk_function_like(
                    &f.name,
                    &f.args,
                    &f.decorator_list,
                    f.range().start(),
                    f.range().end(),
                );
                self.scope_parts.push(f.name.as_str().to_string());
                self.scope_kinds.push(ScopeKind::Function);
                for decorator in &f.decorator_list {
                    self.walk_expr(decorator);
                }
                self.walk_arguments(&f.args);
                if let Some(returns) = &f.returns {
                    self.walk_expr(returns);
                }
                for s in &f.body {
                    self.walk_stmt(s);
                }
                self.scope_parts.pop();
                self.scope_kinds.pop();
            }
            Stmt::AsyncFunctionDef(f) => {
                self.walk_function_like(
                    &f.name,
                    &f.args,
                    &f.decorator_list,
                    f.range().start(),
                    f.range().end(),
                );
                self.scope_parts.push(f.name.as_str().to_string());
                self.scope_kinds.push(ScopeKind::Function);
                for decorator in &f.decorator_list {
                    self.walk_expr(decorator);
                }
                self.walk_arguments(&f.args);
                if let Some(returns) = &f.returns {
                    self.walk_expr(returns);
                }
                for s in &f.body {
                    self.walk_stmt(s);
                }
                self.scope_parts.pop();
                self.scope_kinds.pop();
            }
            Stmt::Import(i) => self.add_aliases(&i.names),
            Stmt::ImportFrom(i) => {
                if i.module.as_deref() != Some("__future__") {
                    self.add_aliases(&i.names);
                }
            }
            Stmt::Assign(a) => {
                if ignore::assigns_special_variable_all(&a.targets) {
                    let elts: &[Expr] = match a.value.as_ref() {
                        Expr::List(l) => &l.elts,
                        Expr::Tuple(t) => &t.elts,
                        _ => &[],
                    };
                    for elt in elts {
                        if let Some(s) = as_str_constant(elt) {
                            self.add_used_name(s);
                        }
                    }
                }
                for target in &a.targets {
                    self.walk_expr(target);
                }
                self.walk_expr(&a.value);
            }
            Stmt::AnnAssign(a) => {
                self.walk_expr(&a.target);
                self.walk_expr(&a.annotation);
                if let Some(v) = &a.value {
                    self.walk_expr(v);
                }
            }
            Stmt::AugAssign(a) => {
                self.walk_expr(&a.target);
                self.walk_expr(&a.value);
            }
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.walk_expr(v);
                }
            }
            Stmt::Delete(d) => {
                for t in &d.targets {
                    self.walk_expr(t);
                }
            }
            Stmt::For(s) => {
                self.walk_expr(&s.target);
                self.walk_expr(&s.iter);
                for st in &s.body {
                    self.walk_stmt(st);
                }
                for st in &s.orelse {
                    self.walk_stmt(st);
                }
            }
            Stmt::AsyncFor(s) => {
                self.walk_expr(&s.target);
                self.walk_expr(&s.iter);
                for st in &s.body {
                    self.walk_stmt(st);
                }
                for st in &s.orelse {
                    self.walk_stmt(st);
                }
            }
            Stmt::While(s) => {
                self.walk_expr(&s.test);
                for st in &s.body {
                    self.walk_stmt(st);
                }
                for st in &s.orelse {
                    self.walk_stmt(st);
                }
            }
            Stmt::If(s) => {
                self.walk_expr(&s.test);
                for st in &s.body {
                    self.walk_stmt(st);
                }
                for st in &s.orelse {
                    self.walk_stmt(st);
                }
            }
            Stmt::With(s) => {
                for item in &s.items {
                    self.walk_expr(&item.context_expr);
                    if let Some(v) = &item.optional_vars {
                        self.walk_expr(v);
                    }
                }
                for st in &s.body {
                    self.walk_stmt(st);
                }
            }
            Stmt::AsyncWith(s) => {
                for item in &s.items {
                    self.walk_expr(&item.context_expr);
                    if let Some(v) = &item.optional_vars {
                        self.walk_expr(v);
                    }
                }
                for st in &s.body {
                    self.walk_stmt(st);
                }
            }
            Stmt::Match(s) => {
                self.walk_expr(&s.subject);
                for case in &s.cases {
                    self.handle_match_pattern(&case.pattern);
                    if let Some(guard) = &case.guard {
                        self.walk_expr(guard);
                    }
                    for st in &case.body {
                        self.walk_stmt(st);
                    }
                }
            }
            Stmt::Raise(s) => {
                if let Some(exc) = &s.exc {
                    self.walk_expr(exc);
                }
                if let Some(cause) = &s.cause {
                    self.walk_expr(cause);
                }
            }
            Stmt::Try(s) => {
                for st in &s.body {
                    self.walk_stmt(st);
                }
                for handler in &s.handlers {
                    let rustpython_ast::ExceptHandler::ExceptHandler(h) = handler;
                    if let Some(t) = &h.type_ {
                        self.walk_expr(t);
                    }
                    for st in &h.body {
                        self.walk_stmt(st);
                    }
                }
                for st in &s.orelse {
                    self.walk_stmt(st);
                }
                for st in &s.finalbody {
                    self.walk_stmt(st);
                }
            }
            Stmt::TryStar(s) => {
                for st in &s.body {
                    self.walk_stmt(st);
                }
                for handler in &s.handlers {
                    let rustpython_ast::ExceptHandler::ExceptHandler(h) = handler;
                    if let Some(t) = &h.type_ {
                        self.walk_expr(t);
                    }
                    for st in &h.body {
                        self.walk_stmt(st);
                    }
                }
                for st in &s.orelse {
                    self.walk_stmt(st);
                }
                for st in &s.finalbody {
                    self.walk_stmt(st);
                }
            }
            Stmt::Assert(s) => {
                self.walk_expr(&s.test);
                if let Some(msg) = &s.msg {
                    self.walk_expr(msg);
                }
            }
            Stmt::Expr(s) => self.walk_expr(&s.value),
            Stmt::TypeAlias(s) => {
                self.walk_expr(&s.value);
            }
            Stmt::Global(_)
            | Stmt::Nonlocal(_)
            | Stmt::Pass(_)
            | Stmt::Break(_)
            | Stmt::Continue(_) => {}
        }
    }

    fn walk_arguments(&mut self, args: &Arguments) {
        for a in args
            .posonlyargs
            .iter()
            .chain(args.args.iter())
            .chain(args.kwonlyargs.iter())
        {
            if let Some(annotation) = &a.def.annotation {
                self.walk_expr(annotation);
            }
            if let Some(default) = &a.default {
                self.walk_expr(default);
            }
        }
        if let Some(vararg) = &args.vararg {
            if let Some(annotation) = &vararg.annotation {
                self.walk_expr(annotation);
            }
        }
        if let Some(kwarg) = &args.kwarg {
            if let Some(annotation) = &kwarg.annotation {
                self.walk_expr(annotation);
            }
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Name(n) => {
                self.handle_name(&n.id, n.ctx, n.range().start(), n.range().end());
            }
            Expr::Attribute(a) => {
                self.handle_attribute(&a.value, &a.attr, a.ctx, a.range().start(), a.range().end());
                self.walk_expr(&a.value);
            }
            Expr::Call(c) => {
                self.handle_call(&c.func, &c.args, &c.keywords);
                self.walk_expr(&c.func);
                for arg in &c.args {
                    self.walk_expr(arg);
                }
                for kw in &c.keywords {
                    self.walk_expr(&kw.value);
                }
            }
            Expr::BinOp(b) => {
                self.handle_binop(&b.left, &b.op, &b.right);
                self.walk_expr(&b.left);
                self.walk_expr(&b.right);
            }
            Expr::BoolOp(b) => {
                for v in &b.values {
                    self.walk_expr(v);
                }
            }
            Expr::UnaryOp(u) => self.walk_expr(&u.operand),
            Expr::NamedExpr(n) => {
                self.walk_expr(&n.target);
                self.walk_expr(&n.value);
            }
            Expr::Lambda(l) => {
                self.walk_arguments(&l.args);
                self.walk_expr(&l.body);
            }
            Expr::IfExp(e) => {
                self.walk_expr(&e.test);
                self.walk_expr(&e.body);
                self.walk_expr(&e.orelse);
            }
            Expr::Dict(d) => {
                for k in d.keys.iter().flatten() {
                    self.walk_expr(k);
                }
                for v in &d.values {
                    self.walk_expr(v);
                }
            }
            Expr::Set(s) => {
                for e in &s.elts {
                    self.walk_expr(e);
                }
            }
            Expr::ListComp(c) => {
                self.walk_expr(&c.elt);
                self.walk_comprehensions(&c.generators);
            }
            Expr::SetComp(c) => {
                self.walk_expr(&c.elt);
                self.walk_comprehensions(&c.generators);
            }
            Expr::DictComp(c) => {
                self.walk_expr(&c.key);
                self.walk_expr(&c.value);
                self.walk_comprehensions(&c.generators);
            }
            Expr::GeneratorExp(c) => {
                self.walk_expr(&c.elt);
                self.walk_comprehensions(&c.generators);
            }
            Expr::Await(a) => self.walk_expr(&a.value),
            Expr::Yield(y) => {
                if let Some(v) = &y.value {
                    self.walk_expr(v);
                }
            }
            Expr::YieldFrom(y) => self.walk_expr(&y.value),
            Expr::Compare(c) => {
                self.walk_expr(&c.left);
                for comparator in &c.comparators {
                    self.walk_expr(comparator);
                }
            }
            Expr::FormattedValue(f) => {
                self.walk_expr(&f.value);
                if let Some(spec) = &f.format_spec {
                    self.walk_expr(spec);
                }
            }
            Expr::JoinedStr(j) => {
                for v in &j.values {
                    self.walk_expr(v);
                }
            }
            Expr::Subscript(s) => {
                self.walk_expr(&s.value);
                self.walk_expr(&s.slice);
            }
            Expr::Starred(s) => self.walk_expr(&s.value),
            Expr::List(l) => {
                for e in &l.elts {
                    self.walk_expr(e);
                }
            }
            Expr::Tuple(t) => {
                for e in &t.elts {
                    self.walk_expr(e);
                }
            }
            Expr::Slice(s) => {
                if let Some(lower) = &s.lower {
                    self.walk_expr(lower);
                }
                if let Some(upper) = &s.upper {
                    self.walk_expr(upper);
                }
                if let Some(step) = &s.step {
                    self.walk_expr(step);
                }
            }
            Expr::Constant(_) => {}
        }
    }

    fn walk_comprehensions(&mut self, generators: &[rustpython_ast::Comprehension]) {
        for gen in generators {
            self.walk_expr(&gen.target);
            self.walk_expr(&gen.iter);
            for if_expr in &gen.ifs {
                self.walk_expr(if_expr);
            }
        }
    }
}

fn unused_of(items: &[CodeItem], used_names: &HashSet<String>) -> Vec<CodeItem> {
    let mut unused: Vec<CodeItem> = items
        .iter()
        .filter(|item| !used_names.contains(&item.name))
        .cloned()
        .collect();
    unused.sort_by_key(|item| item.name.to_lowercase());
    unused
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        args: Args,
        filenames: Vec<String>,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let mut filenames = Vec::new();
            for (name, content) in files {
                let path = dir.path().join(name);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(&path, content).unwrap();
                filenames.push(path.to_string_lossy().to_string());
            }
            Fixture {
                _dir: dir,
                args: Args::default(),
                filenames,
            }
        }

        fn run(&self) -> Vec<CodeItem> {
            let tach_index = TachIndex::new(vec![]);
            let mut visitor = DeadCodeVisitor::new(&self.args, &tach_index);
            visitor.visit_files(&self.filenames);
            visitor.get_unused_code_items()
        }
    }

    fn names(items: &[CodeItem]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn unused_variable_is_reported() {
        let fx = Fixture::new(&[("foo.py", "unused_var = 1\n")]);
        let items = fx.run();
        assert_eq!(names(&items), vec!["unused_var"]);
        assert_eq!(items[0].error_code(), "DC01");
        assert_eq!(items[0].name_line, Some(1));
        assert_eq!(items[0].name_column, Some(0));
    }

    #[test]
    fn used_variable_is_not_reported() {
        let fx = Fixture::new(&[("foo.py", "used_var = 1\nprint(used_var)\n")]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn unused_function_and_nested_closure() {
        let fx = Fixture::new(&[(
            "functions.py",
            "def unused_function():\n    if 2 > 1:\n        pass\n\n\ndef this_one_is_used():\n    pass\n\n\nthis_one_is_used()\n\n\ndef another_unused_function(arg1: str = '', arg2: str = 'Hello') -> None:\n    def this_is_unused_closure():\n        pass\n\n    print(arg1, arg2)\n",
        )]);
        let items = fx.run();
        let mut sorted = items.clone();
        sorted.sort_by_key(|i| i.name_line);
        assert_eq!(
            names(&sorted),
            vec![
                "unused_function",
                "another_unused_function",
                "this_is_unused_closure"
            ]
        );
        assert_eq!(sorted[0].name_line, Some(1));
        assert_eq!(sorted[1].name_line, Some(13));
        assert_eq!(sorted[2].name_line, Some(14));
        assert_eq!(sorted[2].name_column, Some(4));
    }

    #[test]
    fn unused_class_reported_used_class_and_dunder_init_not_reported() {
        let fx = Fixture::new(&[(
            "classes.py",
            "class UnusedClass(object):\n    pass\n\n\nclass ThisClassIsUsed:\n    pass\n\n\ninstance_of_a_used_class = ThisClassIsUsed()\nprint(instance_of_a_used_class)\n\n\nclass AnotherUnusedClass:\n    def __init__(self):\n        pass\n",
        )]);
        let items = fx.run();
        // Final ordering is by (filename, name_line) — not alphabetical —
        // matching Python's `get_unused_code_items` sort key exactly.
        assert_eq!(names(&items), vec!["UnusedClass", "AnotherUnusedClass"]);
    }

    #[test]
    fn cross_file_aliased_import_usage_is_tracked() {
        let fx = Fixture::new(&[
            ("foo.py", "used_var = None\n"),
            (
                "bar.py",
                "from foo import used_var as foo_var; print(foo_var)\n",
            ),
        ]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn module_attribute_usage_via_import() {
        let fx = Fixture::new(&[
            ("foo.py", "used_var = None\n"),
            ("bar.py", "import foo\nprint(foo.used_var)\n"),
        ]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn pytest_fixture_in_conftest_is_never_flagged() {
        let fx = Fixture::new(&[(
            "conftest.py",
            "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n",
        )]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn pytest_fixture_outside_pytest_location_is_flagged() {
        let fx = Fixture::new(&[(
            "myapp/helpers.py",
            "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n",
        )]);
        let items = fx.run();
        assert_eq!(names(&items), vec!["db"]);
        assert_eq!(items[0].error_code(), "DC02");
    }

    #[test]
    fn usefixtures_mark_counts_as_usage_across_files() {
        let fx = Fixture::new(&[
            ("myapp/helpers.py", "import pytest\n\n@pytest.fixture\ndef db():\n    return {}\n"),
            (
                "tests/test_foo.py",
                "import pytest\n\n@pytest.mark.usefixtures(\"db\")\ndef test_something():\n    pass\n",
            ),
        ]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn typing_override_method_is_never_flagged() {
        let fx = Fixture::new(&[(
            "myapp/foo.py",
            "import typing\n\nclass Foo(Base):\n    @typing.override\n    def method(self):\n        pass\n\nFoo()\n",
        )]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn method_without_override_decorator_is_flagged() {
        let fx = Fixture::new(&[(
            "myapp/foo.py",
            "class Foo(Base):\n    def method(self):\n        pass\n\nFoo()\n",
        )]);
        let items = fx.run();
        assert_eq!(names(&items), vec!["method"]);
        assert_eq!(items[0].error_code(), "DC04");
    }

    const INHERITS_FIXTURE: &str = "class UnusedClass(Base):\n    def __init__(self):\n        pass\n\n    def unused_method(self):\n        pass\n\n    unused_attribute = 1\n\n    class UnusedInnerClass:\n        pass\n\n\nclass Base:\n    pass\n\n\nclass AnotherUnusedClass:\n    def __init__(self):\n        pass\n\n    another_unused_attribute = 1\n";

    #[test]
    fn ignore_bodies_if_inherits_from_suppresses_only_inner_items() {
        let fx = Fixture::new(&[("foo.py", INHERITS_FIXTURE)]);
        let mut fx = fx;
        fx.args.ignore_bodies_if_inherits_from = vec!["Base".to_string()];
        let items = fx.run();
        // Outer UnusedClass is still reported+would-be-removed; its inner
        // members (unused_method, unused_attribute, UnusedInnerClass) are
        // NOT individually reported. AnotherUnusedClass (no inheritance
        // match) still has its inner attribute reported individually.
        // Line-number order (UnusedClass at line 1, before AnotherUnusedClass),
        // matching the real cataloged Python test's expected output order.
        assert_eq!(
            names(&items),
            vec![
                "UnusedClass",
                "AnotherUnusedClass",
                "another_unused_attribute"
            ]
        );
    }

    #[test]
    fn ignore_definitions_if_inherits_from_suppresses_whole_subtree() {
        let fx = Fixture::new(&[("foo.py", INHERITS_FIXTURE)]);
        let mut fx = fx;
        fx.args.ignore_definitions_if_inherits_from = vec!["Base".to_string()];
        let items = fx.run();
        // UnusedClass itself is NOT reported at all here (whole subtree
        // preserved), only AnotherUnusedClass + its attribute.
        assert_eq!(
            names(&items),
            vec!["AnotherUnusedClass", "another_unused_attribute"]
        );
    }

    #[test]
    fn ignore_definitions_hides_whole_subtree_by_exact_name() {
        let fx = Fixture::new(&[(
            "foo.py",
            "class UnusedClass:\n    def unused_method(self):\n        pass\n\n\nclass AnotherUnusedClass:\n    pass\n",
        )]);
        let mut fx = fx;
        fx.args.ignore_definitions = vec!["UnusedClass".to_string()];
        let items = fx.run();
        assert_eq!(names(&items), vec!["AnotherUnusedClass"]);
    }

    #[test]
    fn ignore_definitions_glob_pattern() {
        let fx = Fixture::new(&[("foo.py", "class UnusedClass:\n    pass\n")]);
        let mut fx = fx;
        fx.args.ignore_definitions = vec!["Unused*".to_string()];
        assert!(fx.run().is_empty());
    }

    #[test]
    fn multi_level_inheritance_chain_is_flattened() {
        let fx = Fixture::new(&[(
            "foo.py",
            "class Foo:\n    pass\n\n\nclass Bar(Foo):\n    pass\n\n\nclass Spam(Bar):\n    pass\n",
        )]);
        let mut fx = fx;
        fx.args.ignore_definitions_if_inherits_from = vec!["Foo".to_string()];
        // Bar directly inherits Foo -> should_ignore_new_definitions latches
        // at Bar and cascades to nested Spam... but Spam is a SIBLING of Bar,
        // not nested inside it, so it needs the NestedScope chain lookup
        // (Bar's own inherits_from=["Foo"] must be found when computing
        // Spam's inherits_from=["Bar", ...Bar's chain]).
        assert!(fx.run().is_empty());
    }

    #[test]
    fn noqa_specific_code_suppresses_matching_violation_only() {
        let fx = Fixture::new(&[("foo.py", "unused_variable = 1  # noqa: DC01\n")]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn noqa_wrong_code_does_not_suppress() {
        let fx = Fixture::new(&[("foo.py", "unused_variable = 1  # noqa: DC02\n")]);
        let items = fx.run();
        assert_eq!(names(&items), vec!["unused_variable"]);
    }

    #[test]
    fn dunder_all_marks_names_as_used() {
        let fx = Fixture::new(&[(
            "foo.py",
            "def helper():\n    pass\n\n__all__ = ['helper']\n",
        )]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn getattr_string_arg_counts_as_usage() {
        let fx = Fixture::new(&[(
            "foo.py",
            "class Foo:\n    bar = 1\n\nf = Foo()\ngetattr(f, 'bar')\n",
        )]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn self_attribute_reported_by_default_non_self_attribute_ignored_with_flag() {
        let fx = Fixture::new(&[(
            "foo.py",
            "class Foo:\n    def __init__(self):\n        self.baz = 1\n\nfoo = Foo()\nfoo.bar = 'thing'\n",
        )]);
        let items = fx.run();
        let mut item_names = names(&items);
        item_names.sort();
        assert_eq!(item_names, vec!["bar", "baz"]);

        let mut fx2 = Fixture::new(&[(
            "foo.py",
            "class Foo:\n    def __init__(self):\n        self.baz = 1\n\nfoo = Foo()\nfoo.bar = 'thing'\n",
        )]);
        fx2.args.ignore_non_self_attributes = true;
        let items2 = fx2.run();
        assert_eq!(names(&items2), vec!["baz"]);
    }

    #[test]
    fn empty_file_reported_as_dc11() {
        let fx = Fixture::new(&[("foo.py", "   \n\n")]);
        let items = fx.run();
        assert_eq!(names(&items), vec!["foo.py"]);
        assert_eq!(items[0].error_code(), "DC11");
    }

    #[test]
    fn dunder_init_file_never_treated_as_empty() {
        let fx = Fixture::new(&[("pkg/__init__.py", "")]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn syntax_error_is_silently_ignored_not_a_crash() {
        let fx = Fixture::new(&[("foo.py", "this is not valid python !!! ===\n")]);
        assert!(fx.run().is_empty());
    }

    #[test]
    fn comment_and_string_occurrences_do_not_count_as_usage() {
        let fx = Fixture::new(&[(
            "foo.py",
            "# unused_var mentioned in a comment\nunused_var = 1\nprint('unused_var mentioned in a string')\n",
        )]);
        let items = fx.run();
        assert_eq!(names(&items), vec!["unused_var"]);
        assert_eq!(items[0].name_line, Some(2));
    }
}
