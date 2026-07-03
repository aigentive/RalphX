import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class ProjectDocsContractTests(unittest.TestCase):
    def test_frontend_root_claude_doc_exists(self):
        frontend_doc = ROOT / "frontend" / "CLAUDE.md"
        self.assertTrue(
            frontend_doc.is_file(),
            "frontend/CLAUDE.md should exist so subtree doc probes do not fail",
        )
        text = frontend_doc.read_text(encoding="utf-8")
        self.assertIn("frontend/src/CLAUDE.md", text)

    def test_owner_strategy_docs_are_optional_probes(self):
        text = (ROOT / "CLAUDE.md").read_text(encoding="utf-8")
        self.assertIn("optional", text.lower())
        self.assertIn("non-failing", text.lower())
        self.assertIn("skip", text.lower())
        self.assertNotIn("agents MUST load these on demand", text)


if __name__ == "__main__":
    unittest.main()
