//! Port of `deadcode/visitor/usage_graph.py`: the definition-anchored usage
//! graph and its single reachability pass.
//!
//! An item is keyed by `(module basename, name)`. A *bare usage* -- a plain
//! name load such as `run()` -- is recorded as a source `(using module, name)`
//! together with the definitions whose body it appears in (its owners). A
//! source is live when it is a root (module-level code, a decorator, or the
//! body of an ignored definition) or when any of its owners is live; an item
//! is live when its name is a plain anchor (a non-bare usage such as an
//! attribute access) or when a live source references it.
//!
//! Usages that only unused definitions own therefore keep nothing alive,
//! which is what lets a whole cascade of dead code be found in one run.

use std::collections::{HashMap, HashSet, VecDeque};

/// `(module basename, name)`.
pub type ItemKey = (String, String);

/// Everything the visitor records for the reachability pass.
#[derive(Default)]
pub struct UsageGraph {
    /// Owner token -> the key of the definition it stands for.
    pub owner_keys: Vec<ItemKey>,
    /// Bare-usage source -> owner tokens of the definitions containing it.
    pub usage_owners: HashMap<ItemKey, HashSet<usize>>,
    /// Bare-usage sources with no owning definition.
    pub root_usage_sources: HashSet<ItemKey>,
    /// Module -> names it defines at module level (functions, classes,
    /// variables).
    pub module_definitions: HashMap<String, HashSet<String>>,
    /// Module -> statically known exports, absent when unknown.
    pub module_exports: HashMap<String, HashSet<String>>,
    /// Module -> modules it wildcard-imports.
    pub wildcard_imports: HashMap<String, HashSet<String>>,
    /// Module -> names it imports explicitly with `from x import name`.
    pub explicit_imports: HashMap<String, HashSet<String>>,
    /// Module -> local name -> `(source module, imported name)` it binds to.
    pub import_edges: HashMap<String, HashMap<String, HashSet<ItemKey>>>,
}

impl UsageGraph {
    /// Resolves a bare usage of `name` in module `importer` to the
    /// definitions it binds to, or `None` when that is unknown or ambiguous.
    fn resolve_bare_usage(&self, name: &str, importer: &str) -> Option<HashSet<ItemKey>> {
        let mut candidates: HashSet<ItemKey> = HashSet::new();
        let defines = |module: &str, name: &str| {
            self.module_definitions
                .get(module)
                .is_some_and(|names| names.contains(name))
        };
        let edges_of = |module: &str, name: &str| -> Vec<ItemKey> {
            self.import_edges
                .get(module)
                .and_then(|edges| edges.get(name))
                .map(|targets| targets.iter().cloned().collect())
                .unwrap_or_default()
        };

        if defines(importer, name) {
            candidates.insert((importer.to_string(), name.to_string()));
        }

        let mut visited: HashSet<ItemKey> = HashSet::new();
        let mut expanded: HashSet<ItemKey> = HashSet::new();
        let mut frontier = edges_of(importer, name);
        while let Some(pair) = frontier.pop() {
            if !visited.insert(pair.clone()) {
                // A cycle makes the usage unresolvable.
                return None;
            }
            let (edge_module, edge_name) = pair;
            // `from x import n` binds in `x.py` or in `x/__init__.py`, so every
            // hop also tries the `__init__` twin.
            let mut hops = vec![edge_module.clone()];
            if edge_module != "__init__" {
                hops.push("__init__".to_string());
            }
            for hop_module in hops {
                if !expanded.insert((hop_module.clone(), edge_name.clone())) {
                    continue;
                }
                if defines(&hop_module, &edge_name) {
                    candidates.insert((hop_module.clone(), edge_name.clone()));
                }
                frontier.extend(edges_of(&hop_module, &edge_name));
            }
        }

        if candidates.is_empty() {
            for wildcard_module in self.wildcard_imports.get(importer).into_iter().flatten() {
                // Unknown exports make the usage unresolvable.
                let exports = self.module_exports.get(wildcard_module)?;
                if exports.contains(name) {
                    candidates.insert((wildcard_module.clone(), name.to_string()));
                }
            }
        }

        // No candidate, or more than one: unresolvable or ambiguous.
        (candidates.len() == 1).then_some(candidates)
    }

    /// Returns the keys of every live item among `items`, given the plain
    /// anchors in `used_names`.
    pub fn live_item_keys(
        &self,
        items: &[ItemKey],
        used_names: &HashSet<String>,
    ) -> HashSet<ItemKey> {
        let mut items_by_name: HashMap<&str, HashSet<&ItemKey>> = HashMap::new();
        for key in items {
            items_by_name.entry(key.1.as_str()).or_default().insert(key);
        }

        let mut sources_of_key: HashMap<&ItemKey, HashSet<&ItemKey>> = HashMap::new();
        for (source, owners) in &self.usage_owners {
            for &owner in owners {
                if let Some(key) = self.owner_keys.get(owner) {
                    sources_of_key.entry(key).or_default().insert(source);
                }
            }
        }

        let no_names = HashSet::new();
        let references = |source: &ItemKey| -> HashSet<ItemKey> {
            let (using_module, name) = source;
            let mut targets: HashSet<ItemKey> = HashSet::from([source.clone()]);
            let wildcard_modules = self.wildcard_imports.get(using_module).unwrap_or(&no_names);
            let exports_known = self.explicit_imports.get(using_module).unwrap_or(&no_names);
            let resolved = self.resolve_bare_usage(name, using_module);
            for key in items_by_name.get(name.as_str()).into_iter().flatten() {
                let module = &key.0;
                if module == using_module {
                    continue;
                }
                if wildcard_modules.contains(module) {
                    let referenced = match self.module_exports.get(module) {
                        None => true,
                        Some(exports) => exports.contains(name) || exports_known.contains(name),
                    };
                    if referenced {
                        targets.insert((*key).clone());
                    }
                } else if resolved
                    .as_ref()
                    .is_none_or(|resolved| resolved.contains(*key))
                {
                    targets.insert((*key).clone());
                }
            }
            targets
        };

        let mut live_keys: HashSet<ItemKey> = HashSet::new();
        let mut live_sources: HashSet<ItemKey> = HashSet::new();
        // `true` marks an item key, `false` a usage source.
        let mut queue: VecDeque<(bool, ItemKey)> = VecDeque::new();

        for source in &self.root_usage_sources {
            live_sources.insert(source.clone());
            queue.push_back((false, source.clone()));
        }
        for key in items {
            if used_names.contains(&key.1) && live_keys.insert(key.clone()) {
                queue.push_back((true, key.clone()));
            }
        }

        while let Some((is_key, entry)) = queue.pop_front() {
            if is_key {
                for &source in sources_of_key.get(&entry).into_iter().flatten() {
                    if live_sources.insert(source.clone()) {
                        queue.push_back((false, source.clone()));
                    }
                }
            } else {
                for target in references(&entry) {
                    if live_keys.insert(target.clone()) {
                        queue.push_back((true, target));
                    }
                }
            }
        }

        live_keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(module: &str, name: &str) -> ItemKey {
        (module.to_string(), name.to_string())
    }

    #[test]
    fn usages_owned_only_by_dead_definitions_keep_nothing_alive() {
        let mut graph = UsageGraph::default();
        // `def a(): return b()` -- `b` is used only inside unused `a`.
        graph.owner_keys.push(key("m", "a"));
        graph.usage_owners.insert(key("m", "b"), HashSet::from([0]));
        let items = [key("m", "a"), key("m", "b")];
        assert!(graph.live_item_keys(&items, &HashSet::new()).is_empty());
    }

    #[test]
    fn a_resolved_import_keeps_only_its_own_target_alive() {
        let mut graph = UsageGraph::default();
        graph.root_usage_sources.insert(key("c", "run"));
        graph
            .module_definitions
            .insert("a".into(), HashSet::from(["run".into()]));
        graph
            .module_definitions
            .insert("b".into(), HashSet::from(["run".into()]));
        graph
            .import_edges
            .entry("c".into())
            .or_default()
            .insert("run".into(), HashSet::from([key("a", "run")]));
        let items = [key("a", "run"), key("b", "run"), key("c", "run")];
        let live = graph.live_item_keys(&items, &HashSet::new());
        assert!(live.contains(&key("a", "run")));
        assert!(!live.contains(&key("b", "run")));
    }

    #[test]
    fn an_unresolvable_usage_keeps_every_same_named_item_alive() {
        let mut graph = UsageGraph::default();
        graph.root_usage_sources.insert(key("c", "run"));
        let items = [key("a", "run"), key("b", "run")];
        let live = graph.live_item_keys(&items, &HashSet::new());
        assert!(live.contains(&key("a", "run")) && live.contains(&key("b", "run")));
    }
}
