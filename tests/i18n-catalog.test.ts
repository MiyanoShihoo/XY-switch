import { readFileSync, readdirSync } from "node:fs";
import { resolve, join } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import { en } from "../src/i18n/en";
import { navigation, navigationSections } from "../src/shell/SideNav";
import { diagnosticTabs } from "../src/features/providers/ProviderDiagnosticsPage";

describe("translation catalog coverage", () => {
  it("has nonempty English messages with matching named placeholders", () => {
    const slots = (text: string) => [...text.matchAll(/\{([A-Za-z][A-Za-z0-9_]*)\}/g)].map(match => match[1]).sort();
    expect(Object.keys(en).length).toBeGreaterThan(1300);
    for (const [source, target] of Object.entries(en)) {
      expect(target.trim(), source).not.toBe("");
      expect(slots(target), source).toEqual(slots(source));
      if (source !== "简体中文") expect(target, source).not.toMatch(/[\u3400-\u9fff]/u);
    }
  });
  it("covers dynamic navigation sections and diagnostic tabs", () => {
    const labels = [...navigationSections, ...navigation.flatMap(item => [item.label, item.section]), ...diagnosticTabs.map(item => item.label)];
    for (const label of labels) {
      if (/[\u3400-\u9fff]/u.test(label)) expect(en[label], label).toBeTypeOf("string");
    }
  });
  it("covers every literal application translation call", () => {
    const missing: string[] = [];
    function walk(directory: string) {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const full = join(directory, entry.name);
        if (entry.isDirectory()) { if (entry.name !== "i18n") walk(full); continue; }
        if (!/\.tsx?$/.test(entry.name) || entry.name.includes(".test.")) continue;
        const file = ts.createSourceFile(full, readFileSync(full, "utf8"), ts.ScriptTarget.Latest, true, full.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
        function visit(node: ts.Node) {
          if (ts.isCallExpression(node) && ["t", "translate", "translateMessage"].includes(node.expression.getText(file)) && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) {
            const key = node.arguments[0].text;
            if (/[\u3400-\u9fff]/u.test(key) && !Object.hasOwn(en, key)) missing.push(full + ": " + key);
          }
          if (ts.isCallExpression(node) && node.expression.getText(file) === "localizedLabels" && node.arguments[0] && ts.isObjectLiteralExpression(node.arguments[0])) {
            for (const property of node.arguments[0].properties) {
              if (ts.isPropertyAssignment(property) && ts.isStringLiteral(property.initializer)) {
                const key = property.initializer.text;
                if (/[\u3400-\u9fff]/u.test(key) && !Object.hasOwn(en, key)) missing.push(full + ": " + key);
              }
            }
          }
          ts.forEachChild(node, visit);
        }
        visit(file);
      }
    }
    walk(resolve("src"));
    expect(missing).toEqual([]);
  });
});
