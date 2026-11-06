# AGENTS.md

## Skills

Agent skills live in `.agents/skills/` (open Agent Skills `SKILL.md` format). `.claude/skills/` symlinks to the same files for Claude Code.

- **untangle-codebase** (`.agents/skills/untangle-codebase/SKILL.md`): whole-codebase structural refactor. Run only when explicitly asked (e.g. "untangle the codebase"); read the file and follow it end to end.
