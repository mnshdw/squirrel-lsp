function onUpdate(actor) {
	# Check if they have a regular shield
	if (actor.getSkills().hasActive(::Legends.Active.Shieldwall)) {
		::Legends.Effects.grant(actor, ::Legends.Effect.Shieldwall);
	}
	# Check if they have a tower shield
	else if (actor.getSkills().hasActive(::Legends.Active.LegendFortify)) {
		::Legends.Effects.grant(actor, ::Legends.Effect.LegendFortify);
	}
	// A line comment before else has to break the line too
	else {
		# do nothing if they have nothing
	}

	local damage = actor.getDamage() # trailing hash comment
	return damage
}
