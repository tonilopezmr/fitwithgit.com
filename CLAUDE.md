### Commit & Ship Process

When asked to "commit", execute the full process end-to-end:

1. **Lint & format** all changed code (see [Before Committing](#before-committing))
2. **Commit** — create a separate commit for each file changed (not one big commit), no `Co-Authored-By` lines
3. **Push** directly to `main`

### Before Committing

Run these commands and fix any issues before committing:

```bash
cargo fmt                      # Format all Rust code
cargo clippy -- -D warnings    # Lint — treat warnings as errors
cargo check                    # Fast compilation check
cargo test                     # Run all tests
```

### Development

```bash
cargo run                      # Start the dev server at http://localhost:3000
cargo build                    # Build in debug mode
cargo build --release          # Build optimized release binary
```

### Tech Stack

- **Backend:** Rust + Axum 0.8
- **Templates:** Askama (compile-time, Jinja2-like)
- **Frontend interactivity:** htmx 2.0
- **Logging:** tracing + tracing-subscriber

### Available Skills

The following Claude Code skills are available in this environment for code review:

- **web-design-guidelines** — Review UI code for Web Interface Guidelines compliance, accessibility audits, and UX best practices
- **vercel-react-best-practices** — React/Next.js performance optimization (useful if frontend is migrated to React in the future)
