/**
 * Lightweight markdown-to-HTML converter for chat responses.
 *
 * Covers the subset used by the mock backend and typical Axinite assistant
 * output: headings, bold/italic/code, lists, tables, and paragraphs.
 * This is intentionally minimal — a full markdown parser would be overkill
 * for the mockup.
 *
 * All input is HTML-escaped before markdown transforms are applied, so
 * untrusted content (user messages, tool output) cannot inject raw HTML.
 */

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

type BlockKind = "none" | "ul" | "ol" | "table";

export function renderMarkdown(source: string): string {
  const escaped = escapeHtml(source);
  const lines = escaped.split("\n");
  const html: string[] = [];
  let block: BlockKind = "none";
  let paragraphBuffer: string[] = [];

  function closeBlock(): void {
    switch (block) {
      case "ul":
        html.push("</ul>");
        break;
      case "ol":
        html.push("</ol>");
        break;
      case "table":
        html.push("</tbody></table>");
        break;
    }
    block = "none";
  }

  function ensureList(kind: "ul" | "ol"): void {
    if (block === kind) return;
    closeBlock();
    html.push(kind === "ul" ? "<ul>" : "<ol>");
    block = kind;
  }

  function inlineMarkup(text: string): string {
    const codeSpans: string[] = [];
    const withPlaceholders = text.replace(/`([^`]+)`/g, (_, content) => {
      const index = codeSpans.length;
      codeSpans.push(content);
      return `\uFFFDCODE${index}\uFFFD`;
    });
    const formatted = withPlaceholders
      .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
      .replace(/\*(.+?)\*/g, "<em>$1</em>");
    return formatted.replace(/\uFFFDCODE(\d+)\uFFFD/g, (match, indexStr) => {
      const span = codeSpans[Number(indexStr)];
      return span !== undefined ? `<code>${span}</code>` : match;
    });
  }

  function flushParagraph(): void {
    if (paragraphBuffer.length > 0) {
      html.push(`<p>${paragraphBuffer.join(" ")}</p>`);
      paragraphBuffer = [];
    }
  }

  function handleTableRow(line: string): boolean {
    if (!(line.startsWith("|") && line.endsWith("|"))) {
      if (block === "table") closeBlock();
      return false;
    }

    flushParagraph();
    const cells = line
      .slice(1, -1)
      .split("|")
      .map((c) => c.trim());

    if (cells.every((c) => /^[-:]+$/.test(c))) {
      return true;
    }

    if (block !== "table") {
      closeBlock();
      html.push("<table>");
      html.push(
        `<thead><tr>${cells.map((c) => `<th>${inlineMarkup(c)}</th>`).join("")}</tr></thead>`
      );
      html.push("<tbody>");
      block = "table";
      return true;
    }

    html.push(
      `<tr>${cells.map((c) => `<td>${inlineMarkup(c)}</td>`).join("")}</tr>`
    );
    return true;
  }

  for (const raw of lines) {
    const line = raw.trimEnd();

    if (handleTableRow(line)) continue;

    // Heading
    const headingMatch = line.match(/^(#{1,6})\s+(.*)/);
    if (headingMatch) {
      flushParagraph();
      closeBlock();
      const level = headingMatch[1].length;
      html.push(`<h${level}>${inlineMarkup(headingMatch[2])}</h${level}>`);
      continue;
    }

    // Unordered list
    if (line.match(/^[-*]\s+/)) {
      flushParagraph();
      ensureList("ul");
      html.push(`<li>${inlineMarkup(line.replace(/^[-*]\s+/, ""))}</li>`);
      continue;
    }

    // Ordered list
    const olMatch = line.match(/^\d+\.\s+(.*)/);
    if (olMatch) {
      flushParagraph();
      ensureList("ol");
      html.push(`<li>${inlineMarkup(olMatch[1])}</li>`);
      continue;
    }

    closeBlock();

    // Empty line = paragraph break
    if (line.length === 0) {
      flushParagraph();
      continue;
    }

    // Regular text — accumulate into paragraph
    paragraphBuffer.push(inlineMarkup(line));
  }

  flushParagraph();
  closeBlock();

  return html.join("\n");
}
