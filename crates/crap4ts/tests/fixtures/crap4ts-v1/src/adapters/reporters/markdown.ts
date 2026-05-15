import type { ReporterPort } from "../../ports/reporter-port.js";
import type { AnalysisResult, FunctionVerdict } from "../../domain/types.js";

export class MarkdownReporter implements ReporterPort {
  format(result: AnalysisResult): string {
    const lines: string[] = [];
    const { summary, thresholdConfig } = result;

    // ── Heading ────────────────────────────────────────────────────────
    lines.push("## crap4ts Report");
    lines.push("");

    // ── Result line ────────────────────────────────────────────────────
    if (result.passed) {
      lines.push("**Result: PASS**");
    } else {
      const threshold = thresholdConfig.defaultThreshold;
      lines.push(
        `**Result: FAIL** | ${summary.exceedingThreshold} of ${summary.totalFunctions} functions above threshold (${threshold})`,
      );
    }
    lines.push("");

    // ── Collect and sort all verdicts ──────────────────────────────────
    if (result.functions.length === 0) {
      return lines.join("\n");
    }

    // Sort by CRAP score descending
    const sorted = [...result.functions].sort(
      (a, b) => b.scored.crap.value - a.scored.crap.value,
    );

    const failing = sorted.filter((v) => v.exceeds);
    const hasFailures = failing.length > 0;
    const hasPassingBeyondFailures =
      hasFailures && failing.length < sorted.length;

    // ── Above-the-fold table ──────────────────────────────────────────
    if (hasPassingBeyondFailures) {
      // Show only failing functions above the fold
      this.appendTable(lines, failing);
      lines.push("");

      // Wrap ALL functions in <details>
      lines.push(
        `<details><summary>Full results (${sorted.length} functions)</summary>`,
      );
      lines.push("");
      this.appendTable(lines, sorted);
      lines.push("");
      lines.push("</details>");
    } else {
      // All functions in a single table (either all pass or all fail)
      this.appendTable(lines, sorted);
    }

    return lines.join("\n");
  }

  private appendTable(lines: string[], verdicts: FunctionVerdict[]): void {
    // Header
    lines.push("| CRAP | CC | Cov% | Function | Location |");
    lines.push("|-----:|---:|-----:|----------|----------|");

    // Data rows
    for (const v of verdicts) {
      const { scored } = v;
      const crap = scored.crap.value.toFixed(1);
      const cc = String(scored.cyclomaticComplexity);
      const cov = `${scored.coveragePercent.toFixed(1)}%`;
      const fn = `\`${scored.identity.qualifiedName}\``;
      const loc = `\`${scored.identity.filePath}:${scored.identity.span.startLine}\``;

      lines.push(`| ${crap} | ${cc} | ${cov} | ${fn} | ${loc} |`);
    }
  }
}
