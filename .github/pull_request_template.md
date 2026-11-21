<!-- What does this change, and why? -->

- [ ] `npm run build`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --all-targets --locked` pass
- [ ] If a text in the interface changed: `OSPREY_UPDATE_POT=1 cargo test -p osprey-core --test translations`, and its Spanish in `crates/core/translations/es.po`
- [ ] If it changes what users see or get: a line in `CHANGELOG.md` under Unreleased
