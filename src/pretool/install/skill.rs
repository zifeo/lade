pub(super) const SKILL_MD: &str = include_str!("../../../.agents/skills/lade/SKILL.md");

pub(super) fn is_lade_skill(content: &str) -> bool {
    content.contains("\nname: lade\n") && content.contains("Use Lade safely with coding agents.")
}

pub(super) fn skill_is_current(content: &str) -> bool {
    content == SKILL_MD
}
