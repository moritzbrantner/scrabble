# 0001: Rust authority and a static TypeScript client

Accepted. Canonical gameplay belongs in a Rust workspace, separate from browser presentation. The browser uses React, Vite, Tailwind, and the shared @moritzbrantner/ui paper theme. It is built for the GitHub Pages repository path; static hosting does not own multiplayer state. The game-server integration follows in issues #8–#10.

The bootstrap executable intentionally exits without serving; claiming a functioning authoritative service before the transport adapter exists would be misleading. CI and Pages reuse one validated artifact. The owner’s reusable-workflows deploy-pages workflow runs the same check command as PR validation and publishes its resulting artifact without a second build.
