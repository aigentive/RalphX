import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const SRC_ROOT = path.resolve(__dirname, "../..");
const CONTENT_COMPONENTS = new Set(["DialogContent", "AlertDialogContent"]);
const TITLE_COMPONENTS = new Set(["DialogTitle", "AlertDialogTitle"]);
const DESCRIPTION_COMPONENTS = new Set([
  "DialogDescription",
  "AlertDialogDescription",
]);
const TITLE_PROPS = new Set(["aria-label", "aria-labelledby"]);
const DESCRIPTION_PROPS = new Set(["aria-describedby"]);

function listTsxFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir)) {
    const fullPath = path.join(dir, entry);
    const stats = statSync(fullPath);
    if (stats.isDirectory()) {
      files.push(...listTsxFiles(fullPath));
    } else if (entry.endsWith(".tsx")) {
      files.push(fullPath);
    }
  }
  return files;
}

function getJsxName(
  name: ts.JsxTagNameExpression | ts.JsxNamespacedName,
): string | null {
  if (ts.isIdentifier(name)) {
    return name.text;
  }
  if (ts.isPropertyAccessExpression(name)) {
    return name.name.text;
  }
  if (ts.isJsxNamespacedName(name)) {
    return name.name.text;
  }
  return null;
}

function getAttributeNames(opening: ts.JsxOpeningLikeElement): Set<string> {
  const names = new Set<string>();
  for (const prop of opening.attributes.properties) {
    if (ts.isJsxAttribute(prop)) {
      names.add(prop.name.text);
    }
  }
  return names;
}

function hasAnyAttribute(
  opening: ts.JsxOpeningLikeElement,
  expected: Set<string>,
): boolean {
  const names = getAttributeNames(opening);
  return [...expected].some((name) => names.has(name));
}

function hasDescendantComponent(
  node: ts.Node,
  componentNames: Set<string>,
): boolean {
  let found = false;

  function visit(child: ts.Node): void {
    if (found) return;

    if (ts.isJsxElement(child)) {
      const name = getJsxName(child.openingElement.tagName);
      if (name && componentNames.has(name)) {
        found = true;
        return;
      }
    } else if (ts.isJsxSelfClosingElement(child)) {
      const name = getJsxName(child.tagName);
      if (name && componentNames.has(name)) {
        found = true;
        return;
      }
    }

    ts.forEachChild(child, visit);
  }

  ts.forEachChild(node, visit);
  return found;
}

function collectDialogContentViolations(): string[] {
  const violations: string[] = [];

  for (const filePath of listTsxFiles(SRC_ROOT)) {
    const sourceText = readFileSync(filePath, "utf8");
    const source = ts.createSourceFile(
      filePath,
      sourceText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );

    function checkContent(
      node: ts.JsxElement | ts.JsxSelfClosingElement,
      opening: ts.JsxOpeningLikeElement,
    ): void {
      const name = getJsxName(opening.tagName);
      if (!name || !CONTENT_COMPONENTS.has(name)) return;

      const hasTitle =
        hasAnyAttribute(opening, TITLE_PROPS) ||
        hasDescendantComponent(node, TITLE_COMPONENTS);
      const hasDescription =
        hasAnyAttribute(opening, DESCRIPTION_PROPS) ||
        hasDescendantComponent(node, DESCRIPTION_COMPONENTS);

      if (hasTitle && hasDescription) return;

      const { line, character } = source.getLineAndCharacterOfPosition(
        opening.getStart(source),
      );
      const relativePath = path.relative(path.dirname(SRC_ROOT), filePath);
      const missing = [
        !hasTitle ? "title" : null,
        !hasDescription ? "description or aria-describedby" : null,
      ]
        .filter(Boolean)
        .join(" and ");
      violations.push(`${relativePath}:${line + 1}:${character + 1} missing ${missing}`);
    }

    function visit(node: ts.Node): void {
      if (ts.isJsxElement(node)) {
        checkContent(node, node.openingElement);
      } else if (ts.isJsxSelfClosingElement(node)) {
        checkContent(node, node);
      }

      ts.forEachChild(node, visit);
    }

    visit(source);
  }

  return violations;
}

describe("dialog accessibility contracts", () => {
  it("gives every app dialog content an accessible title and description contract", () => {
    expect(collectDialogContentViolations()).toEqual([]);
  });
});
