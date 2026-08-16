//! Staged profiling harness for the Rust port, mirroring the exact stage
//! breakdown used to profile the Python implementation (file discovery, raw
//! I/O, noqa scanning, ast.parse, AST visiting, post-processing) so the two
//! are directly comparable. Throwaway/dev tool, not part of the shipped
//! binary.

use std::time::Instant;

use deadcode::actions::find_python_filenames::find_python_filenames;
use deadcode::actions::parse_abstract_syntax_tree::parse_abstract_syntax_tree;
use deadcode::actions::parse_tach_config::TachIndex;
use deadcode::data_types::Args;
use deadcode::visitor::dead_code_visitor::DeadCodeVisitor;
use deadcode::visitor::noqa::parse_noqa;

struct StageResult {
    files: usize,
    discovery: f64,
    io: f64,
    noqa: f64,
    parse: f64,
    combined: f64,
    visitor_est: f64,
    postproc: f64,
    total: f64,
    items: usize,
}

fn run_target(path: &str, cap: Option<usize>) -> StageResult {
    let args = Args {
        paths: vec![path.to_string()],
        // `target/` (Cargo's build dir: ~5,700 files, ~2.3GB) didn't exist
        // when this same "whole repo" target was profiled on the Python
        // version -- excluded here so the comparison stays apples-to-apples
        // instead of penalizing the Rust walk for artifacts Python's run
        // never had to traverse either.
        exclude: vec!["./target".to_string(), "target".to_string()],
        ..Default::default()
    };
    let tach_index = TachIndex::new(vec![]);

    let t0 = Instant::now();
    let mut filenames = find_python_filenames(&args, &tach_index);
    let discovery = t0.elapsed().as_secs_f64();

    if let Some(cap) = cap {
        filenames.truncate(cap);
    }

    // Stage 2: raw I/O only.
    let t0 = Instant::now();
    let contents: Vec<Vec<u8>> = filenames
        .iter()
        .map(|f| std::fs::read(f).unwrap_or_default())
        .collect();
    let io = t0.elapsed().as_secs_f64();

    // Stage 3: noqa scanning only.
    let t0 = Instant::now();
    for c in &contents {
        parse_noqa(c);
    }
    let noqa = t0.elapsed().as_secs_f64();

    // Stage 4: ast.parse only.
    let decoded: Vec<String> = contents
        .iter()
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect();
    let t0 = Instant::now();
    for src in decoded.iter() {
        let _ = parse_abstract_syntax_tree(src);
    }
    let parse = t0.elapsed().as_secs_f64();

    // Stage 5: combined parse+visit (the real pipeline call).
    let t0 = Instant::now();
    let mut visitor = DeadCodeVisitor::new(&args, &tach_index);
    visitor.visit_files(&filenames);
    let combined = t0.elapsed().as_secs_f64();
    let visitor_est = combined - parse;

    // Stage 6: post-processing.
    let t0 = Instant::now();
    let items = visitor.get_unused_code_items();
    let postproc = t0.elapsed().as_secs_f64();

    let total = discovery + io + noqa + parse + visitor_est + postproc;

    StageResult {
        files: filenames.len(),
        discovery,
        io,
        noqa,
        parse,
        combined,
        visitor_est,
        postproc,
        total,
        items: items.len(),
    }
}

fn report(name: &str, path: &str, cap: Option<usize>) {
    let r = run_target(path, cap);
    println!(
        "\n{}\nTARGET: {name} ({path})\n{}",
        "=".repeat(100),
        "=".repeat(100)
    );
    println!("Files: {}", r.files);
    println!("Unused items found: {}", r.items);
    println!("\n-- Stage timing --");
    println!("1. File discovery:        {:.4}s", r.discovery);
    println!("2. Raw file I/O:          {:.4}s", r.io);
    println!("3. noqa scanning:         {:.4}s", r.noqa);
    println!("4. ast.parse only:        {:.4}s", r.parse);
    println!(
        "5. Combined parse+visit:  {:.4}s  (visitor-only est.: {:.4}s)",
        r.combined, r.visitor_est
    );
    println!("6. Post-processing:       {:.4}s", r.postproc);
    println!("TOTAL:                    {:.4}s", r.total);

    let stages = [
        ("File discovery", r.discovery),
        ("Raw file I/O", r.io),
        ("noqa scanning", r.noqa),
        ("ast.parse", r.parse),
        ("AST visiting (visitor-only est.)", r.visitor_est),
        ("Post-processing", r.postproc),
    ];
    println!("\n{:<38}{:>12}{:>14}", "Stage", "Time (s)", "% of total");
    for (name, t) in stages {
        let pct = if r.total > 0.0 {
            t / r.total * 100.0
        } else {
            0.0
        };
        println!("{name:<38}{t:>12.4}{pct:>13.1}%");
    }
}

fn main() {
    report(
        "deadcode package (dogfooding, small)",
        "legacy-python/deadcode",
        None,
    );
    report("deadcode repo incl. tests", ".", None);
    report(
        "installed site-packages (large, capped)",
        ".venv/Lib/site-packages",
        Some(400),
    );
}
