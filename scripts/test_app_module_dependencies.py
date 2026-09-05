#!/usr/bin/env python3
"""Regression checks for source filtering and genuine module dependency cycles."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "app_module_dependencies", Path(__file__).with_name("app-module-dependencies.py")
)
dependencies = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(dependencies)


class RustSourceTests(unittest.TestCase):
    def test_literals_comments_and_lifetimes(self):
        source = r'''
use crate::app::{alpha::Thing, beta::{self, Other as Renamed}};
/* outer use crate::app::false_block; /* nested */ still comment */
const MESSAGE: &str = "use a new ChangeSet {; escaped \" // not a comment";
const RAW: &str = r##"use crate::app::false_raw; "# /* text */"##;
const BYTES: &[u8] = br#"use crate::app::false_bytes;"#;
const C_STRING: &CStr = cr#"use crate::app::false_c;"#;
const CHAR: char = '\u{7b}';
fn with_lifetime<'a>(value: &'a str) { let quote = '\''; }
// use crate::app::false_line;
use crate::app::gamma::Real;
'''
        code = dependencies.rust_code(source)
        self.assertEqual(len(code), len(source))
        self.assertEqual(code.count("\n"), source.count("\n"))
        self.assertIn("with_lifetime<'a>(value: &'a str)", code)
        self.assertNotIn("false_", code)
        self.assertNotIn("ChangeSet", code)
        self.assertIn("gamma::Real", code)
        self.assertEqual(
            dependencies.expand_use_tree("crate::app::{alpha::Thing, beta::{self, Other as Renamed}}"),
            [("crate", "app", "alpha", "Thing"), ("crate", "app", "beta"), ("crate", "app", "beta", "Other")],
        )

    def test_comment_delimiters_inside_strings_do_not_eat_imports(self):
        code = dependencies.rust_code('let url = "https://example.test/*";\nuse crate::app::real;')
        self.assertIn("use crate::app::real;", code)

    def test_unterminated_literals_and_comments_fail_explicitly(self):
        for source in ['/* missing', '"missing', 'r##"missing"#']:
            with self.subTest(source=source), self.assertRaises(ValueError):
                dependencies.rust_code(source)

    def test_graph_ignores_prose_but_keeps_real_cycle(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "mod.rs").write_text("mod alpha; mod beta;")
            (root / "alpha.rs").write_text('let text = "use a new ChangeSet {;";\nuse super::beta::Thing;')
            (root / "beta.rs").write_text('/* use crate::app::bogus; */\nuse super::alpha::Other;')
            with patch.object(dependencies, "APP_ROOT", root):
                graph = dependencies.graph()
            self.assertEqual(graph["alpha"], {"beta"})
            self.assertEqual(graph["beta"], {"alpha"})
            cycles = [set(c) for c in dependencies.strongly_connected_components(graph) if len(c) > 1]
            self.assertEqual(cycles, [{"alpha", "beta"}])

    def test_inline_tests_resolve_super_from_their_actual_module(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "group").mkdir()
            (root / "mod.rs").write_text("mod group; mod other;")
            (root / "group/mod.rs").write_text("mod leaf;")
            (root / "other.rs").write_text("pub struct Other;")
            (root / "group/leaf.rs").write_text('''
fn helper() {}
#[cfg(test)] mod tests {
    use super::helper;
    mod nested { use super::super::super::super::other::Other; }
}
''')
            with patch.object(dependencies, "APP_ROOT", root):
                graph = dependencies.graph()
            self.assertEqual(graph["group::leaf"], {"other"})


if __name__ == "__main__":
    unittest.main()
