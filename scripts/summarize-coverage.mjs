#!/usr/bin/env node

import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

function parseArgs(argv) {
  const args = {};

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (!arg.startsWith("--")) {
      throw new Error(`Unexpected argument: ${arg}`);
    }

    const key = arg.slice(2);
    const value = argv[index + 1];
    if (!value || value.startsWith("--")) {
      throw new Error(`Missing value for --${key}`);
    }

    args[key] = value;
    index += 1;
  }

  return args;
}

function percent(value) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return "0.00%";
  }

  return `${numeric.toFixed(2)}%`;
}

function count(value) {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return "0";
  }

  return String(Math.round(numeric));
}

function istanbulMetric(label, metric) {
  if (!metric) {
    return null;
  }

  return {
    label,
    covered: metric.covered,
    total: metric.total,
    percent: metric.pct,
  };
}

function llvmMetric(label, metric) {
  if (!metric || metric.count == null) {
    return null;
  }

  const total = Number(metric.count);
  const covered = Number(metric.covered ?? total - Number(metric.notcovered ?? 0));

  return {
    label,
    covered,
    total,
    percent: metric.percent ?? (total > 0 ? (covered / total) * 100 : 0),
  };
}

function parseIstanbul(report) {
  const total = report.total ?? report;

  return [
    istanbulMetric("Lines", total.lines),
    istanbulMetric("Statements", total.statements),
    istanbulMetric("Functions", total.functions),
    istanbulMetric("Branches", total.branches),
  ].filter(Boolean);
}

function parseLlvm(report) {
  const totals = report.data?.[0]?.totals ?? report.totals;
  if (!totals) {
    throw new Error("LLVM coverage JSON does not contain data[0].totals");
  }

  return [
    llvmMetric("Lines", totals.lines),
    llvmMetric("Functions", totals.functions),
    llvmMetric("Regions", totals.regions),
    llvmMetric("Branches", totals.branches),
    llvmMetric("Instantiations", totals.instantiations),
  ].filter(Boolean);
}

function renderMarkdown(suite, rows) {
  const table = [
    `## ${suite}`,
    "",
    "| Metric | Covered | Total | Coverage |",
    "|---|---:|---:|---:|",
    ...rows.map((row) => (
      `| ${row.label} | ${count(row.covered)} | ${count(row.total)} | ${percent(row.percent)} |`
    )),
    "",
  ];

  return `${table.join("\n")}\n`;
}

const args = parseArgs(process.argv.slice(2));
const format = args.format;
const suite = args.suite;
const inputPath = args.input;
const outputPath = args.output;

if (!format || !suite || !inputPath || !outputPath) {
  throw new Error("Usage: summarize-coverage.mjs --format istanbul|llvm --suite <name> --input <path> --output <path>");
}

const report = JSON.parse(readFileSync(inputPath, "utf8"));
const rows = format === "istanbul"
  ? parseIstanbul(report)
  : format === "llvm"
    ? parseLlvm(report)
    : null;

if (!rows) {
  throw new Error(`Unsupported coverage format: ${format}`);
}

if (rows.length === 0) {
  throw new Error(`No coverage metrics found in ${inputPath}`);
}

const markdown = renderMarkdown(suite, rows);
mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, markdown);

if (process.env.GITHUB_STEP_SUMMARY) {
  appendFileSync(process.env.GITHUB_STEP_SUMMARY, markdown);
}
