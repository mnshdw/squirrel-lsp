function removeEffects(target)
{
	while (target.getSkills().hasSkill("effects.goblin_poison"))
	{
		target.getSkills().removeByID("effects.goblin_poison");
	}
	while (target.getSkills().hasSkill("effects.bleeding"))
	{
		target.getSkills().removeByID("effects.bleeding");
	}
	while (target.getSkills().hasSkill("effects.acid"))
	{
		target.getSkills().removeByID("effects.acid");
	}
}

function doWhileExample()
{
	do {
		local x = 1;
	} while (x < 10);
}
