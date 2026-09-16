/** node has no DOM, so the real DOMPurify cannot run (its default export is a
    factory that needs a window). The benchmark measures the markdown-it +
    highlight.js cost — the quadratic term in efficiency.md §3.4 — and sanitize
    is a constant per render that is identical on every path compared here. */
export default { sanitize: (html) => html };
