# TetraMem V14 Stability Rules

## Iron Rule
System is in stable production. Any optimization MUST prioritize overall stability.

1. **Priority order**: Stability > Functionality > Performance > Code aesthetics
2. **Before changes**: Assess impact scope - which files, which modules affected
3. **After changes**: Verify `cargo check` zero errors + server deploy health check + test affected API endpoints
4. **NO "fix one break another"** - optimizing module A must not introduce bugs in module B
5. **Major changes**: Local `cargo check` first, then server `cargo build --release`
6. **After every deploy**: Run source leak check (no .rs/.toml/.lock on server)
7. **When in doubt**: Don't change it. Stability wins over perfection.

## Deploy Checklist
- [ ] `cargo check` zero errors on Windows
- [ ] tar + scp + `cargo build --release` on server
- [ ] `systemctl stop` → strip → cp → chown tetramem:tetramem → `systemctl start`
- [ ] `curl /health` returns ok
- [ ] Source leak check: no .rs/.toml/.lock in /opt/tetramem/ or /var/www/
- [ ] `rm -rf /root/src_build`
