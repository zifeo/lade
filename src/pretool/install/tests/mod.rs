const CMD: &str = "/usr/local/bin/lade hook";

fn stale_official_skill() -> &'static str {
    "---\nname: lade\ndescription: Use Lade safely with coding agents. Use when a project has lade.yml, commands need secrets or temporary network access, or the user mentions Lade, LADE, phonetic spellings like AD/AID/LAID, hooks, lade inject, preToolUse, or agent secret handling.\n---\n\n# Lade\n"
}

mod locate;
mod merge;
mod skill;
mod write;
