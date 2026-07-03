/**
 * Minimal, dependency-free markdown for assistant output. Escape-first, then
 * a small trusted subset: fenced code, inline code, bold, italic, headings
 * (rendered as bold lines), and unordered list markers. Anything else stays
 * literal text — honesty over cleverness.
 */

import { useMemo } from "react";

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function renderInline(escaped: string): string {
  return escaped
    .replace(/`([^`\n]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[\s(])\*([^*\n]+)\*(?=[\s).,;:!?]|$)/g, "$1<em>$2</em>");
}

export function renderMarkdown(src: string): string {
  const out: string[] = [];
  const lines = src.split("\n");
  let inCode = false;
  let codeBuf: string[] = [];

  for (const raw of lines) {
    if (raw.trimStart().startsWith("```")) {
      if (inCode) {
        out.push(`<pre><code>${codeBuf.join("\n")}</code></pre>`);
        codeBuf = [];
        inCode = false;
      } else {
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      codeBuf.push(escapeHtml(raw));
      continue;
    }
    const escaped = escapeHtml(raw);
    const heading = escaped.match(/^(#{1,4})\s+(.*)$/);
    if (heading) {
      out.push(`<strong class="md-h">${renderInline(heading[2])}</strong>`);
      continue;
    }
    const bullet = escaped.match(/^(\s*)[-*]\s+(.*)$/);
    if (bullet) {
      out.push(`${bullet[1]}<span class="md-bullet">·</span> ${renderInline(bullet[2])}`);
      continue;
    }
    out.push(renderInline(escaped));
  }
  if (inCode && codeBuf.length) {
    out.push(`<pre><code>${codeBuf.join("\n")}</code></pre>`);
  }
  return out.join("\n");
}

export function Markdown({ text }: { text: string }) {
  const html = useMemo(() => renderMarkdown(text), [text]);
  return <div className="md" dangerouslySetInnerHTML={{ __html: html }} />;
}
