//! Static import discovery and compilation for the AOT backend.
//!
//! Only literal `import("path")` calls can be precompiled.  Modules are
//! compiled recursively at build time and stored in the program image.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::image::ImageRoot;
use crate::compiler::parser::ParserCtx;
use crate::compiler::scanner::ScannerCtx;
use crate::compiler::Token;
use crate::{Chunk, StringPool};

pub struct CompiledProgram {
    pub roots: Vec<ImageRoot>,
}

/// Compile the entry source and every statically imported module.
pub fn compile_program(entry_source: &str) -> Result<CompiledProgram, String> {
    let mut pool = StringPool::new();
    let mut roots = Vec::new();
    let mut visited = HashSet::new();

    compile_one("", entry_source, &mut pool, &mut roots, &mut visited)?;

    // Assign stable IDs in the same DFS order used by the image serializer and
    // by the runtime dispatcher.
    let mut next_id = 0usize;
    for root in roots.iter_mut() {
        assign_ids(&mut root.chunk, &mut next_id);
    }

    Ok(CompiledProgram { roots })
}

fn compile_one(
    import_path: &str,
    source: &str,
    pool: &mut StringPool,
    roots: &mut Vec<ImageRoot>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let mut scanner = ScannerCtx::new(source, pool);
    scanner.parse()?;
    let scanner_result = scanner.finish();
    let imports = collect_literal_imports(&scanner_result.tokens);

    let mut parser = ParserCtx::new(scanner_result, HashMap::new(), pool);
    parser.parse_prog()?;
    let parsed = parser.finish();

    roots.push(ImageRoot {
        path: import_path.to_string(),
        chunk: parsed.chunk,
    });

    for path in imports {
        let resolved = resolve_import(&path);
        let canonical = fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
        if !visited.insert(canonical.clone()) {
            // A cycle in the import graph.  The module will be entered from the
            // outer compile_one call, so stopping here is correct.
            continue;
        }
        let module_source = fs::read_to_string(&resolved).map_err(|e| {
            format!(
                "cannot read imported module `{}` ({}): {e}",
                path,
                resolved.display()
            )
        })?;
        compile_one(&path, &module_source, pool, roots, visited)?;
        visited.remove(&canonical);
    }
    Ok(())
}

fn resolve_import(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// Find `import("literal")` calls in a token stream.
fn collect_literal_imports(tokens: &[Token]) -> Vec<String> {
    let mut imports = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let is_import = matches!(
            &tokens[index],
            Token::Symbol(symbol) if symbol.get_inner() == "import"
        );
        if is_import && index + 3 < tokens.len() {
            if matches!(tokens[index + 1], Token::LParen) {
                if let Token::String(path) = &tokens[index + 2] {
                    if matches!(tokens[index + 3], Token::RParen) {
                        imports.push(path.get_inner().to_string());
                    }
                }
            }
        }
        index += 1;
    }
    imports
}

fn assign_ids(chunk: &mut Chunk, next_id: &mut usize) {
    chunk.native_id = *next_id;
    *next_id += 1;
    for child in chunk.chunks.iter_mut() {
        assign_ids(child, next_id);
    }
}
