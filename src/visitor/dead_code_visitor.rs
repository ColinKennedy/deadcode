//! Port of `deadcode/visitor/dead_code_visitor.py`.
//!
//! Structural differences from the Python original, all deliberate (see
//! `RUST_PORT_PLAN.md`):
//! - No generic `ast.iter_fields`-style reflection: an exhaustive `match`
//!   over `ruff_python_ast`'s `Stmt`/`Expr` enums, monomorphized, no dynamic
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

use ruff_python_ast::{
    Alias, Decorator, Expr, ExprContext, Identifier, InterpolatedStringElement, Keyword, Operator,
    Parameters, Pattern, Stmt,
};
use ruff_text_size::{Ranged, TextSize};

use crate::actions::parse_abstract_syntax_tree::parse_abstract_syntax_tree;
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

    /// Files that could not be parsed and were therefore skipped entirely.
    ///
    /// Skipping is silent as far as the *findings* go — an unanalysable file
    /// simply contributes nothing — which meant a run over unparseable source
    /// was indistinguishable from a clean one. Recording them lets the CLI
    /// report a summary and fail the run instead.
    pub parse_failures: Vec<PathBuf>,

    used_names: HashSet<String>,

    filename: PathBuf,
    scope_parts: Vec<String>,
    scope_kinds: Vec<ScopeKind>,
    should_ignore_new_definitions: bool,

    noqa_lines: std::collections::HashMap<String, HashSet<u32>>,
    scopes: NestedScope,
    line_index: LineIndex,
    /// Source of the file currently being visited. Needed to recover the
    /// `def`/`class` keyword offset of a decorated definition — see
    /// `definition_keyword_start`.
    source: String,
}

/// Extracts a `str` constant value, mirroring the several
/// `isinstance(x, ast.Str)` checks in the Python original.
///
/// ruff models string literals as their own `Expr::StringLiteral` variant
/// rather than folding every literal into one `Constant` node, so this is a
/// direct variant test instead of a value-kind test. Implicitly concatenated
/// literals (`"a" "b"`) are joined by `to_str()`, matching what CPython's
/// parser hands `ast.Str`.
fn as_str_constant(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str()),
        _ => None,
    }
}

fn is_locals_call(node: &Expr) -> bool {
    if let Expr::Call(c) = node {
        if let Expr::Name(n) = c.func.as_ref() {
            return n.id.as_str() == "locals"
                && c.arguments.args.is_empty()
                && c.arguments.keywords.is_empty();
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
            parse_failures: Vec::new(),
            used_names: HashSet::new(),
            filename: PathBuf::new(),
            scope_parts: Vec::new(),
            scope_kinds: Vec::new(),
            should_ignore_new_definitions: false,
            noqa_lines: std::collections::HashMap::new(),
            scopes: NestedScope::new(),
            line_index: LineIndex::new(""),
            source: String::new(),
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
                self.source = content_str;

                let parsed = parse_abstract_syntax_tree(&self.source);
                match parsed {
                    Ok(module) => {
                        for stmt in &module {
                            self.walk_stmt(stmt);
                        }
                    }
                    Err(_) => {
                        // Deliberately NOT gated on `--count`/`--quiet`, unlike
                        // the Python original. Those flags exist to quiet the
                        // *findings* on stdout; suppressing this too is what
                        // let unparseable files vanish without a trace in CI.
                        // It goes to stderr, so `--count`'s machine-readable
                        // stdout stays clean.
                        eprintln!("Error: Failed to parse {file_path} file, ignoring it.");
                        self.parse_failures.push(PathBuf::from(file_path));
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

    /// Returns the offset Python's `ast` would report as a definition's
    /// `lineno`/`col_offset`.
    ///
    /// CPython (and `rustpython-parser`, which this was originally written
    /// against) place a decorated `def`/`class` node at the `def`/`async`/
    /// `class` keyword, keeping the decorators in a separate `decorator_list`
    /// whose own line is reported separately. ruff instead *starts* the
    /// node's range at the first decorator, so for a decorated definition the
    /// keyword offset has to be recovered by scanning forward from the end of
    /// the last decorator, past whitespace, comments and line continuations.
    ///
    /// Without this, a decorated definition is reported on its decorator's
    /// line, which also breaks `# noqa` lookup: the noqa comment sits on the
    /// `def` line, not the decorator's.
    fn definition_keyword_start(&self, node_start: TextSize, decorators: &[Decorator]) -> TextSize {
        let Some(last_decorator) = decorators.last() else {
            return node_start;
        };
        let bytes = self.source.as_bytes();
        let mut offset = usize::from(last_decorator.range().end());
        while offset < bytes.len() {
            match bytes[offset] {
                // A comment runs to the end of its line.
                b'#' => {
                    while offset < bytes.len() && bytes[offset] != b'\n' {
                        offset += 1;
                    }
                }
                b'\\' => offset += 1,
                byte if byte.is_ascii_whitespace() => offset += 1,
                // First real token after the decorators: the keyword.
                _ => break,
            }
        }
        TextSize::from(offset as u32)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_definition(
        &mut self,
        kind: DefinedKind,
        name: &str,
        start: TextSize,
        end: TextSize,
        decorator_list: &[Decorator],
        type_specific_ignored: bool,
        inherits_from: Option<Vec<String>>,
    ) {
        let type_ = kind.unused_code_type();
        let error_code = type_.error_code();

        // No-op for everything except decorated definitions.
        let start = self.definition_keyword_start(start, decorator_list);

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

    fn define_variable(&mut self, name: &str, start: TextSize, end: TextSize) {
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
            for arg in call.arguments.args.iter() {
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
            let effective_name = alias.asname.as_ref().map_or(name, |a| a.as_str());
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

    fn handle_name(&mut self, id: &str, ctx: ExprContext, start: TextSize, end: TextSize) {
        match ctx {
            ExprContext::Load | ExprContext::Del => {
                if !ignore::IGNORED_VARIABLE_NAMES.contains(&id) {
                    self.add_used_name(id);
                }
            }
            ExprContext::Store => {
                self.define_variable(id, start, end);
            }
            // ruff-only variant, used for error-recovery nodes in invalid
            // source. CPython's `ast` has no equivalent, so there is no
            // Python behavior to mirror — ignore it.
            ExprContext::Invalid => {}
        }
    }

    fn handle_attribute(
        &mut self,
        value: &Expr,
        attr: &Identifier,
        ctx: ExprContext,
        start: TextSize,
        end: TextSize,
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
            ExprContext::Del | ExprContext::Invalid => {}
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
        parameters: &Parameters,
        decorator_list: &[Decorator],
        start: TextSize,
        end: TextSize,
    ) {
        let decorator_names: Vec<String> = decorator_list
            .iter()
            .map(|d| get_decorator_name(&d.expression))
            .collect();
        for decorator in decorator_list {
            self.track_usefixtures_mark(&decorator.expression);
        }

        // Deliberately `args` only, not `posonlyargs`: Python checks
        // `node.args.args[0].arg == 'self'`, and CPython's `ast` (like ruff's)
        // keeps positional-only parameters in a separate list. So `def m(self, /)`
        // is not treated as a method by the original either — preserved here
        // rather than "fixed", per the exact-parity rule for this port.
        let first_arg = parameters.args.first().map(|a| a.parameter.name.as_str());

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
                // ruff groups what Python keeps as parallel `kwd_attrs` /
                // `kwd_patterns` lists into one `keywords` list of
                // (attr, pattern) pairs; same traversal, same order.
                for keyword in &p.arguments.keywords {
                    self.add_used_name(keyword.attr.as_str());
                }
                self.walk_expr(&p.cls);
                for pat in &p.arguments.patterns {
                    self.handle_match_pattern(pat);
                }
                for keyword in &p.arguments.keywords {
                    self.handle_match_pattern(&keyword.pattern);
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
                // `bases()`/`keywords()` return empty slices when the class
                // has no argument list at all (`class C:`), matching Python's
                // always-present-but-empty `bases`/`keywords` fields.
                let inherits_from = self.compute_inherits_from(c.bases());

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
                    self.track_usefixtures_mark(&decorator.expression);
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
                    self.walk_expr(&decorator.expression);
                }
                for base in c.bases() {
                    self.walk_expr(base);
                }
                for kw in c.keywords() {
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
            // Covers both `def` and `async def`: ruff models them as one node
            // with an `is_async` flag, where Python/rustpython had separate
            // `FunctionDef`/`AsyncFunctionDef` nodes. The two arms here were
            // byte-identical, and nothing in this visitor branches on
            // asyncness, so collapsing them is behavior-preserving.
            Stmt::FunctionDef(f) => {
                self.walk_function_like(
                    &f.name,
                    &f.parameters,
                    &f.decorator_list,
                    f.range().start(),
                    f.range().end(),
                );
                self.scope_parts.push(f.name.as_str().to_string());
                self.scope_kinds.push(ScopeKind::Function);
                for decorator in &f.decorator_list {
                    self.walk_expr(&decorator.expression);
                }
                self.walk_parameters(&f.parameters);
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
            // `for` and `async for` (merged in ruff behind `is_async`).
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
                // Python nests each `elif` as another `If` inside `orelse`,
                // so recursion reached every branch's test and body. ruff
                // flattens the whole chain into one clause list instead
                // (`test: None` marks the trailing `else`) — iterating it
                // visits exactly the same nodes.
                for clause in &s.elif_else_clauses {
                    if let Some(test) = &clause.test {
                        self.walk_expr(test);
                    }
                    for st in &clause.body {
                        self.walk_stmt(st);
                    }
                }
            }
            // `with` and `async with` (merged in ruff behind `is_async`).
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
            // `try`/`except` and `try`/`except*` (merged in ruff behind
            // `is_star`). Also covers PEP 758's parenthesis-free
            // `except A, B:` form, which parses into the same handler shape.
            Stmt::Try(s) => {
                for st in &s.body {
                    self.walk_stmt(st);
                }
                for handler in &s.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(h) = handler;
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
            // Jupyter-only (`%magic`, `!shell`). deadcode only ever parses
            // `.py` files in `Mode::Module`, so this is unreachable in
            // practice; ignored rather than panicking.
            Stmt::IpyEscapeCommand(_) => {}
        }
    }

    fn walk_parameters(&mut self, parameters: &Parameters) {
        for a in parameters
            .posonlyargs
            .iter()
            .chain(parameters.args.iter())
            .chain(parameters.kwonlyargs.iter())
        {
            if let Some(annotation) = &a.parameter.annotation {
                self.walk_expr(annotation);
            }
            if let Some(default) = &a.default {
                self.walk_expr(default);
            }
        }
        if let Some(vararg) = &parameters.vararg {
            if let Some(annotation) = &vararg.annotation {
                self.walk_expr(annotation);
            }
        }
        if let Some(kwarg) = &parameters.kwarg {
            if let Some(annotation) = &kwarg.annotation {
                self.walk_expr(annotation);
            }
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Name(n) => {
                self.handle_name(n.id.as_str(), n.ctx, n.range().start(), n.range().end());
            }
            Expr::Attribute(a) => {
                self.handle_attribute(&a.value, &a.attr, a.ctx, a.range().start(), a.range().end());
                self.walk_expr(&a.value);
            }
            Expr::Call(c) => {
                self.handle_call(&c.func, &c.arguments.args, &c.arguments.keywords);
                self.walk_expr(&c.func);
                for arg in c.arguments.args.iter() {
                    self.walk_expr(arg);
                }
                for kw in &c.arguments.keywords {
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
            Expr::Named(n) => {
                self.walk_expr(&n.target);
                self.walk_expr(&n.value);
            }
            Expr::Lambda(l) => {
                // `parameters` is `None` for a bare `lambda: ...`, where
                // Python still supplies an empty `arguments` node.
                if let Some(parameters) = &l.parameters {
                    self.walk_parameters(parameters);
                }
                self.walk_expr(&l.body);
            }
            Expr::If(e) => {
                self.walk_expr(&e.test);
                self.walk_expr(&e.body);
                self.walk_expr(&e.orelse);
            }
            Expr::Dict(d) => {
                // ruff pairs keys with values in one `items` list where
                // Python has parallel `keys`/`values` lists; a `None` key is
                // `**expansion`, matching Python's `None` key entry. Keys are
                // walked before values to preserve the original visit order.
                for item in &d.items {
                    if let Some(key) = &item.key {
                        self.walk_expr(key);
                    }
                }
                for item in &d.items {
                    self.walk_expr(&item.value);
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
                // `key` is optional only to represent invalid source that
                // ruff recovered from; a well-formed dict comprehension
                // always has one.
                if let Some(key) = &c.key {
                    self.walk_expr(key);
                }
                self.walk_expr(&c.value);
                self.walk_comprehensions(&c.generators);
            }
            Expr::Generator(c) => {
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
            // Python nests a `FormattedValue` inside a `JoinedStr`, both of
            // which are plain `expr` nodes. ruff instead models an f-string as
            // a list of parts (to support PEP 701 and implicit concatenation),
            // so the interpolations have to be pulled out explicitly. Walking
            // them is what makes a name used only inside an f-string —
            // `f"{some_var}"` — count as a usage.
            Expr::FString(f) => {
                for element in f.value.elements() {
                    self.walk_interpolated_element(element);
                }
            }
            // PEP 750 t-strings (Python 3.14). Structurally identical to an
            // f-string for our purposes: the interpolations are real
            // expressions and the names in them are genuine usages.
            Expr::TString(t) => {
                for element in t.value.elements() {
                    self.walk_interpolated_element(element);
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
            // Python's single `ast.Constant` node, split by ruff into one
            // variant per literal kind. None of them contain sub-expressions
            // or names, so all are terminal — same as the old
            // `Expr::Constant(_) => {}`.
            Expr::StringLiteral(_)
            | Expr::BytesLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::BooleanLiteral(_)
            | Expr::NoneLiteral(_)
            | Expr::EllipsisLiteral(_) => {}
            // Jupyter-only; see the `Stmt::IpyEscapeCommand` arm.
            Expr::IpyEscapeCommand(_) => {}
        }
    }

    /// Walks one f-string/t-string element. Literal chunks hold no names;
    /// interpolations hold a real expression plus an optional format spec,
    /// which can itself contain further interpolations (`f"{x:{width}}"`).
    fn walk_interpolated_element(&mut self, element: &InterpolatedStringElement) {
        match element {
            InterpolatedStringElement::Interpolation(interpolation) => {
                self.walk_expr(&interpolation.expression);
                if let Some(spec) = &interpolation.format_spec {
                    for nested in &spec.elements {
                        self.walk_interpolated_element(nested);
                    }
                }
            }
            InterpolatedStringElement::Literal(_) => {}
        }
    }

    fn walk_comprehensions(&mut self, generators: &[ruff_python_ast::Comprehension]) {
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
