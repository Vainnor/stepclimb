# private/ (local only)

Real member exports, organization data sets, database dumps and anything else that must never
enter git. Only this README is committed; `.gitignore`, content-guard and git-guard keep the rest
out (`.claude/rules/content-and-ip.md`). Test fixtures are synthetic and live beside their tests.
