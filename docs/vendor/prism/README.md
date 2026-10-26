# Prism syntax highlighting

Prism 1.30.0, matching the local assets used by the Luma documentation:
https://gpui-luma.dev/checkbox.html

Files copied from https://gpui-luma.dev/vendor/prism/ on 2026-10-26:

- prism-core.min.js
- prism-rust.min.js
- LICENSE (MIT, copyright Lea Verou)

Upstream: https://prismjs.com/ and https://github.com/PrismJS/prism/tree/v1.30.0

The docs use manual highlighting after route changes. Rust fence flags such as
`rust,ignore` are normalized to Rust. Other languages remain readable plain text.
Token colors follow Luma. Only Rust syntax is highlighted; code blocks have no
added language labels, wrappers or Copy controls. No CDN request is required.
